use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::{GraphOps, StorageEngine, VectorIndex};
use astraea_core::types::*;
use astraea_vector::HnswVectorIndex;

use crate::traversal;

/// Reports the outcome of [`Graph::load_or_rebuild_vector_index`].
///
/// Callers can log or assert on this to distinguish a fast snapshot-load
/// path from a full O(n·log n) rebuild.
#[derive(Debug, PartialEq)]
pub enum VectorIndexInit {
    /// The snapshot file was loaded successfully.
    ///
    /// Delta-reconcile added `inserted` embeddings for nodes that were
    /// written to storage after the last snapshot (WAL-durable, snapshot-
    /// not-yet-written), and stripped `removed` node ids from the index
    /// for nodes that were deleted after the last snapshot.
    Loaded { inserted: usize, removed: usize },

    /// The snapshot was missing, corrupt, had a dimension mismatch, or had a
    /// metric mismatch (e.g. the operator changed the configured metric since
    /// the last snapshot was saved).
    ///
    /// A fresh index was constructed and the full O(n) scan-and-reinsert
    /// fallback ran.  `count` is the number of embeddings inserted.
    /// The rebuilt index was also saved so the next restart is fast.
    Rebuilt { count: usize },
}

/// The primary graph database handle.
///
/// Wraps a `StorageEngine` and provides high-level graph operations
/// including CRUD, traversals, and path finding.
pub struct Graph {
    storage: Box<dyn StorageEngine>,
    next_node_id: AtomicU64,
    next_edge_id: AtomicU64,
    vector_index: Option<Arc<dyn VectorIndex>>,
}

impl Graph {
    /// Create a new graph backed by the given storage engine.
    pub fn new(storage: Box<dyn StorageEngine>) -> Self {
        Self {
            storage,
            next_node_id: AtomicU64::new(1),
            next_edge_id: AtomicU64::new(1),
            vector_index: None,
        }
    }

    /// Create a new graph with an attached vector index.
    pub fn with_vector_index(
        storage: Box<dyn StorageEngine>,
        vector_index: Arc<dyn VectorIndex>,
    ) -> Self {
        Self {
            storage,
            next_node_id: AtomicU64::new(1),
            next_edge_id: AtomicU64::new(1),
            vector_index: Some(vector_index),
        }
    }

    /// Create a new graph with explicit starting IDs (for recovery).
    pub fn with_start_ids(
        storage: Box<dyn StorageEngine>,
        next_node_id: u64,
        next_edge_id: u64,
    ) -> Self {
        Self {
            storage,
            next_node_id: AtomicU64::new(next_node_id),
            next_edge_id: AtomicU64::new(next_edge_id),
            vector_index: None,
        }
    }

    fn alloc_node_id(&self) -> NodeId {
        NodeId(self.next_node_id.fetch_add(1, Ordering::Relaxed))
    }

    fn alloc_edge_id(&self) -> EdgeId {
        EdgeId(self.next_edge_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Get a reference to the underlying storage engine.
    pub fn storage(&self) -> &dyn StorageEngine {
        self.storage.as_ref()
    }

    /// Set the vector index after construction.
    pub fn set_vector_index(&mut self, index: Arc<dyn VectorIndex>) {
        self.vector_index = Some(index);
    }

    /// Get a reference to the vector index, if configured.
    pub fn vector_index(&self) -> Option<&Arc<dyn VectorIndex>> {
        self.vector_index.as_ref()
    }

    /// Rebuild the in-memory vector index by scanning every node in storage.
    ///
    /// Iterates all node IDs returned by [`StorageEngine::list_all_nodes`],
    /// fetches each node, and inserts any node that carries an embedding into
    /// the attached [`VectorIndex`].  Nodes without embeddings are skipped.
    ///
    /// This is O(n) over stored nodes and is the correct way to restore HNSW
    /// consistency after a WAL replay / restart.  Call it after constructing a
    /// `Graph` over a recovered [`DiskStorageEngine`] and attaching a fresh
    /// vector index via [`set_vector_index`]:
    ///
    /// ```rust,ignore
    /// let (engine, max_node_id, max_edge_id) = DiskStorageEngine::open(data_dir)?;
    /// let mut graph = Graph::with_start_ids(Box::new(engine), max_node_id + 1, max_edge_id + 1);
    /// graph.set_vector_index(Arc::new(HnswVectorIndex::new(768, DistanceMetric::Cosine)));
    /// graph.rebuild_vector_index()?;
    /// ```
    ///
    /// Returns the number of embeddings inserted into the index.
    /// Returns `Ok(0)` immediately if no vector index is attached.
    pub fn rebuild_vector_index(&self) -> astraea_core::error::Result<usize> {
        let vi = match &self.vector_index {
            Some(vi) => vi,
            None => return Ok(0),
        };

        let node_ids = self.storage.list_all_nodes()?;
        let mut count = 0usize;
        for id in node_ids {
            let node = match self.storage.get_node(id)? {
                Some(n) => n,
                None => continue,
            };
            if let Some(ref emb) = node.embedding {
                vi.insert(id, emb)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Persist the attached vector index to `path` using an **atomic write**.
    ///
    /// Internally writes to `<path>.tmp` and then renames it over `path`, so a
    /// crash mid-write never leaves a torn `.hnsw` file on disk.
    ///
    /// If no vector index is attached, this is a no-op and returns `Ok(())`.
    /// If the index implementation does not support persistence (i.e., its
    /// [`VectorIndex::save_to_path`] returns an error), that error is propagated.
    pub fn save_vector_index(&self, path: &Path) -> Result<()> {
        let vi = match &self.vector_index {
            Some(vi) => vi,
            None => return Ok(()),
        };

        // Build the temp path as `<path>.tmp` in the same directory.
        let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(".tmp");
        let tmp_path = path.with_file_name(tmp_name);

        // Write to temp; rename atomically.  If the rename fails we leave
        // the tmp file behind (harmless — it will be overwritten next time).
        vi.save_to_path(&tmp_path)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Load the vector index from `path`, attach it, and delta-reconcile it
    /// against storage.  Falls back to a full rebuild when the file is
    /// missing, corrupt, or has a dimension mismatch.
    ///
    /// # Load + reconcile (fast path)
    ///
    /// 1. Deserialise the snapshot from `path`.
    /// 2. Check dimension against `dim`; return [`AstraeaError::DimensionMismatch`]
    ///    as the trigger for the fallback (not a hard error).
    /// 3. Compute the symmetric diff between `storage.list_all_nodes()` and
    ///    `index.node_ids()`:
    ///    - **Missing** (in storage, not in index): nodes written to the WAL after
    ///      the last snapshot — re-insert their embeddings.
    ///    - **Extra** (in index, not in storage): nodes deleted after the last
    ///      snapshot — remove them from the index.
    ///
    /// # Rebuild fallback
    ///
    /// On any load error, construct a fresh `HnswVectorIndex::new(dim, metric)`,
    /// call [`rebuild_vector_index`], and then [`save_vector_index`] so the next
    /// restart is fast.  The save failure is non-fatal (only logged).
    ///
    /// Returns a [`VectorIndexInit`] so callers can log or assert on which path
    /// was taken and how many deltas were applied.
    pub fn load_or_rebuild_vector_index(
        &mut self,
        path: &Path,
        dim: usize,
        metric: DistanceMetric,
    ) -> Result<VectorIndexInit> {
        use std::collections::HashSet;

        // Attempt to deserialise the snapshot.
        let load_result: Result<HnswVectorIndex> = (|| {
            let loaded = HnswVectorIndex::load_from_file(path)?;
            if loaded.dimension() != dim {
                return Err(AstraeaError::DimensionMismatch {
                    expected: dim,
                    got: loaded.dimension(),
                });
            }
            if loaded.metric() != metric {
                return Err(AstraeaError::Config(format!(
                    "metric mismatch: expected {:?}, got {:?}",
                    metric,
                    loaded.metric(),
                )));
            }
            Ok(loaded)
        })();

        match load_result {
            Ok(loaded_idx) => {
                // Attach the loaded index; keep a local Arc for the reconcile
                // loop so we can call insert/remove without going through self.
                let vi: Arc<dyn VectorIndex> = Arc::new(loaded_idx);
                self.set_vector_index(vi.clone());

                // Snapshot ids vs. current storage ids (post-WAL-replay).
                let storage_ids: HashSet<NodeId> =
                    self.storage.list_all_nodes()?.into_iter().collect();
                let index_ids: HashSet<NodeId> = vi.node_ids().into_iter().collect();

                // Collect both diff directions before mutating.
                let missing: Vec<NodeId> = storage_ids.difference(&index_ids).copied().collect();
                let extra: Vec<NodeId> = index_ids.difference(&storage_ids).copied().collect();

                // LIMITATION: this reconcile only covers *existence* deltas —
                // nodes written to the WAL after the snapshot (missing from the
                // index) and nodes deleted after the snapshot (extra in the
                // index).  A node present in BOTH sets whose embedding was
                // *updated* in storage after the snapshot was saved will keep
                // the stale vector from the snapshot until the next full
                // rebuild.  Correcting this would require per-node
                // versioning/generation tracking so the reconcile can detect
                // changed content, not just changed membership.
                // Tracked as a follow-up.

                // Post-snapshot inserts: WAL-replayed but not yet snapshotted.
                let mut inserted = 0usize;
                for id in missing {
                    if let Some(node) = self.storage.get_node(id)?
                        && let Some(ref emb) = node.embedding
                    {
                        vi.insert(id, emb)?;
                        inserted += 1;
                    }
                }

                // Post-snapshot deletes: still in snapshot but gone from storage.
                let mut removed = 0usize;
                for id in extra {
                    vi.remove(id)?;
                    removed += 1;
                }

                Ok(VectorIndexInit::Loaded { inserted, removed })
            }

            Err(load_err) => {
                // Missing file, corrupt magic/version/body, or dimension mismatch.
                tracing::warn!(
                    "vector index load from {:?} failed ({}); rebuilding from storage",
                    path,
                    load_err
                );

                let fresh = Arc::new(HnswVectorIndex::new(dim, metric));
                self.set_vector_index(fresh);
                let count = self.rebuild_vector_index()?;

                // Persist the freshly built index — best-effort, non-fatal.
                if let Err(e) = self.save_vector_index(path) {
                    tracing::warn!(
                        "failed to persist rebuilt vector index to {:?}: {}",
                        path,
                        e
                    );
                }

                Ok(VectorIndexInit::Rebuilt { count })
            }
        }
    }
}

impl GraphOps for Graph {
    fn create_node(
        &self,
        labels: Vec<String>,
        properties: serde_json::Value,
        embedding: Option<Vec<f32>>,
    ) -> Result<NodeId> {
        let id = self.alloc_node_id();
        let node = Node {
            id,
            labels,
            properties,
            embedding,
        };
        self.storage.put_node(&node)?;

        // Auto-index embedding in vector index if present.
        //
        // astraeadb-issues.md #20: failures used to be logged and swallowed,
        // leaving the vector index silently out of sync with the graph.
        // We now roll back the node insert so storage and vector stay
        // consistent, and propagate the error to the caller.
        if let (Some(vi), Some(emb)) = (&self.vector_index, &node.embedding)
            && let Err(e) = vi.insert(node.id, emb)
        {
            tracing::error!(
                "vector index insert failed for node {}: {}; rolling back storage put",
                node.id,
                e
            );
            // Best-effort rollback; if this fails we have bigger problems
            // and the original vector error is the more informative one.
            let _ = self.storage.delete_node(node.id);
            return Err(e);
        }

        Ok(id)
    }

    fn create_node_with_id(
        &self,
        id: NodeId,
        labels: Vec<String>,
        properties: serde_json::Value,
        embedding: Option<Vec<f32>>,
    ) -> Result<NodeId> {
        // astraeadb-issues.md #14. Refuse if the id is already in use —
        // accept-and-overwrite would be surprising for an import path.
        if self.storage.get_node(id)?.is_some() {
            return Err(AstraeaError::DuplicateNode(id));
        }

        let node = Node {
            id,
            labels,
            properties,
            embedding,
        };
        self.storage.put_node(&node)?;

        // Auto-index embedding (same rollback discipline as `create_node`).
        if let (Some(vi), Some(emb)) = (&self.vector_index, &node.embedding)
            && let Err(e) = vi.insert(node.id, emb)
        {
            tracing::error!(
                "vector index insert failed for node {}: {}; rolling back storage put",
                node.id,
                e
            );
            let _ = self.storage.delete_node(node.id);
            return Err(e);
        }

        // Bump the auto-allocator past this id so subsequent
        // `create_node` calls don't collide.
        let next = id.0.saturating_add(1);
        self.next_node_id
            .fetch_max(next, std::sync::atomic::Ordering::SeqCst);

        Ok(id)
    }

    fn create_edge(
        &self,
        source: NodeId,
        target: NodeId,
        edge_type: String,
        properties: serde_json::Value,
        weight: f64,
        valid_from: Option<i64>,
        valid_to: Option<i64>,
    ) -> Result<EdgeId> {
        // Verify both endpoints exist.
        if self.storage.get_node(source)?.is_none() {
            return Err(AstraeaError::NodeNotFound(source));
        }
        if self.storage.get_node(target)?.is_none() {
            return Err(AstraeaError::NodeNotFound(target));
        }

        let id = self.alloc_edge_id();
        let edge = Edge {
            id,
            source,
            target,
            edge_type,
            properties,
            weight,
            validity: ValidityInterval {
                valid_from,
                valid_to,
            },
        };
        self.storage.put_edge(&edge)?;
        Ok(id)
    }

    fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        self.storage.get_node(id)
    }

    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
        self.storage.get_edge(id)
    }

    fn update_node(&self, id: NodeId, properties: serde_json::Value) -> Result<()> {
        let mut node = self
            .storage
            .get_node(id)?
            .ok_or(AstraeaError::NodeNotFound(id))?;

        merge_json(&mut node.properties, properties);
        self.storage.put_node(&node)
    }

    fn update_edge(&self, id: EdgeId, properties: serde_json::Value) -> Result<()> {
        let mut edge = self
            .storage
            .get_edge(id)?
            .ok_or(AstraeaError::EdgeNotFound(id))?;

        merge_json(&mut edge.properties, properties);
        self.storage.put_edge(&edge)
    }

    fn delete_node(&self, id: NodeId) -> Result<()> {
        // Remove from vector index if present. A not-in-index error is
        // expected (node may have had no embedding), so we log at error
        // level and continue. astraeadb-issues.md #20: the previous
        // `let _ = ...` hid real index-corruption errors entirely.
        if let Some(ref vi) = self.vector_index
            && let Err(e) = vi.remove(id)
        {
            tracing::error!("vector index remove failed for node {}: {}", id, e);
        }

        // Delete all connected edges first (both directions).
        let outgoing = self.storage.get_edges(id, Direction::Outgoing)?;
        let incoming = self.storage.get_edges(id, Direction::Incoming)?;

        for edge in outgoing.iter().chain(incoming.iter()) {
            self.storage.delete_edge(edge.id)?;
        }

        self.storage.delete_node(id)?;
        Ok(())
    }

    fn delete_edge(&self, id: EdgeId) -> Result<()> {
        self.storage.delete_edge(id)?;
        Ok(())
    }

    fn neighbors(&self, node_id: NodeId, direction: Direction) -> Result<Vec<(EdgeId, NodeId)>> {
        let edges = self.storage.get_edges(node_id, direction)?;
        Ok(edges
            .into_iter()
            .map(|e| {
                let neighbor = if e.source == node_id {
                    e.target
                } else {
                    e.source
                };
                (e.id, neighbor)
            })
            .collect())
    }

    fn neighbors_filtered(
        &self,
        node_id: NodeId,
        direction: Direction,
        edge_type: &str,
    ) -> Result<Vec<(EdgeId, NodeId)>> {
        let edges = self.storage.get_edges(node_id, direction)?;
        Ok(edges
            .into_iter()
            .filter(|e| e.edge_type == edge_type)
            .map(|e| {
                let neighbor = if e.source == node_id {
                    e.target
                } else {
                    e.source
                };
                (e.id, neighbor)
            })
            .collect())
    }

    fn bfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<(NodeId, usize)>> {
        traversal::bfs(self.storage.as_ref(), start, max_depth)
    }

    fn dfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<NodeId>> {
        traversal::dfs(self.storage.as_ref(), start, max_depth)
    }

    fn shortest_path(&self, from: NodeId, to: NodeId) -> Result<Option<GraphPath>> {
        traversal::shortest_path_unweighted(self.storage.as_ref(), from, to)
    }

    fn shortest_path_weighted(&self, from: NodeId, to: NodeId) -> Result<Option<(GraphPath, f64)>> {
        traversal::shortest_path_dijkstra(self.storage.as_ref(), from, to)
    }

    fn find_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        self.storage.find_nodes_by_label(label)
    }

    fn find_edges_by_type(&self, edge_type: &str) -> Result<Vec<(EdgeId, NodeId, NodeId)>> {
        self.storage.find_edges_by_type(edge_type)
    }

    fn neighbors_at(
        &self,
        node_id: NodeId,
        direction: Direction,
        timestamp: i64,
    ) -> Result<Vec<(EdgeId, NodeId)>> {
        let edges = self.storage.get_edges(node_id, direction)?;
        Ok(edges
            .into_iter()
            .filter(|e| e.validity.contains(timestamp))
            .map(|e| {
                let neighbor = if e.source == node_id {
                    e.target
                } else {
                    e.source
                };
                (e.id, neighbor)
            })
            .collect())
    }

    fn bfs_at(
        &self,
        start: NodeId,
        max_depth: usize,
        timestamp: i64,
    ) -> Result<Vec<(NodeId, usize)>> {
        traversal::bfs_at(self.storage.as_ref(), start, max_depth, timestamp)
    }

    fn shortest_path_at(
        &self,
        from: NodeId,
        to: NodeId,
        timestamp: i64,
    ) -> Result<Option<GraphPath>> {
        traversal::shortest_path_unweighted_at(self.storage.as_ref(), from, to, timestamp)
    }

    fn shortest_path_weighted_at(
        &self,
        from: NodeId,
        to: NodeId,
        timestamp: i64,
    ) -> Result<Option<(GraphPath, f64)>> {
        traversal::shortest_path_dijkstra_at(self.storage.as_ref(), from, to, timestamp)
    }

    /// Flush dirty buffer-pool pages to disk via the underlying storage engine.
    ///
    /// Delegates to [`StorageEngine::flush`]. Called by the server on clean
    /// shutdown (SIGTERM / SIGINT) so the buffer pool is persisted even when
    /// WAL replay would recover the data on next startup.
    ///
    /// astraeadb-issues.md #1.
    fn flush(&self) -> Result<()> {
        self.storage.flush()
    }

    fn hybrid_search(
        &self,
        anchor: NodeId,
        query_embedding: &[f32],
        max_hops: usize,
        k: usize,
        alpha: f32,
    ) -> Result<Vec<(NodeId, f32)>> {
        use astraea_core::types::DistanceMetric;
        use astraea_vector::distance::compute_distance;

        // Step 1: BFS to collect candidates with their depths.
        let bfs_results = self.bfs(anchor, max_hops)?;

        // Determine the distance metric from the vector index, defaulting to Cosine.
        let metric = self
            .vector_index
            .as_ref()
            .map(|vi| vi.metric())
            .unwrap_or(DistanceMetric::Cosine);

        // Step 2-4: Score each candidate.
        let mut scored: Vec<(NodeId, f32)> = Vec::new();

        for (node_id, depth) in &bfs_results {
            // Skip the anchor node itself.
            if *node_id == anchor {
                continue;
            }

            let node = match self.get_node(*node_id)? {
                Some(n) => n,
                None => continue,
            };

            // Graph distance score: closer hops = lower score (better).
            let graph_score = *depth as f32 / (max_hops as f32 + 1.0);

            // Vector distance score (if embedding available).
            let vector_score = if let Some(ref emb) = node.embedding {
                match compute_distance(metric, query_embedding, emb) {
                    Ok(d) => d,
                    Err(_) => continue, // skip on dimension mismatch
                }
            } else {
                1.0 // max distance for nodes without embeddings
            };

            // Blend scores.
            let final_score = alpha * vector_score + (1.0 - alpha) * graph_score;
            scored.push((*node_id, final_score));
        }

        // Step 5: Sort by score ascending (lower = better), take top-k.
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);

        Ok(scored)
    }

    fn semantic_neighbors(
        &self,
        node_id: NodeId,
        concept_embedding: &[f32],
        direction: Direction,
        k: usize,
    ) -> Result<Vec<(NodeId, f32)>> {
        use astraea_core::types::DistanceMetric;
        use astraea_vector::distance::compute_distance;

        let metric = self
            .vector_index
            .as_ref()
            .map(|vi| vi.metric())
            .unwrap_or(DistanceMetric::Cosine);

        let neighbors = self.neighbors(node_id, direction)?;

        let mut scored: Vec<(NodeId, f32)> = Vec::new();

        for (_edge_id, neighbor_id) in neighbors {
            let node = match self.get_node(neighbor_id)? {
                Some(n) => n,
                None => continue,
            };

            if let Some(ref emb) = node.embedding {
                match compute_distance(metric, concept_embedding, emb) {
                    Ok(d) => scored.push((neighbor_id, d)),
                    Err(_) => continue,
                }
            }
        }

        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);

        Ok(scored)
    }

    fn semantic_walk(
        &self,
        start: NodeId,
        concept_embedding: &[f32],
        max_hops: usize,
    ) -> Result<Vec<(NodeId, f32)>> {
        use astraea_core::types::DistanceMetric;
        use astraea_vector::distance::compute_distance;

        let metric = self
            .vector_index
            .as_ref()
            .map(|vi| vi.metric())
            .unwrap_or(DistanceMetric::Cosine);

        let mut path: Vec<(NodeId, f32)> = Vec::new();
        let mut current = start;
        let mut visited = std::collections::HashSet::new();
        visited.insert(current);

        // Score the starting node.
        if let Some(node) = self.get_node(current)?
            && let Some(ref emb) = node.embedding
            && let Ok(d) = compute_distance(metric, concept_embedding, emb)
        {
            path.push((current, d));
        }

        for _ in 0..max_hops {
            let neighbors = self.neighbors(current, Direction::Outgoing)?;

            let mut best: Option<(NodeId, f32)> = None;

            for (_edge_id, neighbor_id) in neighbors {
                if visited.contains(&neighbor_id) {
                    continue;
                }

                let node = match self.get_node(neighbor_id)? {
                    Some(n) => n,
                    None => continue,
                };

                if let Some(ref emb) = node.embedding
                    && let Ok(d) = compute_distance(metric, concept_embedding, emb)
                    && (best.is_none() || d < best.unwrap().1)
                {
                    best = Some((neighbor_id, d));
                }
            }

            match best {
                Some((next_id, dist)) => {
                    visited.insert(next_id);
                    path.push((next_id, dist));
                    current = next_id;
                }
                None => break, // no unvisited neighbors with embeddings
            }
        }

        Ok(path)
    }
}

/// Merge `patch` into `target` with JSON object merge semantics.
/// - If both are objects, keys from patch are inserted/overwritten.
/// - Otherwise, target is replaced by patch.
fn merge_json(target: &mut serde_json::Value, patch: serde_json::Value) {
    if let (Some(target_map), serde_json::Value::Object(patch_map)) =
        (target.as_object_mut(), &patch)
    {
        for (key, value) in patch_map {
            target_map.insert(key.clone(), value.clone());
        }
    } else {
        *target = patch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::InMemoryStorage;

    #[test]
    fn create_and_get_node() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let id = graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Alice"}),
                None,
            )
            .unwrap();

        let node = graph.get_node(id).unwrap().unwrap();
        assert_eq!(node.id, id);
        assert_eq!(node.labels, vec!["Person"]);
        assert_eq!(node.properties["name"], "Alice");
    }

    #[test]
    fn create_and_get_edge() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let e = graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();

        let edge = graph.get_edge(e).unwrap().unwrap();
        assert_eq!(edge.source, a);
        assert_eq!(edge.target, b);
        assert_eq!(edge.edge_type, "KNOWS");
    }

    #[test]
    fn create_edge_missing_node_fails() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();

        let result = graph.create_edge(
            a,
            NodeId(999),
            "KNOWS".into(),
            serde_json::json!({}),
            1.0,
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn update_node_properties() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let id = graph
            .create_node(
                vec![],
                serde_json::json!({"name": "Alice", "age": 30}),
                None,
            )
            .unwrap();

        graph
            .update_node(id, serde_json::json!({"age": 31, "city": "NYC"}))
            .unwrap();

        let node = graph.get_node(id).unwrap().unwrap();
        assert_eq!(node.properties["name"], "Alice");
        assert_eq!(node.properties["age"], 31);
        assert_eq!(node.properties["city"], "NYC");
    }

    #[test]
    fn delete_node_cascades_edges() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let e = graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();

        graph.delete_node(a).unwrap();

        assert!(graph.get_node(a).unwrap().is_none());
        assert!(graph.get_edge(e).unwrap().is_none());
        // b should still exist
        assert!(graph.get_node(b).unwrap().is_some());
    }

    #[test]
    fn neighbors_both_directions() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let c = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();

        graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(
                c,
                a,
                "FOLLOWS".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        let out = graph.neighbors(a, Direction::Outgoing).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, b);

        let inc = graph.neighbors(a, Direction::Incoming).unwrap();
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0].1, c);

        let both = graph.neighbors(a, Direction::Both).unwrap();
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn neighbors_filtered_by_type() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let c = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();

        graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(
                a,
                c,
                "FOLLOWS".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        let knows = graph
            .neighbors_filtered(a, Direction::Outgoing, "KNOWS")
            .unwrap();
        assert_eq!(knows.len(), 1);
        assert_eq!(knows[0].1, b);
    }

    #[test]
    fn merge_json_objects() {
        let mut target = serde_json::json!({"a": 1, "b": 2});
        merge_json(&mut target, serde_json::json!({"b": 3, "c": 4}));
        assert_eq!(target, serde_json::json!({"a": 1, "b": 3, "c": 4}));
    }

    #[test]
    fn merge_json_non_object_replaces() {
        let mut target = serde_json::json!({"a": 1});
        merge_json(&mut target, serde_json::json!(42));
        assert_eq!(target, serde_json::json!(42));
    }
}

#[cfg(test)]
mod semantic_tests {
    use super::*;
    use crate::test_utils::InMemoryStorage;
    use astraea_core::types::DistanceMetric;
    use astraea_vector::HnswVectorIndex;
    use std::sync::Arc;

    /// Build a graph with 5 nodes that have 3-dimensional embeddings and directional edges.
    ///
    /// Topology (all edges are outgoing from source):
    ///   n1 -> n3, n1 -> n4
    ///   n3 -> n5
    ///   n2 -> n4
    ///   n4 -> n5
    ///
    /// Embeddings (Euclidean space):
    ///   n1: [1.0, 0.0, 0.0]  -- "concept A"
    ///   n2: [0.0, 1.0, 0.0]  -- "concept B"
    ///   n3: [0.9, 0.1, 0.0]  -- "close to A"
    ///   n4: [0.1, 0.9, 0.0]  -- "close to B"
    ///   n5: [0.0, 0.0, 1.0]  -- "concept C"
    fn make_semantic_graph() -> (Graph, NodeId, NodeId, NodeId, NodeId, NodeId) {
        let storage = InMemoryStorage::new();
        let vi = Arc::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), vi);

        let n1 = graph
            .create_node(
                vec!["Thing".into()],
                serde_json::json!({"name": "A"}),
                Some(vec![1.0, 0.0, 0.0]),
            )
            .unwrap();
        let n2 = graph
            .create_node(
                vec!["Thing".into()],
                serde_json::json!({"name": "B"}),
                Some(vec![0.0, 1.0, 0.0]),
            )
            .unwrap();
        let n3 = graph
            .create_node(
                vec!["Thing".into()],
                serde_json::json!({"name": "closeA"}),
                Some(vec![0.9, 0.1, 0.0]),
            )
            .unwrap();
        let n4 = graph
            .create_node(
                vec!["Thing".into()],
                serde_json::json!({"name": "closeB"}),
                Some(vec![0.1, 0.9, 0.0]),
            )
            .unwrap();
        let n5 = graph
            .create_node(
                vec!["Thing".into()],
                serde_json::json!({"name": "C"}),
                Some(vec![0.0, 0.0, 1.0]),
            )
            .unwrap();

        // Edges: n1->n3, n1->n4, n3->n5, n2->n4, n4->n5
        graph
            .create_edge(
                n1,
                n3,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                n1,
                n4,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                n3,
                n5,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                n2,
                n4,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                n4,
                n5,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        (graph, n1, n2, n3, n4, n5)
    }

    #[test]
    fn test_hybrid_search_alpha_0_pure_graph() {
        let (graph, n1, _n2, n3, n4, _n5) = make_semantic_graph();

        // alpha=0 means pure graph proximity -- vector similarity is ignored.
        let results = graph
            .hybrid_search(n1, &[0.0, 1.0, 0.0], 2, 10, 0.0)
            .unwrap();

        // All BFS reachable nodes from n1 within 2 hops (excluding n1):
        // depth 1: n3, n4   -> graph_score = 1/3 = 0.333
        // depth 2: n5       -> graph_score = 2/3 = 0.667
        assert!(!results.is_empty());

        // Depth-1 nodes should rank before depth-2 nodes.
        let depth1_ids: Vec<NodeId> = results
            .iter()
            .filter(|(_, s)| *s < 0.5)
            .map(|(id, _)| *id)
            .collect();
        assert!(depth1_ids.contains(&n3));
        assert!(depth1_ids.contains(&n4));
    }

    #[test]
    fn test_hybrid_search_alpha_1_pure_vector() {
        let (graph, n1, _n2, n3, n4, n5) = make_semantic_graph();

        // alpha=1 means pure vector similarity.
        // Query embedding [1.0, 0.0, 0.0] is exactly n1's embedding.
        // Among BFS-reachable nodes: n3 [0.9,0.1,0] is closest, then n4 [0.1,0.9,0], then n5 [0,0,1].
        let results = graph
            .hybrid_search(n1, &[1.0, 0.0, 0.0], 2, 10, 1.0)
            .unwrap();

        assert!(results.len() >= 2);
        // n3 should be the top result (closest to [1,0,0] in Euclidean space).
        assert_eq!(results[0].0, n3);
        // n4 should come next.
        assert_eq!(results[1].0, n4);
        // n5 should be last.
        assert_eq!(results[2].0, n5);
    }

    #[test]
    fn test_hybrid_search_blended() {
        let (graph, n1, _n2, n3, n4, _n5) = make_semantic_graph();

        // alpha=0.5 blends graph and vector equally.
        // Query: [0.1, 0.9, 0.0] -- semantically close to n4.
        let results = graph
            .hybrid_search(n1, &[0.1, 0.9, 0.0], 1, 10, 0.5)
            .unwrap();

        // Within 1 hop from n1: n3 and n4
        assert_eq!(results.len(), 2);
        // n4's embedding [0.1, 0.9, 0.0] is an exact match for the query,
        // so its vector_score=0, giving it a lower blended score than n3.
        assert_eq!(results[0].0, n4);
        assert_eq!(results[1].0, n3);
    }

    #[test]
    fn test_semantic_neighbors_ranking() {
        let (graph, n1, _n2, n3, n4, _n5) = make_semantic_graph();

        // From n1, outgoing neighbors are n3 and n4.
        // Query concept [1,0,0] should rank n3 (closeA, [0.9,0.1,0]) above n4 (closeB, [0.1,0.9,0]).
        let results = graph
            .semantic_neighbors(n1, &[1.0, 0.0, 0.0], Direction::Outgoing, 10)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, n3); // n3 is closer to [1,0,0]
        assert_eq!(results[1].0, n4);
        // n3's distance should be less than n4's.
        assert!(results[0].1 < results[1].1);
    }

    #[test]
    fn test_semantic_neighbors_limits_k() {
        let (graph, n1, _n2, _n3, _n4, _n5) = make_semantic_graph();

        // Request k=1, should only get the most similar neighbor.
        let results = graph
            .semantic_neighbors(n1, &[1.0, 0.0, 0.0], Direction::Outgoing, 1)
            .unwrap();

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_semantic_walk_toward_concept() {
        let (graph, n1, _n2, _n3, _n4, n5) = make_semantic_graph();

        // Walk from n1 toward concept [0,0,1] (n5's embedding).
        // From n1, outgoing neighbors: n3 [0.9,0.1,0] and n4 [0.1,0.9,0].
        //   dist(n3, concept) = sqrt(0.81+0.01+1) = sqrt(1.82) ~ 1.349
        //   dist(n4, concept) = sqrt(0.01+0.81+1) = sqrt(1.82) ~ 1.349
        // Both are equidistant, so either could be picked first.
        // Then from whichever is picked, n5 [0,0,1] should be the next step (exact match to concept).
        let path = graph.semantic_walk(n1, &[0.0, 0.0, 1.0], 5).unwrap();

        // Path should start at n1 and end at n5.
        assert!(path.len() >= 2);
        assert_eq!(path[0].0, n1);
        assert_eq!(path.last().unwrap().0, n5);

        // The last step (n5) should have distance 0 (exact match).
        assert!(path.last().unwrap().1 < 1e-6);
    }

    #[test]
    fn test_semantic_walk_path_contains_intermediate() {
        let (graph, n1, _n2, n3, n4, n5) = make_semantic_graph();

        // Walk toward concept [0, 0, 1] from n1.
        let path = graph.semantic_walk(n1, &[0.0, 0.0, 1.0], 5).unwrap();

        // Path should be n1 -> (n3 or n4) -> n5.
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].0, n1);
        // The intermediate node must be either n3 or n4.
        assert!(path[1].0 == n3 || path[1].0 == n4);
        assert_eq!(path[2].0, n5);
    }

    #[test]
    fn test_semantic_walk_stops_at_dead_end() {
        let (graph, _n1, _n2, _n3, _n4, n5) = make_semantic_graph();

        // Walk from n5 -- it has no outgoing edges, so walk should stop immediately.
        let path = graph.semantic_walk(n5, &[1.0, 0.0, 0.0], 5).unwrap();

        assert_eq!(path.len(), 1);
        assert_eq!(path[0].0, n5);
    }

    #[test]
    fn test_semantic_neighbors_no_embedding() {
        let storage = InMemoryStorage::new();
        let vi = Arc::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), vi);

        let n1 = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![1.0, 0.0, 0.0]))
            .unwrap();
        // n2 has no embedding.
        let n2 = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        graph
            .create_edge(
                n1,
                n2,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        // Semantic neighbors should exclude n2 (no embedding).
        let results = graph
            .semantic_neighbors(n1, &[1.0, 0.0, 0.0], Direction::Outgoing, 10)
            .unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn test_hybrid_search_no_vector_index() {
        // Graph without a vector index should still work (uses Cosine default).
        let storage = InMemoryStorage::new();
        let graph = Graph::new(Box::new(storage));

        let n1 = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![1.0, 0.0, 0.0]))
            .unwrap();
        let n2 = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![0.0, 1.0, 0.0]))
            .unwrap();
        graph
            .create_edge(
                n1,
                n2,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        let results = graph
            .hybrid_search(n1, &[1.0, 0.0, 0.0], 1, 10, 0.5)
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, n2);
    }

    #[test]
    fn test_semantic_walk_avoids_cycles() {
        let storage = InMemoryStorage::new();
        let vi = Arc::new(HnswVectorIndex::new(2, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), vi);

        // Create a cycle: n1 -> n2 -> n1
        let n1 = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![1.0, 0.0]))
            .unwrap();
        let n2 = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![0.0, 1.0]))
            .unwrap();
        graph
            .create_edge(
                n1,
                n2,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                n2,
                n1,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        // Walk from n1 -- should not loop back.
        let path = graph.semantic_walk(n1, &[0.0, 1.0], 10).unwrap();

        // Should be [n1, n2] and then stop because n1 is already visited.
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].0, n1);
        assert_eq!(path[1].0, n2);
    }

    #[test]
    fn test_create_node_with_id_roundtrip() {
        // astraeadb-issues.md #14. Mimics a Flight export+import:
        // original graph has ids 1, 2, 3 with edge 1->2 and 2->3.
        // Re-imported into a fresh graph via create_node_with_id, the
        // ids survive so the edges still resolve.
        let dst = Graph::new(Box::new(InMemoryStorage::new()));
        for id in [1u64, 2, 3] {
            dst.create_node_with_id(
                NodeId(id),
                vec!["Person".into()],
                serde_json::json!({ "imported": id }),
                None,
            )
            .unwrap();
        }
        // Edges reference the imported ids — they must resolve.
        dst.create_edge(
            NodeId(1),
            NodeId(2),
            "KNOWS".into(),
            serde_json::json!({}),
            1.0,
            None,
            None,
        )
        .unwrap();
        dst.create_edge(
            NodeId(2),
            NodeId(3),
            "KNOWS".into(),
            serde_json::json!({}),
            1.0,
            None,
            None,
        )
        .unwrap();

        // Subsequent auto-assignment must pick an id strictly greater
        // than the max we supplied (3).
        let next = dst
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        assert!(
            next.0 > 3,
            "next auto id {} must exceed supplied max 3",
            next.0
        );

        // Re-importing the same id must fail with DuplicateNode rather
        // than silently overwriting.
        let err = dst
            .create_node_with_id(NodeId(2), vec![], serde_json::json!({}), None)
            .unwrap_err();
        assert!(matches!(err, AstraeaError::DuplicateNode(NodeId(2))));
    }
}

/// Issue #26 end-to-end integration tests: graph-level restart with
/// a 768-dim embedding + large (~8 KB) properties that force the multi-page
/// overflow chain introduced in Phase 1 and Phase 2 of the fix.
///
/// These tests exercise `DiskStorageEngine` directly and require the
/// `tempfile` dev-dependency.
///
/// # HNSW repopulation on restart
///
/// `DiskStorageEngine::open()` replays the WAL by calling
/// `StorageEngine::put_node` directly — it never goes through
/// `Graph::create_node`, so the in-memory HNSW vector index is NOT
/// updated during replay.  After reconstruction via `Graph::with_start_ids`
/// and attaching a fresh index via `set_vector_index`, callers must call
/// `Graph::rebuild_vector_index()` to scan all stored nodes and re-insert
/// their embeddings into the HNSW.  This is O(n) over nodes and restores
/// full vector-search correctness after restart.
#[cfg(test)]
mod disk_restart_tests {
    use super::*;
    use astraea_core::types::DistanceMetric;
    use astraea_storage::DiskStorageEngine;
    use astraea_vector::HnswVectorIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Build a deterministic 768-dim embedding for use across test phases.
    fn make_embedding() -> Vec<f32> {
        (0..768u32).map(|i| i as f32 * 0.001 + 0.5).collect()
    }

    /// Issue #26 regression test — graph level.
    ///
    /// Verifies that a node with a 768-dim embedding AND ~8 KB of properties
    /// (which forces the multi-page overflow chain) round-trips through a
    /// `Graph` backed by `DiskStorageEngine` across a simulated restart:
    ///
    ///   Phase 1 — write via `Graph::create_node`.
    ///   Phase 2 — drop + reopen via `DiskStorageEngine::open` and
    ///             `Graph::with_start_ids` + `set_vector_index`.
    ///
    /// Storage assertions (GREEN): `get_node` returns the node with
    /// `embedding.is_some()` and bit-identical f32 values.
    ///
    /// Vector index assertion (documents the GAP — see module doc and
    /// `test_issue_26_hnsw_gap_after_restart`): HNSW search for the node's
    /// own embedding returns zero results after restart because the index is
    /// never repopulated.
    #[test]
    fn test_issue_26_graph_restart_storage_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();
        let embedding = make_embedding();
        // ~8 KB property body + 3,072-byte embedding exceeds a single 8 KB
        // page, so the multi-page overflow chain (Phase 2) is exercised.
        let large_prop = "A".repeat(8_000);

        let node_id;
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            let vi = Arc::new(HnswVectorIndex::new(768, DistanceMetric::Cosine));
            let graph = Graph::with_vector_index(Box::new(engine), vi);

            node_id = graph
                .create_node(
                    vec!["BigEmbeddedNode".into()],
                    serde_json::json!({ "content": large_prop }),
                    Some(embedding.clone()),
                )
                .expect("create_node with 768-dim embedding + 8 KB props must succeed");

            // Sanity: HNSW search finds the node before restart.
            let pre_results = graph.vector_index().unwrap().search(&embedding, 1).unwrap();
            assert_eq!(
                pre_results.len(),
                1,
                "HNSW must find the node before restart"
            );
            assert_eq!(
                pre_results[0].node_id, node_id,
                "HNSW result must be the inserted node"
            );

            graph.flush().unwrap();
        } // graph + engine dropped here

        // ── Restart ──────────────────────────────────────────────────────────
        let (engine2, max_node_id, max_edge_id) = DiskStorageEngine::open(data_dir)
            .expect("reopen must succeed after overflow-chain writes");

        assert_eq!(
            max_node_id, node_id.0,
            "WAL replay must recover the correct max node id"
        );

        // Attach a fresh (empty) HNSW index — no repopulation happens.
        let mut graph2 = Graph::with_start_ids(Box::new(engine2), max_node_id + 1, max_edge_id + 1);
        graph2.set_vector_index(Arc::new(HnswVectorIndex::new(768, DistanceMetric::Cosine)));

        // ── Storage assertions (GREEN) ────────────────────────────────────────
        let recovered = graph2
            .get_node(node_id)
            .expect("get_node must not error after restart")
            .expect("node must be present after WAL replay");

        assert!(
            recovered.embedding.is_some(),
            "embedding must survive restart via WAL replay (storage fix is correct)"
        );
        let got_emb = recovered.embedding.as_ref().unwrap();
        assert_eq!(
            got_emb.len(),
            768,
            "embedding dimension must be preserved after restart"
        );
        for (i, (orig, got)) in embedding.iter().zip(got_emb.iter()).enumerate() {
            assert_eq!(
                orig.to_bits(),
                got.to_bits(),
                "embedding[{i}] must be bit-identical after restart"
            );
        }
        assert_eq!(
            recovered.properties["content"].as_str().unwrap().len(),
            8_000,
            "large property body must survive restart"
        );
    }

    /// Issue #26 — HNSW repopulation after restart.
    ///
    /// Verifies that calling `Graph::rebuild_vector_index()` after reopening
    /// a `DiskStorageEngine` restores full vector-search correctness.
    ///
    /// Sequence:
    ///   1. Create a node with a 768-dim embedding via `Graph::create_node`.
    ///   2. Flush and drop the graph (simulated restart).
    ///   3. Reopen via `DiskStorageEngine::open` + `Graph::with_start_ids`.
    ///   4. Attach a fresh HNSW index via `set_vector_index`.
    ///   5. Call `rebuild_vector_index()` — O(n) scan-and-reinsert.
    ///   6. Assert HNSW search for the node's own embedding returns the node.
    #[test]
    fn test_issue_26_hnsw_repopulated_after_restart() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();
        let embedding = make_embedding();

        let node_id;
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            let vi = Arc::new(HnswVectorIndex::new(768, DistanceMetric::Cosine));
            let graph = Graph::with_vector_index(Box::new(engine), vi);

            node_id = graph
                .create_node(
                    vec!["EmbeddedNode".into()],
                    serde_json::json!({ "content": "x".repeat(8_000) }),
                    Some(embedding.clone()),
                )
                .expect("create_node must succeed");

            graph.flush().unwrap();
        } // graph + engine dropped here

        // ── Restart ──────────────────────────────────────────────────────────
        let (engine2, max_node_id, max_edge_id) =
            DiskStorageEngine::open(data_dir).expect("reopen must succeed");

        let mut graph2 = Graph::with_start_ids(Box::new(engine2), max_node_id + 1, max_edge_id + 1);
        graph2.set_vector_index(Arc::new(HnswVectorIndex::new(768, DistanceMetric::Cosine)));

        // Rebuild the HNSW from storage (Option B: scan-and-reinsert).
        let inserted = graph2
            .rebuild_vector_index()
            .expect("rebuild_vector_index must not error");
        assert_eq!(inserted, 1, "exactly one embedding should be reinserted");

        // Storage is correct: the node is present with its embedding.
        let recovered = graph2.get_node(node_id).unwrap().unwrap();
        assert!(
            recovered.embedding.is_some(),
            "embedding must survive restart via WAL replay"
        );

        // Vector search now returns the node after repopulation.
        let post_results = graph2
            .vector_index()
            .unwrap()
            .search(&embedding, 1)
            .unwrap();

        assert!(
            !post_results.is_empty(),
            "HNSW must find the node after restart once rebuild_vector_index is called"
        );
        assert_eq!(
            post_results[0].node_id, node_id,
            "top HNSW hit must be the recovered node"
        );
    }

    // ── Task A/B/C tests: persisted-index round-trip + delta-reconcile ─────────

    /// Helper: build a small deterministic embedding for a given integer seed.
    /// Uses dim=4 for speed in the reconcile tests.
    fn make_small_embedding(seed: f32) -> Vec<f32> {
        vec![seed, seed * 0.5, seed * 0.25, seed * 0.125]
    }

    /// Round-trip: create a node with embedding, `save_vector_index`, drop,
    /// reopen, `load_or_rebuild_vector_index` → must report `Loaded` (not
    /// Rebuilt) with zero deltas, and vector search must return the node.
    ///
    /// Proves we actually used the persisted file rather than rebuilding.
    #[test]
    fn test_hnsw_loaded_index_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();
        let hnsw_path = data_dir.join("astraea.hnsw");
        let embedding = make_embedding(); // 768-dim + 8 KB props exercises overflow chain
        let large_prop = "Z".repeat(8_000);
        let node_id;

        // ─── Phase 1: write, save snapshot, flush ─────────────────────────────
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            let vi = Arc::new(HnswVectorIndex::new(768, DistanceMetric::Cosine));
            let graph = Graph::with_vector_index(Box::new(engine), vi);

            node_id = graph
                .create_node(
                    vec!["Snapshotted".into()],
                    serde_json::json!({ "body": large_prop }),
                    Some(embedding.clone()),
                )
                .expect("create_node must succeed");

            graph
                .save_vector_index(&hnsw_path)
                .expect("save_vector_index must succeed");
            graph.flush().unwrap();
        }

        // ─── Phase 2: reopen + load (must NOT rebuild) ────────────────────────
        let (engine2, max_node_id, max_edge_id) =
            DiskStorageEngine::open(data_dir).expect("reopen must succeed");
        let mut graph2 = Graph::with_start_ids(Box::new(engine2), max_node_id + 1, max_edge_id + 1);

        let init = graph2
            .load_or_rebuild_vector_index(&hnsw_path, 768, DistanceMetric::Cosine)
            .expect("load_or_rebuild must not error");

        // Must have loaded from the file, not rebuilt.
        assert!(
            matches!(
                init,
                VectorIndexInit::Loaded {
                    inserted: 0,
                    removed: 0
                }
            ),
            "expected Loaded{{inserted:0, removed:0}}, got {:?}",
            init
        );

        // Vector search must return the node.
        let results = graph2
            .vector_index()
            .unwrap()
            .search(&embedding, 1)
            .unwrap();
        assert!(
            !results.is_empty(),
            "search must return at least one result"
        );
        assert_eq!(
            results[0].node_id, node_id,
            "top result must be the snapshotted node"
        );
    }

    /// Crash-simulation delta-reconcile (both insert and delete directions).
    ///
    /// All writes happen in **one** `DiskStorageEngine` session so that WAL
    /// records are appended correctly (avoids the cursor-at-0 issue on
    /// re-open).  The snapshot is saved **mid-session** (representing the last
    /// checkpoint), then B1/B2 are added and A1 is deleted **without** a
    /// second snapshot save (crash simulation).  On the next open, WAL replay
    /// gives storage {A2, A3, B1, B2}; the stale snapshot has {A1, A2, A3}.
    ///
    /// Expected: `Loaded { inserted: 2, removed: 1 }`.
    /// B1/B2 searchable; A1 absent from the index.
    #[test]
    fn test_hnsw_delta_reconcile_crash_sim() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();
        let hnsw_path = data_dir.join("astraea.hnsw");
        const DIM: usize = 4;
        let metric = DistanceMetric::Euclidean;

        // ─── Single write session ──────────────────────────────────────────────
        // Create A1/A2/A3, checkpoint the snapshot, then add B1/B2 and delete
        // A1 WITHOUT a second snapshot save.  This models:
        //   t=0  snapshot saved (index = {A1, A2, A3})
        //   t=1  WAL-durable: create B1, create B2, delete A1
        //   t=2  crash before next checkpoint
        let id_a1;
        let id_b1;
        let id_b2;
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            let vi = Arc::new(HnswVectorIndex::new(DIM, metric));
            let graph = Graph::with_vector_index(Box::new(engine), vi);

            id_a1 = graph
                .create_node(
                    vec![],
                    serde_json::json!({}),
                    Some(make_small_embedding(1.0)),
                )
                .unwrap();
            let _id_a2 = graph
                .create_node(
                    vec![],
                    serde_json::json!({}),
                    Some(make_small_embedding(2.0)),
                )
                .unwrap();
            let _id_a3 = graph
                .create_node(
                    vec![],
                    serde_json::json!({}),
                    Some(make_small_embedding(3.0)),
                )
                .unwrap();

            // Checkpoint: save snapshot with {A1, A2, A3}.
            graph
                .save_vector_index(&hnsw_path)
                .expect("save_vector_index must succeed");

            // Post-checkpoint WAL-durable writes — snapshot NOT updated.
            id_b1 = graph
                .create_node(
                    vec![],
                    serde_json::json!({}),
                    Some(make_small_embedding(10.0)),
                )
                .unwrap();
            id_b2 = graph
                .create_node(
                    vec![],
                    serde_json::json!({}),
                    Some(make_small_embedding(20.0)),
                )
                .unwrap();
            graph.delete_node(id_a1).unwrap();

            // Flush (WAL fsynced) — no second save_vector_index (crash sim).
            graph.flush().unwrap();
        }

        // ─── Restart: WAL replay → storage {A2, A3, B1, B2}; load stale snap ─
        {
            let (engine, max_node_id, max_edge_id) =
                DiskStorageEngine::open(data_dir).expect("reopen must succeed");
            let mut graph2 =
                Graph::with_start_ids(Box::new(engine), max_node_id + 1, max_edge_id + 1);

            let init = graph2
                .load_or_rebuild_vector_index(&hnsw_path, DIM, metric)
                .expect("load_or_rebuild must succeed");

            match init {
                VectorIndexInit::Loaded { inserted, removed } => {
                    assert_eq!(inserted, 2, "reconcile must insert B1 and B2");
                    assert_eq!(removed, 1, "reconcile must remove A1");
                }
                VectorIndexInit::Rebuilt { .. } => {
                    panic!("expected Loaded (snapshot exists), got Rebuilt")
                }
            }

            let vi = graph2.vector_index().unwrap();

            // B1 and B2 must be searchable after reconcile.
            let r1 = vi.search(&make_small_embedding(10.0), 1).unwrap();
            assert!(
                !r1.is_empty() && r1[0].node_id == id_b1,
                "B1 must be findable after reconcile"
            );
            let r2 = vi.search(&make_small_embedding(20.0), 1).unwrap();
            assert!(
                !r2.is_empty() && r2[0].node_id == id_b2,
                "B2 must be findable after reconcile"
            );

            // A1 must have been stripped from the loaded index.
            let index_ids: std::collections::HashSet<NodeId> = vi.node_ids().into_iter().collect();
            assert!(
                !index_ids.contains(&id_a1),
                "A1 must be absent from the index after reconcile"
            );
        }
    }

    /// N1 regression: a persisted index loaded with a *different* metric than
    /// it was built with must trigger `Rebuilt`, not silently serve stale
    /// ranked results.
    ///
    /// Sequence:
    ///   1. Build and save a snapshot with `DistanceMetric::Cosine`.
    ///   2. Call `load_or_rebuild_vector_index` requesting `DistanceMetric::Euclidean`.
    ///   3. Assert the result is `Rebuilt` (not `Loaded`).
    ///   4. Assert vector search still returns the correct node (rebuild was full).
    #[test]
    fn test_hnsw_metric_mismatch_triggers_rebuild() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();
        let hnsw_path = data_dir.join("astraea.hnsw");
        const DIM: usize = 4;
        let embedding = make_small_embedding(1.0);
        let node_id;

        // ─── Phase 1: write with Cosine, save snapshot ────────────────────────
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            let vi = Arc::new(HnswVectorIndex::new(DIM, DistanceMetric::Cosine));
            let graph = Graph::with_vector_index(Box::new(engine), vi);

            node_id = graph
                .create_node(vec![], serde_json::json!({}), Some(embedding.clone()))
                .expect("create_node must succeed");

            graph
                .save_vector_index(&hnsw_path)
                .expect("save_vector_index must succeed");
            graph.flush().unwrap();
        }

        // ─── Phase 2: reopen requesting Euclidean (mismatch) ─────────────────
        let (engine2, max_node_id, max_edge_id) =
            DiskStorageEngine::open(data_dir).expect("reopen must succeed");
        let mut graph2 = Graph::with_start_ids(Box::new(engine2), max_node_id + 1, max_edge_id + 1);

        let init = graph2
            .load_or_rebuild_vector_index(&hnsw_path, DIM, DistanceMetric::Euclidean)
            .expect("load_or_rebuild must not error on metric mismatch");

        // Metric mismatch must fall through to the rebuild path.
        assert!(
            matches!(init, VectorIndexInit::Rebuilt { count: 1 }),
            "expected Rebuilt{{count:1}} on metric mismatch, got {:?}",
            init
        );

        // The rebuilt index (now Euclidean) must still find the node.
        let results = graph2
            .vector_index()
            .unwrap()
            .search(&embedding, 1)
            .unwrap();
        assert!(
            !results.is_empty(),
            "search must return a result after metric-mismatch rebuild"
        );
        assert_eq!(results[0].node_id, node_id, "top result must be the node");
    }

    /// Fallback (a): no `.hnsw` file present → must report `Rebuilt`, leave
    /// the file on disk for the next restart, and return correct search results.
    #[test]
    fn test_hnsw_missing_snapshot_rebuilds() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();
        let hnsw_path = data_dir.join("astraea.hnsw");
        let embedding = make_embedding();
        let node_id;

        // Create a node but do NOT save the index.
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            let graph = Graph::new(Box::new(engine));
            node_id = graph
                .create_node(
                    vec![],
                    serde_json::json!({ "content": "x".repeat(8_000) }),
                    Some(embedding.clone()),
                )
                .unwrap();
            graph.flush().unwrap();
        }

        // No .hnsw file should exist at this point.
        assert!(
            !hnsw_path.exists(),
            "hnsw file must not exist before load_or_rebuild"
        );

        let (engine2, max_node_id, max_edge_id) =
            DiskStorageEngine::open(data_dir).expect("reopen must succeed");
        let mut graph2 = Graph::with_start_ids(Box::new(engine2), max_node_id + 1, max_edge_id + 1);

        let init = graph2
            .load_or_rebuild_vector_index(&hnsw_path, 768, DistanceMetric::Cosine)
            .expect("load_or_rebuild must not fail even with missing file");

        assert!(
            matches!(init, VectorIndexInit::Rebuilt { count: 1 }),
            "expected Rebuilt{{count:1}}, got {:?}",
            init
        );

        // The rebuild must have saved the file so the next boot is fast.
        assert!(
            hnsw_path.exists(),
            "save_vector_index after rebuild must create the .hnsw file"
        );

        // Vector search must work.
        let results = graph2
            .vector_index()
            .unwrap()
            .search(&embedding, 1)
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].node_id, node_id);
    }

    /// Fallback (b): a corrupt `.hnsw` file → must report `Rebuilt` (not an
    /// error), replace the corrupt file with a valid one, and return correct
    /// search results.
    #[test]
    fn test_hnsw_corrupt_snapshot_rebuilds() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();
        let hnsw_path = data_dir.join("astraea.hnsw");
        let embedding = make_embedding();
        let node_id;

        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            let graph = Graph::new(Box::new(engine));
            node_id = graph
                .create_node(
                    vec![],
                    serde_json::json!({ "content": "y".repeat(8_000) }),
                    Some(embedding.clone()),
                )
                .unwrap();
            graph.flush().unwrap();
        }

        // Plant a corrupt file at the expected snapshot path.
        std::fs::write(&hnsw_path, b"not a valid HNSW file -- garbage bytes").unwrap();

        let (engine2, max_node_id, max_edge_id) =
            DiskStorageEngine::open(data_dir).expect("reopen must succeed");
        let mut graph2 = Graph::with_start_ids(Box::new(engine2), max_node_id + 1, max_edge_id + 1);

        let init = graph2
            .load_or_rebuild_vector_index(&hnsw_path, 768, DistanceMetric::Cosine)
            .expect("load_or_rebuild must not propagate a corrupt-file error");

        assert!(
            matches!(init, VectorIndexInit::Rebuilt { count: 1 }),
            "expected Rebuilt{{count:1}} for corrupt snapshot, got {:?}",
            init
        );

        // File must have been replaced with a valid snapshot.
        assert!(
            hnsw_path.exists(),
            "corrupt file must be replaced by a valid rebuild snapshot"
        );

        let results = graph2
            .vector_index()
            .unwrap()
            .search(&embedding, 1)
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].node_id, node_id);
    }
}

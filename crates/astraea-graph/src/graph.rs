use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::{GraphOps, StorageEngine, VectorIndex};
use astraea_core::types::*;

use crate::traversal;

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
        if let (Some(vi), Some(emb)) = (&self.vector_index, &node.embedding) {
            // Don't fail node creation if vector indexing fails; just log the error.
            if let Err(e) = vi.insert(node.id, emb) {
                tracing::warn!("failed to index embedding for node {}: {}", node.id, e);
            }
        }

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
        // Remove from vector index if present (ignore errors; node may not have had an embedding).
        if let Some(ref vi) = self.vector_index {
            let _ = vi.remove(id);
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

    fn shortest_path_weighted(
        &self,
        from: NodeId,
        to: NodeId,
    ) -> Result<Option<(GraphPath, f64)>> {
        traversal::shortest_path_dijkstra(self.storage.as_ref(), from, to)
    }

    fn find_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        self.storage.find_nodes_by_label(label)
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
        if let Some(node) = self.get_node(current)? {
            if let Some(ref emb) = node.embedding {
                if let Ok(d) = compute_distance(metric, concept_embedding, emb) {
                    path.push((current, d));
                }
            }
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

                if let Some(ref emb) = node.embedding {
                    if let Ok(d) = compute_distance(metric, concept_embedding, emb) {
                        if best.is_none() || d < best.unwrap().1 {
                            best = Some((neighbor_id, d));
                        }
                    }
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

        let result = graph.create_edge(a, NodeId(999), "KNOWS".into(), serde_json::json!({}), 1.0, None, None);
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
            .create_edge(c, a, "FOLLOWS".into(), serde_json::json!({}), 1.0, None, None)
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
            .create_edge(a, c, "FOLLOWS".into(), serde_json::json!({}), 1.0, None, None)
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
            .create_edge(n1, n3, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(n1, n4, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(n3, n5, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(n2, n4, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(n4, n5, "LINK".into(), serde_json::json!({}), 1.0, None, None)
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
        let path = graph
            .semantic_walk(n1, &[0.0, 0.0, 1.0], 5)
            .unwrap();

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
        let path = graph
            .semantic_walk(n1, &[0.0, 0.0, 1.0], 5)
            .unwrap();

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
        let path = graph
            .semantic_walk(n5, &[1.0, 0.0, 0.0], 5)
            .unwrap();

        assert_eq!(path.len(), 1);
        assert_eq!(path[0].0, n5);
    }

    #[test]
    fn test_semantic_neighbors_no_embedding() {
        let storage = InMemoryStorage::new();
        let vi = Arc::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        let graph = Graph::with_vector_index(Box::new(storage), vi);

        let n1 = graph
            .create_node(
                vec![],
                serde_json::json!({}),
                Some(vec![1.0, 0.0, 0.0]),
            )
            .unwrap();
        // n2 has no embedding.
        let n2 = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        graph
            .create_edge(n1, n2, "LINK".into(), serde_json::json!({}), 1.0, None, None)
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
            .create_node(
                vec![],
                serde_json::json!({}),
                Some(vec![1.0, 0.0, 0.0]),
            )
            .unwrap();
        let n2 = graph
            .create_node(
                vec![],
                serde_json::json!({}),
                Some(vec![0.0, 1.0, 0.0]),
            )
            .unwrap();
        graph
            .create_edge(n1, n2, "LINK".into(), serde_json::json!({}), 1.0, None, None)
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
            .create_node(
                vec![],
                serde_json::json!({}),
                Some(vec![1.0, 0.0]),
            )
            .unwrap();
        let n2 = graph
            .create_node(
                vec![],
                serde_json::json!({}),
                Some(vec![0.0, 1.0]),
            )
            .unwrap();
        graph
            .create_edge(n1, n2, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(n2, n1, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();

        // Walk from n1 -- should not loop back.
        let path = graph
            .semantic_walk(n1, &[0.0, 1.0], 10)
            .unwrap();

        // Should be [n1, n2] and then stop because n1 is already visited.
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].0, n1);
        assert_eq!(path[1].0, n2);
    }
}

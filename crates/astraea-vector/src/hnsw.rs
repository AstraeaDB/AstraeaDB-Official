//! HNSW (Hierarchical Navigable Small World) index implementation.
//!
//! Based on: Malkov & Yashunin, "Efficient and robust approximate nearest
//! neighbor search using Hierarchical Navigable Small World graphs", 2016.
//!
//! The index builds a multi-layer graph where each layer is a navigable
//! small-world network. Higher layers contain fewer nodes (exponential decay)
//! and serve as "express lanes" for fast coarse-grained search, while layer 0
//! contains all nodes and provides fine-grained nearest-neighbor results.

use std::collections::{BinaryHeap, HashMap, HashSet};

use ordered_float::OrderedFloat;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::{DistanceMetric, NodeId};

use crate::distance::compute_distance;

/// A candidate in the priority queue: (distance, node_id).
///
/// The default `BinaryHeap` is a max-heap, so we negate the distance when we
/// want a min-heap. Instead, we use two wrapper types below.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    distance: OrderedFloat<f32>,
    node_id: NodeId,
}

/// Min-heap ordering: smallest distance first.
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse so BinaryHeap (max-heap) pops the *smallest* distance first.
        other
            .distance
            .cmp(&self.distance)
            .then_with(|| self.node_id.cmp(&other.node_id))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Max-heap ordering: largest distance first (used for the result set / "worst" tracking).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RevCandidate {
    distance: OrderedFloat<f32>,
    node_id: NodeId,
}

impl Ord for RevCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .cmp(&other.distance)
            .then_with(|| self.node_id.cmp(&other.node_id))
    }
}

impl PartialOrd for RevCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The core HNSW index data structure.
///
/// This stores vectors and the multi-layer navigable small-world graph.
/// Thread safety is handled externally (by `HnswVectorIndex` via `RwLock`).
#[derive(Debug, Serialize, Deserialize)]
pub struct HnswIndex {
    /// Dimensionality of the vectors.
    dimension: usize,
    /// Distance metric to use.
    metric: DistanceMetric,

    // --- HNSW parameters ---
    /// Maximum connections per node per layer (except layer 0).
    m: usize,
    /// Maximum connections at layer 0 (typically 2 * m).
    m_max0: usize,
    /// Beam width during construction.
    ef_construction: usize,
    /// Level generation factor: 1 / ln(m).
    ml: f64,

    // --- Data storage ---
    /// Mapping from node ID to its embedding vector.
    vectors: HashMap<NodeId, Vec<f32>>,
    /// Multi-layer adjacency lists: layers[layer][node] = vec of neighbor node IDs.
    layers: Vec<HashMap<NodeId, Vec<NodeId>>>,
    /// The current entry point (the node at the highest layer).
    entry_point: Option<NodeId>,
    /// The current maximum layer index.
    max_level: usize,
    /// For each node, the highest layer it appears in.
    node_levels: HashMap<NodeId, usize>,

    /// Deterministic RNG for `random_level`. When `None`, falls back to
    /// `thread_rng` — the default for back-compat. astraeadb-issues.md #18.
    /// Not persisted: seed state is for build reproducibility, not for
    /// resuming an index across serialize / deserialize.
    #[serde(skip, default)]
    rng: Option<StdRng>,
}

impl HnswIndex {
    /// Create a new, empty HNSW index.
    ///
    /// # Parameters
    /// - `dimension`: the fixed dimensionality of all vectors
    /// - `metric`: the distance metric to use
    /// - `m`: max connections per node per layer (default recommendation: 16)
    /// - `ef_construction`: beam width during insertion (default recommendation: 200)
    ///
    /// Uses `thread_rng` for level sampling. For reproducible index builds
    /// (tests, benchmarks), use [`HnswIndex::with_seed`] instead.
    pub fn new(dimension: usize, metric: DistanceMetric, m: usize, ef_construction: usize) -> Self {
        Self::with_optional_seed(dimension, metric, m, ef_construction, None)
    }

    /// Create a new, empty HNSW index with a fixed RNG seed for reproducible
    /// level sampling. Same parameters as [`HnswIndex::new`] plus a `seed`
    /// value used to initialize a [`StdRng`]. Two indexes built with the
    /// same seed and the same insert sequence will produce byte-identical
    /// graph structure. astraeadb-issues.md #18.
    pub fn with_seed(
        dimension: usize,
        metric: DistanceMetric,
        m: usize,
        ef_construction: usize,
        seed: u64,
    ) -> Self {
        Self::with_optional_seed(dimension, metric, m, ef_construction, Some(seed))
    }

    fn with_optional_seed(
        dimension: usize,
        metric: DistanceMetric,
        m: usize,
        ef_construction: usize,
        seed: Option<u64>,
    ) -> Self {
        let m_max0 = m * 2;
        let ml = 1.0 / (m as f64).ln();
        let rng = seed.map(StdRng::seed_from_u64);

        Self {
            dimension,
            metric,
            m,
            m_max0,
            ef_construction,
            ml,
            vectors: HashMap::new(),
            layers: vec![HashMap::new()], // start with layer 0
            entry_point: None,
            max_level: 0,
            node_levels: HashMap::new(),
            rng,
        }
    }

    /// Return the number of vectors stored in the index.
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    /// Return whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Return all node IDs currently stored in the index.
    ///
    /// Iterates the `vectors` map keys; allocation is O(n) in the number of
    /// stored vectors. Used by the Graph layer for snapshot reconciliation
    /// (§11.3 of the issue-26 design).
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.vectors.keys().copied().collect()
    }

    /// Return whether the index contains a vector for the given node ID.
    pub fn contains(&self, id: NodeId) -> bool {
        self.vectors.contains_key(&id)
    }

    /// Return the configured dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Return the configured metric.
    pub fn metric(&self) -> DistanceMetric {
        self.metric
    }

    /// Return the `m` parameter (max connections per non-zero layer).
    pub fn m(&self) -> usize {
        self.m
    }

    /// Return the `m_max0` parameter (max connections at layer 0).
    pub fn m_max0(&self) -> usize {
        self.m_max0
    }

    /// Return the `ef_construction` parameter.
    pub fn ef_construction(&self) -> usize {
        self.ef_construction
    }

    /// Return the number of layers currently in the graph.
    pub fn num_layers(&self) -> usize {
        self.layers.len()
    }

    /// Insert a vector for the given node.
    ///
    /// Implements Algorithm 1 from Malkov & Yashunin (2016):
    /// 1. Generate a random level for this node.
    /// 2. Starting from the entry point, greedily traverse layers above the new node's level.
    /// 3. For each layer from min(node_level, max_level) down to 0, perform beam search
    ///    to find ef_construction nearest neighbors, then connect to the top M (or M_max0).
    pub fn insert(&mut self, node_id: NodeId, vector: &[f32]) -> Result<()> {
        if vector.len() != self.dimension {
            return Err(AstraeaError::DimensionMismatch {
                expected: self.dimension,
                got: vector.len(),
            });
        }

        // Store the vector.
        self.vectors.insert(node_id, vector.to_vec());

        // Generate random level.
        let level = self.random_level();

        // Ensure we have enough layers.
        while self.layers.len() <= level {
            self.layers.push(HashMap::new());
        }

        // Register the node in every layer from 0..=level.
        for l in 0..=level {
            self.layers[l].entry(node_id).or_default();
        }
        self.node_levels.insert(node_id, level);

        // If this is the first node, set it as entry point and return.
        let ep = match self.entry_point {
            Some(ep) => ep,
            None => {
                self.entry_point = Some(node_id);
                self.max_level = level;
                return Ok(());
            }
        };

        let mut current_ep = ep;

        // Phase 1: Greedily descend through layers above the new node's level.
        // We traverse from max_level down to level+1, each time finding the single
        // closest node (greedy search with ef=1).
        if self.max_level > level {
            for l in (level + 1..=self.max_level).rev() {
                let nearest = self.search_layer(vector, &[current_ep], 1, l)?;
                if let Some(closest) = nearest.into_iter().next() {
                    current_ep = closest.1;
                }
            }
        }

        // Phase 2: For each layer from min(level, max_level) down to 0, do a beam
        // search to find ef_construction nearest neighbors, then connect.
        let top_layer = level.min(self.max_level);
        for l in (0..=top_layer).rev() {
            let max_conn = if l == 0 { self.m_max0 } else { self.m };
            let ef = self.ef_construction;

            let candidates = self.search_layer(vector, &[current_ep], ef, l)?;

            // Update the current entry point for the next (lower) layer to the
            // closest node found here (candidates are sorted ascending). This is
            // independent of which neighbours the heuristic ends up keeping.
            if let Some(&(_, nearest)) = candidates.first() {
                current_ep = nearest;
            }

            // Select neighbours with the SELECT-NEIGHBORS-HEURISTIC (Algorithm 4)
            // rather than a naive top-`max_conn`. Plain top-M yields clustered,
            // poorly-navigable neighbourhoods whose recall collapses past a few
            // thousand vectors (astraeadb-issues.md #25); the heuristic keeps a
            // candidate only if it is closer to the new node than to any
            // already-selected neighbour, producing diverse, well-connected links.
            let neighbors = self.select_neighbors_heuristic(vector, &candidates, max_conn)?;

            // Add bidirectional connections.
            for &neighbor in &neighbors {
                // node -> neighbor
                if let Some(adj) = self.layers[l].get_mut(&node_id)
                    && !adj.contains(&neighbor)
                {
                    adj.push(neighbor);
                }
                // neighbor -> node
                if let Some(adj) = self.layers[l].get_mut(&neighbor) {
                    if !adj.contains(&node_id) {
                        adj.push(node_id);
                    }
                    // Prune if over capacity.
                    if adj.len() > max_conn {
                        self.shrink_connections(neighbor, l, max_conn)?;
                    }
                }
            }
        }

        // If the new node's level exceeds max_level, update the entry point.
        if level > self.max_level {
            self.max_level = level;
            self.entry_point = Some(node_id);
        }

        Ok(())
    }

    /// Search the index for the `k` nearest neighbors of `query`.
    ///
    /// Implements Algorithm 5 from Malkov & Yashunin (2016):
    /// 1. Greedily descend from the entry point through layers above 0.
    /// 2. At layer 0, perform beam search with `ef_search` width.
    /// 3. Return the top `k` results sorted by distance (ascending).
    pub fn search(&self, query: &[f32], k: usize, ef_search: usize) -> Result<Vec<(NodeId, f32)>> {
        if query.len() != self.dimension {
            return Err(AstraeaError::DimensionMismatch {
                expected: self.dimension,
                got: query.len(),
            });
        }

        let ep = match self.entry_point {
            Some(ep) => ep,
            None => return Ok(Vec::new()), // empty index
        };

        let mut current_ep = ep;

        // Phase 1: greedy descent through upper layers.
        if self.max_level > 0 {
            for l in (1..=self.max_level).rev() {
                let nearest = self.search_layer(query, &[current_ep], 1, l)?;
                if let Some(closest) = nearest.into_iter().next() {
                    current_ep = closest.1;
                }
            }
        }

        // Phase 2: beam search at layer 0.
        let ef = ef_search.max(k);
        let candidates = self.search_layer(query, &[current_ep], ef, 0)?;

        // Return top k.
        let results: Vec<(NodeId, f32)> = candidates
            .into_iter()
            .take(k)
            .map(|(d, id)| (id, d))
            .collect();

        Ok(results)
    }

    /// Remove a node from the index and repair connections.
    ///
    /// For each layer the node participated in, we remove it from all neighbor
    /// lists and attempt to reconnect orphaned neighbors to each other.
    pub fn remove(&mut self, node_id: NodeId) -> Result<bool> {
        if self.vectors.remove(&node_id).is_none() {
            return Ok(false);
        }

        let level = match self.node_levels.remove(&node_id) {
            Some(l) => l,
            None => return Ok(true),
        };

        for l in 0..=level.min(self.layers.len().saturating_sub(1)) {
            if let Some(neighbors) = self.layers[l].remove(&node_id) {
                // Remove node_id from each neighbor's adjacency list.
                for &neighbor in &neighbors {
                    if let Some(adj) = self.layers[l].get_mut(&neighbor) {
                        adj.retain(|&n| n != node_id);
                    }
                }

                // Repair: try to connect orphaned pairs that lost connectivity.
                // For each pair of former neighbors, if they are not already connected,
                // add a direct link (if capacity allows).
                let max_conn = if l == 0 { self.m_max0 } else { self.m };
                for i in 0..neighbors.len() {
                    for j in (i + 1)..neighbors.len() {
                        let a = neighbors[i];
                        let b = neighbors[j];
                        // Only repair if both nodes still exist in this layer.
                        let a_exists = self.layers[l].contains_key(&a);
                        let b_exists = self.layers[l].contains_key(&b);
                        if !a_exists || !b_exists {
                            continue;
                        }
                        let a_has_b = self.layers[l]
                            .get(&a)
                            .map(|adj| adj.contains(&b))
                            .unwrap_or(false);
                        if !a_has_b {
                            if let Some(adj) = self.layers[l].get_mut(&a)
                                && adj.len() < max_conn
                            {
                                adj.push(b);
                            }
                            if let Some(adj) = self.layers[l].get_mut(&b)
                                && adj.len() < max_conn
                            {
                                adj.push(a);
                            }
                        }
                    }
                }
            }
        }

        // If we removed the entry point, pick a new one.
        if self.entry_point == Some(node_id) {
            self.entry_point = None;
            self.max_level = 0;

            // Find the node with the highest level to be the new entry point.
            for (&nid, &nlevel) in &self.node_levels {
                if nlevel >= self.max_level {
                    self.max_level = nlevel;
                    self.entry_point = Some(nid);
                }
            }
        }

        Ok(true)
    }

    /// Beam search at a single layer.
    ///
    /// Implements Algorithm 2 from Malkov & Yashunin (2016).
    ///
    /// Returns up to `ef` nearest neighbors sorted by ascending distance.
    fn search_layer(
        &self,
        query: &[f32],
        entry_points: &[NodeId],
        ef: usize,
        layer: usize,
    ) -> Result<Vec<(f32, NodeId)>> {
        let layer_adj = match self.layers.get(layer) {
            Some(l) => l,
            None => return Ok(Vec::new()),
        };

        let mut visited = HashSet::new();

        // candidates: min-heap of nodes to explore (closest first).
        let mut candidates = BinaryHeap::<Candidate>::new();
        // results: max-heap of the ef-best results so far (farthest first for easy pruning).
        let mut results = BinaryHeap::<RevCandidate>::new();

        for &ep in entry_points {
            if !self.vectors.contains_key(&ep) {
                continue;
            }
            let d = self.distance(query, ep)?;
            visited.insert(ep);
            candidates.push(Candidate {
                distance: OrderedFloat(d),
                node_id: ep,
            });
            results.push(RevCandidate {
                distance: OrderedFloat(d),
                node_id: ep,
            });
        }

        while let Some(Candidate {
            distance: c_dist,
            node_id: c_id,
        }) = candidates.pop()
        {
            // If the closest candidate is farther than the farthest result, stop.
            let farthest = results
                .peek()
                .map(|r| r.distance)
                .unwrap_or(OrderedFloat(f32::MAX));
            if c_dist > farthest {
                break;
            }

            // Explore neighbors of the current candidate in this layer.
            if let Some(neighbors) = layer_adj.get(&c_id) {
                for &neighbor in neighbors {
                    if visited.contains(&neighbor) {
                        continue;
                    }
                    visited.insert(neighbor);

                    let d = self.distance(query, neighbor)?;
                    let farthest = results
                        .peek()
                        .map(|r| r.distance)
                        .unwrap_or(OrderedFloat(f32::MAX));

                    if d < farthest.into_inner() || results.len() < ef {
                        candidates.push(Candidate {
                            distance: OrderedFloat(d),
                            node_id: neighbor,
                        });
                        results.push(RevCandidate {
                            distance: OrderedFloat(d),
                            node_id: neighbor,
                        });
                        if results.len() > ef {
                            results.pop(); // remove the farthest
                        }
                    }
                }
            }
        }

        // Convert the max-heap into a sorted vec (ascending distance).
        let mut result_vec: Vec<(f32, NodeId)> = results
            .into_iter()
            .map(|rc| (rc.distance.into_inner(), rc.node_id))
            .collect();
        result_vec.sort_by_key(|a| OrderedFloat(a.0));

        Ok(result_vec)
    }

    /// SELECT-NEIGHBORS-HEURISTIC — Algorithm 4 of Malkov & Yashunin (2016),
    /// basic variant (no `extendCandidates` / `keepPrunedConnections`).
    ///
    /// Given `base_vec` (the node we are connecting) and `candidates` already
    /// sorted by ascending distance to `base_vec` as `(distance, node_id)`,
    /// greedily keep a candidate only if it is closer to `base_vec` than to
    /// every neighbour already selected. This favours edges that span different
    /// directions around the node rather than a tight cluster, which is what
    /// gives HNSW its navigability (and fixes the recall collapse in #25).
    ///
    /// Returns at most `m` node IDs. May return fewer than `m` — that is the
    /// intended behaviour of the heuristic and is required for good graph
    /// quality; callers must not pad the result back up to `m`.
    fn select_neighbors_heuristic(
        &self,
        base_vec: &[f32],
        candidates: &[(f32, NodeId)],
        m: usize,
    ) -> Result<Vec<NodeId>> {
        let mut selected: Vec<NodeId> = Vec::with_capacity(m);

        for &(_, cand_id) in candidates {
            if selected.len() >= m {
                break;
            }
            let cand_vec = match self.vectors.get(&cand_id) {
                Some(v) => v,
                None => continue,
            };
            // Distance from this candidate to the node being connected. Computed
            // from `base_vec` directly so the heuristic does not depend on the
            // caller's candidate distances being in the same metric.
            let dist_to_base = compute_distance(self.metric, base_vec, cand_vec)?;

            // Keep `cand` only if it is no farther from `base_vec` than it is
            // from any already-selected neighbour. If some selected neighbour is
            // strictly closer to `cand` than `base_vec` is, `cand` is redundant
            // (that neighbour already covers this direction) — discard it.
            let mut keep = true;
            for &sel_id in &selected {
                if let Some(sel_vec) = self.vectors.get(&sel_id) {
                    let d = compute_distance(self.metric, cand_vec, sel_vec)?;
                    if d < dist_to_base {
                        keep = false;
                        break;
                    }
                }
            }
            if keep {
                selected.push(cand_id);
            }
        }

        Ok(selected)
    }

    /// Shrink a node's connection list to at most `max_conn`.
    ///
    /// Re-selects which links to keep with [`Self::select_neighbors_heuristic`]
    /// rather than naive closest-`max_conn` truncation, so that a node whose
    /// list overflows after a back-link keeps a diverse, navigable neighbourhood
    /// (astraeadb-issues.md #25).
    fn shrink_connections(&mut self, node_id: NodeId, layer: usize, max_conn: usize) -> Result<()> {
        let neighbors = match self.layers[layer].get(&node_id) {
            Some(n) => n.clone(),
            None => return Ok(()),
        };

        if neighbors.len() <= max_conn {
            return Ok(());
        }

        let node_vec = match self.vectors.get(&node_id) {
            Some(v) => v.clone(),
            None => return Ok(()),
        };

        // Score each neighbor by distance, sorted ascending for the heuristic.
        let mut scored: Vec<(f32, NodeId)> = Vec::with_capacity(neighbors.len());
        for &n in &neighbors {
            if let Some(nv) = self.vectors.get(&n) {
                let d = compute_distance(self.metric, &node_vec, nv)?;
                scored.push((d, n));
            }
        }
        scored.sort_by_key(|a| OrderedFloat(a.0));

        let new_neighbors = self.select_neighbors_heuristic(&node_vec, &scored, max_conn)?;
        if let Some(adj) = self.layers[layer].get_mut(&node_id) {
            *adj = new_neighbors;
        }

        Ok(())
    }

    /// Compute distance between a query vector and a stored node's vector.
    fn distance(&self, query: &[f32], node_id: NodeId) -> Result<f32> {
        let v = self
            .vectors
            .get(&node_id)
            .ok_or(AstraeaError::NoEmbedding(node_id))?;
        compute_distance(self.metric, query, v)
    }

    /// Generate a random level using an exponential distribution.
    ///
    /// level = floor(-ln(uniform(0,1)) * ml)
    ///
    /// Uses the seeded [`StdRng`] if the index was constructed with
    /// [`Self::with_seed`]; otherwise falls back to `thread_rng`.
    fn random_level(&mut self) -> usize {
        let r: f64 = match &mut self.rng {
            Some(rng) => rng.r#gen::<f64>(),
            None => rand::thread_rng().r#gen::<f64>(),
        };
        // Avoid ln(0) which is -infinity.
        let r = r.max(1e-10);
        (-r.ln() * self.ml).floor() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_index(dim: usize) -> HnswIndex {
        HnswIndex::new(dim, DistanceMetric::Euclidean, 16, 200)
    }

    #[test]
    fn test_insert_single_vector() {
        let mut idx = make_index(3);
        idx.insert(NodeId(1), &[1.0, 2.0, 3.0]).unwrap();
        assert_eq!(idx.len(), 1);
        assert!(!idx.is_empty());
    }

    #[test]
    fn test_search_empty_index() {
        let idx = make_index(3);
        let results = idx.search(&[1.0, 2.0, 3.0], 5, 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_single_vector() {
        let mut idx = make_index(3);
        idx.insert(NodeId(1), &[1.0, 2.0, 3.0]).unwrap();
        let results = idx.search(&[1.0, 2.0, 3.0], 5, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, NodeId(1));
        assert!(
            results[0].1 < 1e-6,
            "distance to identical vector should be ~0"
        );
    }

    #[test]
    fn test_search_k_greater_than_index_size() {
        let mut idx = make_index(2);
        idx.insert(NodeId(1), &[0.0, 0.0]).unwrap();
        idx.insert(NodeId(2), &[1.0, 0.0]).unwrap();
        idx.insert(NodeId(3), &[0.0, 1.0]).unwrap();

        let results = idx.search(&[0.0, 0.0], 10, 50).unwrap();
        assert_eq!(results.len(), 3, "should return all 3 vectors when k > n");
    }

    #[test]
    fn test_dimension_mismatch_on_insert() {
        let mut idx = make_index(3);
        let result = idx.insert(NodeId(1), &[1.0, 2.0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_dimension_mismatch_on_search() {
        let mut idx = make_index(3);
        idx.insert(NodeId(1), &[1.0, 2.0, 3.0]).unwrap();
        let result = idx.search(&[1.0, 2.0], 5, 50);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_node() {
        let mut idx = make_index(2);
        idx.insert(NodeId(1), &[0.0, 0.0]).unwrap();
        idx.insert(NodeId(2), &[1.0, 0.0]).unwrap();
        idx.insert(NodeId(3), &[0.0, 1.0]).unwrap();

        assert!(idx.remove(NodeId(2)).unwrap());
        assert_eq!(idx.len(), 2);

        // Searching should still work.
        let results = idx.search(&[1.0, 0.0], 5, 50).unwrap();
        assert_eq!(results.len(), 2);
        // NodeId(2) should not appear in results.
        for (nid, _) in &results {
            assert_ne!(*nid, NodeId(2));
        }
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut idx = make_index(2);
        assert!(!idx.remove(NodeId(999)).unwrap());
    }

    #[test]
    fn test_brute_force_correctness_euclidean() {
        // Insert 100 random vectors, search for the 5 nearest, compare against brute force.
        let dim = 32;
        let n = 100;
        let k = 5;
        let mut idx = make_index(dim);

        let mut rng = rand::thread_rng();
        let mut all_vectors: Vec<(NodeId, Vec<f32>)> = Vec::new();

        for i in 0..n {
            let v: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>()).collect();
            let nid = NodeId(i as u64);
            idx.insert(nid, &v).unwrap();
            all_vectors.push((nid, v));
        }

        // Query vector.
        let query: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>()).collect();

        // Brute-force search.
        let mut brute_force: Vec<(NodeId, f32)> = all_vectors
            .iter()
            .map(|(nid, v)| {
                let d = crate::distance::euclidean_distance(&query, v).unwrap();
                (*nid, d)
            })
            .collect();
        brute_force.sort_by_key(|a| OrderedFloat(a.1));
        let bf_top_k: Vec<NodeId> = brute_force.iter().take(k).map(|(nid, _)| *nid).collect();

        // HNSW search with high ef for accuracy.
        let hnsw_results = idx.search(&query, k, 200).unwrap();
        let hnsw_top_k: Vec<NodeId> = hnsw_results.iter().map(|(nid, _)| *nid).collect();

        // With ef_search=200, HNSW should find the exact nearest neighbor in most cases.
        // Check that the #1 result matches.
        assert_eq!(
            hnsw_top_k[0], bf_top_k[0],
            "HNSW should find the exact nearest neighbor"
        );

        // Check recall: at least 4 of 5 should match.
        let recall: usize = hnsw_top_k.iter().filter(|id| bf_top_k.contains(id)).count();
        assert!(recall >= 4, "recall should be at least 4/5, got {recall}/5");
    }

    #[test]
    fn test_seeded_random_level_is_deterministic() {
        // astraeadb-issues.md #18. Two indexes built with the same seed +
        // the same insert sequence produce byte-identical node_levels.
        let build = || {
            let mut idx = HnswIndex::with_seed(4, DistanceMetric::Euclidean, 16, 200, 42);
            for i in 1..=25u64 {
                let v: Vec<f32> = vec![(i as f32).sin(), (i as f32).cos(), i as f32 * 0.1, 0.5];
                idx.insert(NodeId(i), &v).unwrap();
            }
            idx
        };
        let a = build();
        let b = build();
        assert_eq!(a.node_levels, b.node_levels, "level assignment must match");
        assert_eq!(a.max_level, b.max_level);
    }

    #[test]
    fn test_unseeded_index_still_builds() {
        // Back-compat: unseeded indexes still work via thread_rng.
        let mut idx = make_index(3);
        for i in 1..=10u64 {
            idx.insert(NodeId(i), &[i as f32, 0.0, 0.0]).unwrap();
        }
        assert_eq!(idx.len(), 10);
    }

    #[test]
    fn test_brute_force_correctness_cosine() {
        let dim = 16;
        let n = 50;
        let k = 3;
        let mut idx = HnswIndex::new(dim, DistanceMetric::Cosine, 16, 200);

        let mut rng = rand::thread_rng();
        let mut all_vectors: Vec<(NodeId, Vec<f32>)> = Vec::new();

        for i in 0..n {
            let v: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>() + 0.01).collect();
            let nid = NodeId(i as u64);
            idx.insert(nid, &v).unwrap();
            all_vectors.push((nid, v));
        }

        let query: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>() + 0.01).collect();

        // Brute-force.
        let mut brute_force: Vec<(NodeId, f32)> = all_vectors
            .iter()
            .map(|(nid, v)| {
                let d = crate::distance::cosine_distance(&query, v).unwrap();
                (*nid, d)
            })
            .collect();
        brute_force.sort_by_key(|a| OrderedFloat(a.1));
        let bf_top_k: Vec<NodeId> = brute_force.iter().take(k).map(|(nid, _)| *nid).collect();

        let hnsw_results = idx.search(&query, k, 200).unwrap();
        let hnsw_top_k: Vec<NodeId> = hnsw_results.iter().map(|(nid, _)| *nid).collect();

        // Check nearest neighbor.
        assert_eq!(hnsw_top_k[0], bf_top_k[0]);
    }

    #[test]
    fn test_insert_and_remove_then_search() {
        const DIM: usize = 4;
        let mut idx = make_index(DIM);

        // Insert 20 vectors.
        for i in 0..20u64 {
            let v = vec![i as f32; DIM];
            idx.insert(NodeId(i), &v).unwrap();
        }
        assert_eq!(idx.len(), 20);

        // Remove half.
        for i in 0..10u64 {
            idx.remove(NodeId(i)).unwrap();
        }
        assert_eq!(idx.len(), 10);

        // Search should still return valid results from the remaining nodes.
        let results = idx.search(&[15.0; DIM], 5, 50).unwrap();
        assert!(!results.is_empty());
        for (nid, _) in &results {
            assert!(nid.0 >= 10, "should only find remaining nodes (>= 10)");
        }
    }

    #[test]
    fn test_remove_entry_point() {
        let mut idx = make_index(2);
        idx.insert(NodeId(1), &[0.0, 0.0]).unwrap();
        idx.insert(NodeId(2), &[1.0, 0.0]).unwrap();
        idx.insert(NodeId(3), &[0.0, 1.0]).unwrap();

        // Remove all nodes, including the entry point.
        let ep = idx.entry_point.unwrap();
        idx.remove(ep).unwrap();
        assert_eq!(idx.len(), 2);

        // Should still be able to search.
        let results = idx.search(&[0.5, 0.5], 5, 50).unwrap();
        assert!(!results.is_empty());
    }

    // --- Task 4 matrix tests ---

    /// (a) Build an index at the motivating 768-dim size, insert and search successfully.
    #[test]
    fn test_non_128_dimension_insert_and_search_768() {
        const DIM: usize = 768;
        let mut idx = HnswIndex::new(DIM, DistanceMetric::Cosine, 16, 200);
        assert_eq!(idx.dimension(), DIM);

        // Insert a one-hot vector at position 0 and position 1.
        let mut v0 = vec![0.0f32; DIM];
        v0[0] = 1.0;
        let mut v1 = vec![0.0f32; DIM];
        v1[1] = 1.0;

        idx.insert(NodeId(1), &v0).unwrap();
        idx.insert(NodeId(2), &v1).unwrap();
        assert_eq!(idx.len(), 2);

        // Query with v0; nearest should be NodeId(1) at distance ~0.
        let results = idx.search(&v0, 1, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, NodeId(1));
        assert!(
            results[0].1 < 1e-5,
            "cosine distance to identical vector should be ~0"
        );
    }

    /// (b) DimensionMismatch on insert — assert the exact error variant and field values.
    #[test]
    fn test_dimension_mismatch_on_insert_exact_variant() {
        let mut idx = make_index(768);
        // Supply a 3-element vector to a 768-dim index.
        let result = idx.insert(NodeId(1), &[1.0, 0.0, 0.0]);
        match result {
            Err(AstraeaError::DimensionMismatch { expected, got }) => {
                assert_eq!(expected, 768, "expected field must be the index dimension");
                assert_eq!(got, 3, "got field must be the supplied vector length");
            }
            other => panic!("expected DimensionMismatch, got: {other:?}"),
        }
    }

    /// (c) DimensionMismatch on search — assert the exact error variant and field values.
    #[test]
    fn test_dimension_mismatch_on_search_exact_variant() {
        let mut idx = make_index(768);
        let mut v = vec![0.0f32; 768];
        v[0] = 1.0;
        idx.insert(NodeId(1), &v).unwrap();

        // Query with a 5-element vector.
        let result = idx.search(&[1.0, 0.0, 0.0, 0.0, 0.0], 1, 50);
        match result {
            Err(AstraeaError::DimensionMismatch { expected, got }) => {
                assert_eq!(expected, 768, "expected field must be the index dimension");
                assert_eq!(got, 5, "got field must be the query vector length");
            }
            other => panic!("expected DimensionMismatch, got: {other:?}"),
        }
    }

    // --- Task 1 (issue-26 §11): node_ids / contains ---

    /// node_ids returns exactly the inserted ids; contains reflects insert+remove.
    #[test]
    fn test_node_ids_and_contains_after_insert_remove() {
        let mut idx = make_index(2);

        // Empty index.
        assert!(idx.node_ids().is_empty());
        assert!(!idx.contains(NodeId(1)));

        // Insert three nodes.
        idx.insert(NodeId(1), &[1.0, 0.0]).unwrap();
        idx.insert(NodeId(2), &[0.0, 1.0]).unwrap();
        idx.insert(NodeId(3), &[1.0, 1.0]).unwrap();

        let mut ids = idx.node_ids();
        ids.sort();
        assert_eq!(ids, vec![NodeId(1), NodeId(2), NodeId(3)]);
        assert!(idx.contains(NodeId(1)));
        assert!(idx.contains(NodeId(2)));
        assert!(idx.contains(NodeId(3)));
        assert!(!idx.contains(NodeId(99)));

        // Remove one node.
        assert!(idx.remove(NodeId(2)).unwrap());

        let mut ids = idx.node_ids();
        ids.sort();
        assert_eq!(ids, vec![NodeId(1), NodeId(3)]);
        assert!(idx.contains(NodeId(1)));
        assert!(
            !idx.contains(NodeId(2)),
            "removed node must not be contained"
        );
        assert!(idx.contains(NodeId(3)));
    }

    // --- Issue #25: recall@k regression guard for graph construction quality ---

    /// Generate `n` clustered vectors at dimension `dim`: pick `n_clusters`
    /// random centres, then draw each vector as a centre plus per-component
    /// Gaussian noise. Clustered data is the regime where the old naive
    /// top-`M` neighbour selection collapsed (recall ~0.6 at scale); the
    /// SELECT-NEIGHBORS-HEURISTIC must hold recall@10 >= 0.95 here.
    ///
    /// Fully seeded (Box–Muller over a seeded `StdRng`) so the test is
    /// deterministic run-to-run (astraeadb-issues.md #18).
    fn make_clustered_vectors(n: usize, dim: usize, n_clusters: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut rng = StdRng::seed_from_u64(seed);

        // Standard normal sample via Box–Muller.
        let mut gauss = |rng: &mut StdRng| -> f32 {
            let u1: f32 = rng.r#gen::<f32>().max(1e-9);
            let u2: f32 = rng.r#gen::<f32>();
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
        };

        // Cluster centres, spread out over the unit cube.
        let centres: Vec<Vec<f32>> = (0..n_clusters)
            .map(|_| (0..dim).map(|_| rng.r#gen::<f32>()).collect())
            .collect();

        (0..n)
            .map(|i| {
                let c = &centres[i % n_clusters];
                (0..dim).map(|d| c[d] + 0.55 * gauss(&mut rng)).collect()
            })
            .collect()
    }

    /// Build a seeded index over `vectors`, then measure mean recall@k for a
    /// sample of queries against a brute-force cosine ground truth over the
    /// retained copies. Returns (mean_recall, build_secs, mean_query_micros).
    fn measure_recall(
        vectors: &[Vec<f32>],
        dim: usize,
        k: usize,
        ef_search: usize,
        n_queries: usize,
    ) -> (f64, f64, f64) {
        use std::time::Instant;

        let mut idx = HnswIndex::with_seed(dim, DistanceMetric::Cosine, 16, 200, 0xC0FFEE);

        let build_start = Instant::now();
        for (i, v) in vectors.iter().enumerate() {
            idx.insert(NodeId(i as u64), v).unwrap();
        }
        let build_secs = build_start.elapsed().as_secs_f64();

        // Use the first `n_queries` stored vectors as queries (they exist in
        // the index, so the nearest neighbour is the vector itself — recall@k
        // then measures whether the other k-1 true neighbours are found too).
        let mut total_recall = 0.0f64;
        let mut total_query_micros = 0.0f64;
        for q in 0..n_queries {
            let query = &vectors[q];

            // Brute-force cosine ground truth.
            let mut bf: Vec<(NodeId, f32)> = vectors
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    (
                        NodeId(i as u64),
                        crate::distance::cosine_distance(query, v).unwrap(),
                    )
                })
                .collect();
            bf.sort_by_key(|a| OrderedFloat(a.1));
            let truth: HashSet<NodeId> = bf.iter().take(k).map(|(id, _)| *id).collect();

            let qstart = Instant::now();
            let results = idx.search(query, k, ef_search).unwrap();
            total_query_micros += qstart.elapsed().as_secs_f64() * 1e6;

            let hit = results.iter().filter(|(id, _)| truth.contains(id)).count();
            total_recall += hit as f64 / k as f64;
        }

        (
            total_recall / n_queries as f64,
            build_secs,
            total_query_micros / n_queries as f64,
        )
    }

    /// Always-on guard: clustered set large enough to expose the construction
    /// bug (the old top-M selection already sat well below 0.95 here) but small
    /// enough to run in debug `cargo test`. Pins the recall floor so graph
    /// quality cannot silently regress.
    #[test]
    fn test_recall_clustered_guard_dim128() {
        let dim = 128;
        let vectors = make_clustered_vectors(3_000, dim, 100, 1);
        let (recall, _build, _q) = measure_recall(&vectors, dim, 10, 64, 100);
        assert!(
            recall >= 0.95,
            "recall@10 on clustered dim-128 set must stay >= 0.95, got {recall:.3}"
        );
    }

    /// Full-scale acceptance guard for #25 at N=10k, dim 128. Heavy in debug;
    /// run with `cargo test --release -- --ignored` (this is the CI guard).
    #[test]
    #[ignore = "heavy: N=10k build; run in release via `cargo test --release -- --ignored`"]
    fn test_recall_clustered_10k_dim128() {
        let dim = 128;
        let vectors = make_clustered_vectors(10_000, dim, 100, 2);
        let (recall, build, q) = measure_recall(&vectors, dim, 10, 64, 200);
        eprintln!("dim128 N=10k recall@10={recall:.3} build={build:.1}s query={q:.1}us");
        assert!(
            recall >= 0.95,
            "recall@10 at N=10k dim 128 must be >= 0.95, got {recall:.3}"
        );
    }

    /// Full-scale acceptance guard for #25 at N=10k, dim 768 (the a-llama
    /// regime). Heavy in debug; run with `cargo test --release -- --ignored`.
    #[test]
    #[ignore = "heavy: N=10k x dim768 build; run in release via `cargo test --release -- --ignored`"]
    fn test_recall_clustered_10k_dim768() {
        let dim = 768;
        let vectors = make_clustered_vectors(10_000, dim, 100, 3);
        let (recall, build, q) = measure_recall(&vectors, dim, 10, 64, 200);
        eprintln!("dim768 N=10k recall@10={recall:.3} build={build:.1}s query={q:.1}us");
        assert!(
            recall >= 0.95,
            "recall@10 at N=10k dim 768 must be >= 0.95, got {recall:.3}"
        );
    }
}

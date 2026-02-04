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
use rand::Rng;

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
}

impl HnswIndex {
    /// Create a new, empty HNSW index.
    ///
    /// # Parameters
    /// - `dimension`: the fixed dimensionality of all vectors
    /// - `metric`: the distance metric to use
    /// - `m`: max connections per node per layer (default recommendation: 16)
    /// - `ef_construction`: beam width during insertion (default recommendation: 200)
    pub fn new(dimension: usize, metric: DistanceMetric, m: usize, ef_construction: usize) -> Self {
        let m_max0 = m * 2;
        let ml = 1.0 / (m as f64).ln();

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

    /// Return the configured dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Return the configured metric.
    pub fn metric(&self) -> DistanceMetric {
        self.metric
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
            self.layers[l].entry(node_id).or_insert_with(Vec::new);
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

            // Select neighbors: take the top `max_conn` closest.
            let neighbors: Vec<NodeId> = candidates
                .into_iter()
                .take(max_conn)
                .map(|(_, id)| id)
                .collect();

            // Update the current entry point for the next (lower) layer.
            if let Some(first) = neighbors.first() {
                current_ep = *first;
            }

            // Add bidirectional connections.
            for &neighbor in &neighbors {
                // node -> neighbor
                if let Some(adj) = self.layers[l].get_mut(&node_id) {
                    if !adj.contains(&neighbor) {
                        adj.push(neighbor);
                    }
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
                            if let Some(adj) = self.layers[l].get_mut(&a) {
                                if adj.len() < max_conn {
                                    adj.push(b);
                                }
                            }
                            if let Some(adj) = self.layers[l].get_mut(&b) {
                                if adj.len() < max_conn {
                                    adj.push(a);
                                }
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

        while let Some(Candidate { distance: c_dist, node_id: c_id }) = candidates.pop() {
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
        result_vec.sort_by(|a, b| OrderedFloat(a.0).cmp(&OrderedFloat(b.0)));

        Ok(result_vec)
    }

    /// Shrink a node's connection list to at most `max_conn` by keeping
    /// the closest neighbors.
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

        // Score each neighbor by distance.
        let mut scored: Vec<(OrderedFloat<f32>, NodeId)> = Vec::with_capacity(neighbors.len());
        for &n in &neighbors {
            if let Some(nv) = self.vectors.get(&n) {
                let d = compute_distance(self.metric, &node_vec, nv)?;
                scored.push((OrderedFloat(d), n));
            }
        }
        scored.sort_by(|a, b| a.0.cmp(&b.0));
        scored.truncate(max_conn);

        let new_neighbors: Vec<NodeId> = scored.into_iter().map(|(_, id)| id).collect();
        if let Some(adj) = self.layers[layer].get_mut(&node_id) {
            *adj = new_neighbors;
        }

        Ok(())
    }

    /// Compute distance between a query vector and a stored node's vector.
    fn distance(&self, query: &[f32], node_id: NodeId) -> Result<f32> {
        let v = self.vectors.get(&node_id).ok_or(AstraeaError::NoEmbedding(node_id))?;
        compute_distance(self.metric, query, v)
    }

    /// Generate a random level using an exponential distribution.
    ///
    /// level = floor(-ln(uniform(0,1)) * ml)
    fn random_level(&self) -> usize {
        let mut rng = rand::thread_rng();
        let r: f64 = rng.r#gen::<f64>();
        // Avoid ln(0) which is -infinity.
        let r = r.max(1e-10);
        let level = (-r.ln() * self.ml).floor() as usize;
        level
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
        assert!(results[0].1 < 1e-6, "distance to identical vector should be ~0");
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
        brute_force.sort_by(|a, b| OrderedFloat(a.1).cmp(&OrderedFloat(b.1)));
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
        assert!(
            recall >= 4,
            "recall should be at least 4/5, got {recall}/5"
        );
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
        brute_force.sort_by(|a, b| OrderedFloat(a.1).cmp(&OrderedFloat(b.1)));
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
}

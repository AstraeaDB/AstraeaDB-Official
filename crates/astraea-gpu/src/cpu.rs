use std::collections::HashMap;

use crate::backend::{ComputeResult, GpuBackend, GpuPageRankConfig};
use crate::csr::CsrMatrix;

/// CPU-based compute backend (fallback when GPU is not available).
///
/// Provides reference implementations of graph algorithms using standard
/// Rust iterators and loops on CSR matrices. These are correct but
/// single-threaded; the GPU backend will parallelize the same operations.
pub struct CpuBackend;

impl CpuBackend {
    pub fn new() -> Self {
        CpuBackend
    }
}

impl Default for CpuBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuBackend for CpuBackend {
    fn pagerank(&self, matrix: &CsrMatrix, config: &GpuPageRankConfig) -> ComputeResult {
        let n = matrix.num_nodes;
        if n == 0 {
            return ComputeResult::PageRank(HashMap::new());
        }

        let d = config.damping;
        let teleport = (1.0 - d) / n as f64;

        // Build the transpose so we can iterate over incoming edges efficiently.
        let transposed = matrix.transpose();

        // Compute out-degrees from the original matrix.
        let out_degrees = matrix.out_degrees();

        // Initialize ranks uniformly.
        let mut rank = vec![1.0 / n as f64; n];

        for _iter in 0..config.max_iterations {
            let mut new_rank = vec![teleport; n];

            // For each node i, sum contributions from its incoming neighbors.
            // In the transposed matrix, row i lists all nodes j that have an
            // edge TO i in the original graph.
            for i in 0..n {
                let start = transposed.row_ptr[i];
                let end = transposed.row_ptr[i + 1];

                for idx in start..end {
                    let j = transposed.col_indices[idx]; // j -> i in original
                    let deg_j = out_degrees[j];
                    if deg_j > 0 {
                        new_rank[i] += d * rank[j] / deg_j as f64;
                    }
                }
            }

            // Handle dangling nodes (nodes with no outgoing edges).
            // Their rank "leaks" and is redistributed uniformly.
            let dangling_sum: f64 = (0..n)
                .filter(|&j| out_degrees[j] == 0)
                .map(|j| rank[j])
                .sum();

            if dangling_sum > 0.0 {
                let dangling_contrib = d * dangling_sum / n as f64;
                for r in &mut new_rank {
                    *r += dangling_contrib;
                }
            }

            // Check convergence (L1 norm of difference).
            let diff: f64 = rank
                .iter()
                .zip(new_rank.iter())
                .map(|(old, new)| (old - new).abs())
                .sum();

            rank = new_rank;

            if diff < config.tolerance {
                break;
            }
        }

        // Convert to HashMap<NodeId, f64>.
        let mut result = HashMap::with_capacity(n);
        for (idx, &score) in rank.iter().enumerate() {
            result.insert(matrix.index_to_node[idx], score);
        }

        ComputeResult::PageRank(result)
    }

    fn bfs(&self, matrix: &CsrMatrix, source: usize) -> ComputeResult {
        let n = matrix.num_nodes;

        // Initialize all levels to -1 (unreachable).
        let mut levels = vec![-1i32; n];

        if source >= n {
            // Invalid source: return all unreachable.
            let mut result = HashMap::with_capacity(n);
            for (idx, &level) in levels.iter().enumerate() {
                result.insert(matrix.index_to_node[idx], level);
            }
            return ComputeResult::BfsLevels(result);
        }

        levels[source] = 0;
        let mut frontier = vec![source];
        let mut current_level = 0i32;

        while !frontier.is_empty() {
            let mut next_frontier = Vec::new();
            current_level += 1;

            for &node in &frontier {
                let start = matrix.row_ptr[node];
                let end = matrix.row_ptr[node + 1];

                for idx in start..end {
                    let neighbor = matrix.col_indices[idx];
                    if levels[neighbor] == -1 {
                        levels[neighbor] = current_level;
                        next_frontier.push(neighbor);
                    }
                }
            }

            frontier = next_frontier;
        }

        // Convert to HashMap<NodeId, i32>.
        let mut result = HashMap::with_capacity(n);
        for (idx, &level) in levels.iter().enumerate() {
            result.insert(matrix.index_to_node[idx], level);
        }

        ComputeResult::BfsLevels(result)
    }

    fn sssp(&self, matrix: &CsrMatrix, source: usize) -> ComputeResult {
        let n = matrix.num_nodes;

        // Initialize all distances to infinity.
        let mut dist = vec![f64::INFINITY; n];

        if source < n {
            dist[source] = 0.0;
        }

        // Bellman-Ford: relax all edges N-1 times.
        for _ in 0..n.saturating_sub(1) {
            let mut changed = false;

            for u in 0..n {
                if dist[u] == f64::INFINITY {
                    continue; // no path to u yet, skip
                }

                let start = matrix.row_ptr[u];
                let end = matrix.row_ptr[u + 1];

                for idx in start..end {
                    let v = matrix.col_indices[idx];
                    let weight = matrix.values[idx];
                    let new_dist = dist[u] + weight;

                    if new_dist < dist[v] {
                        dist[v] = new_dist;
                        changed = true;
                    }
                }
            }

            // Early exit if no relaxation happened this round.
            if !changed {
                break;
            }
        }

        // Convert to HashMap<NodeId, f64>.
        let mut result = HashMap::with_capacity(n);
        for (idx, &d) in dist.iter().enumerate() {
            result.insert(matrix.index_to_node[idx], d);
        }

        ComputeResult::SsspDistances(result)
    }

    fn name(&self) -> &str {
        "CPU"
    }

    fn is_available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csr::CsrMatrix;
    use astraea_core::error::{AstraeaError, Result};
    use astraea_core::traits::GraphOps;
    use astraea_core::types::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A minimal in-memory graph for testing CPU backend algorithms.
    struct MockGraph {
        nodes: HashMap<NodeId, Node>,
        edges: HashMap<EdgeId, Edge>,
        outgoing: HashMap<NodeId, Vec<(EdgeId, NodeId)>>,
        next_node_id: Mutex<u64>,
        next_edge_id: Mutex<u64>,
    }

    impl MockGraph {
        fn new() -> Self {
            Self {
                nodes: HashMap::new(),
                edges: HashMap::new(),
                outgoing: HashMap::new(),
                next_node_id: Mutex::new(1),
                next_edge_id: Mutex::new(1),
            }
        }

        fn add_node(&mut self, labels: Vec<String>) -> NodeId {
            let mut id_guard = self.next_node_id.lock().unwrap();
            let id = NodeId(*id_guard);
            *id_guard += 1;

            self.nodes.insert(
                id,
                Node {
                    id,
                    labels,
                    properties: serde_json::json!({}),
                    embedding: None,
                },
            );
            self.outgoing.entry(id).or_default();
            id
        }

        fn add_edge(&mut self, source: NodeId, target: NodeId, weight: f64) -> EdgeId {
            let mut id_guard = self.next_edge_id.lock().unwrap();
            let id = EdgeId(*id_guard);
            *id_guard += 1;

            self.edges.insert(
                id,
                Edge {
                    id,
                    source,
                    target,
                    edge_type: "LINK".into(),
                    properties: serde_json::json!({}),
                    weight,
                    validity: ValidityInterval::always(),
                },
            );
            self.outgoing.entry(source).or_default().push((id, target));
            id
        }
    }

    impl GraphOps for MockGraph {
        fn create_node(
            &self,
            _labels: Vec<String>,
            _properties: serde_json::Value,
            _embedding: Option<Vec<f32>>,
        ) -> Result<NodeId> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn create_edge(
            &self,
            _source: NodeId,
            _target: NodeId,
            _edge_type: String,
            _properties: serde_json::Value,
            _weight: f64,
            _valid_from: Option<i64>,
            _valid_to: Option<i64>,
        ) -> Result<EdgeId> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
            Ok(self.nodes.get(&id).cloned())
        }

        fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
            Ok(self.edges.get(&id).cloned())
        }

        fn update_node(&self, _id: NodeId, _properties: serde_json::Value) -> Result<()> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn update_edge(&self, _id: EdgeId, _properties: serde_json::Value) -> Result<()> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn delete_node(&self, _id: NodeId) -> Result<()> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn delete_edge(&self, _id: EdgeId) -> Result<()> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn neighbors(
            &self,
            node_id: NodeId,
            direction: Direction,
        ) -> Result<Vec<(EdgeId, NodeId)>> {
            match direction {
                Direction::Outgoing => {
                    Ok(self.outgoing.get(&node_id).cloned().unwrap_or_default())
                }
                Direction::Incoming => {
                    let mut result = Vec::new();
                    for edge in self.edges.values() {
                        if edge.target == node_id {
                            result.push((edge.id, edge.source));
                        }
                    }
                    Ok(result)
                }
                Direction::Both => {
                    let mut result =
                        self.outgoing.get(&node_id).cloned().unwrap_or_default();
                    for edge in self.edges.values() {
                        if edge.target == node_id {
                            result.push((edge.id, edge.source));
                        }
                    }
                    Ok(result)
                }
            }
        }

        fn neighbors_filtered(
            &self,
            _node_id: NodeId,
            _direction: Direction,
            _edge_type: &str,
        ) -> Result<Vec<(EdgeId, NodeId)>> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn bfs(&self, _start: NodeId, _max_depth: usize) -> Result<Vec<(NodeId, usize)>> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn dfs(&self, _start: NodeId, _max_depth: usize) -> Result<Vec<NodeId>> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn shortest_path(&self, _from: NodeId, _to: NodeId) -> Result<Option<GraphPath>> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn shortest_path_weighted(
            &self,
            _from: NodeId,
            _to: NodeId,
        ) -> Result<Option<(GraphPath, f64)>> {
            Err(AstraeaError::QueryExecution(
                "not supported in mock".into(),
            ))
        }

        fn find_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
            let mut result = Vec::new();
            for node in self.nodes.values() {
                if node.labels.contains(&label.to_string()) {
                    result.push(node.id);
                }
            }
            Ok(result)
        }
    }

    /// Build a 4-node graph for PageRank/BFS/SSSP testing:
    ///   n1 -> n2  (weight 1.0)
    ///   n2 -> n3  (weight 2.0)
    ///   n3 -> n4  (weight 3.0)
    ///   n1 -> n3  (weight 0.5)
    fn make_test_graph() -> (MockGraph, [NodeId; 4]) {
        let mut g = MockGraph::new();
        let n1 = g.add_node(vec![]);
        let n2 = g.add_node(vec![]);
        let n3 = g.add_node(vec![]);
        let n4 = g.add_node(vec![]);

        g.add_edge(n1, n2, 1.0);
        g.add_edge(n2, n3, 2.0);
        g.add_edge(n3, n4, 3.0);
        g.add_edge(n1, n3, 0.5);

        (g, [n1, n2, n3, n4])
    }

    /// Build a strongly connected 3-node cycle graph for PageRank:
    ///   n1 -> n2  (weight 1.0)
    ///   n2 -> n3  (weight 1.0)
    ///   n3 -> n1  (weight 1.0)
    fn make_cycle_graph() -> (MockGraph, [NodeId; 3]) {
        let mut g = MockGraph::new();
        let n1 = g.add_node(vec![]);
        let n2 = g.add_node(vec![]);
        let n3 = g.add_node(vec![]);

        g.add_edge(n1, n2, 1.0);
        g.add_edge(n2, n3, 1.0);
        g.add_edge(n3, n1, 1.0);

        (g, [n1, n2, n3])
    }

    #[test]
    fn test_cpu_pagerank_cycle_graph() {
        // In a symmetric cycle, all nodes should have equal rank.
        let (graph, nodes) = make_cycle_graph();
        let [n1, n2, n3] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3]).unwrap();
        let backend = CpuBackend::new();
        let config = GpuPageRankConfig::default();

        let result = backend.pagerank(&csr, &config);

        if let ComputeResult::PageRank(ranks) = result {
            // All ranks should be approximately 1/3.
            for &node in &[n1, n2, n3] {
                let r = ranks[&node];
                assert!(
                    (r - 1.0 / 3.0).abs() < 1e-4,
                    "expected rank ~0.333 for {:?}, got {}",
                    node,
                    r
                );
            }

            // Ranks should sum to approximately 1.0.
            let total: f64 = ranks.values().sum();
            assert!(
                (total - 1.0).abs() < 1e-4,
                "expected total rank ~1.0, got {}",
                total
            );
        } else {
            panic!("expected ComputeResult::PageRank");
        }
    }

    #[test]
    fn test_cpu_pagerank_dag() {
        // In a DAG n1->n2->n3->n4 with n1->n3, rank should flow toward
        // later nodes. n4 (the sink) should accumulate the most rank.
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();
        let backend = CpuBackend::new();
        let config = GpuPageRankConfig::default();

        let result = backend.pagerank(&csr, &config);

        if let ComputeResult::PageRank(ranks) = result {
            // Ranks should sum to approximately 1.0.
            let total: f64 = ranks.values().sum();
            assert!(
                (total - 1.0).abs() < 1e-4,
                "expected total rank ~1.0, got {}",
                total
            );

            // n4 is a dangling node (sink) -- it should have high rank
            // because it receives incoming edges but passes rank nowhere.
            assert!(
                ranks[&n4] > ranks[&n1],
                "sink node n4 should have higher rank than source n1"
            );
        } else {
            panic!("expected ComputeResult::PageRank");
        }
    }

    #[test]
    fn test_cpu_pagerank_empty_graph() {
        let g = MockGraph::new();
        let csr = CsrMatrix::from_graph(&g, &[]).unwrap();
        let backend = CpuBackend::new();
        let config = GpuPageRankConfig::default();

        let result = backend.pagerank(&csr, &config);

        if let ComputeResult::PageRank(ranks) = result {
            assert!(ranks.is_empty());
        } else {
            panic!("expected ComputeResult::PageRank");
        }
    }

    #[test]
    fn test_cpu_bfs_levels() {
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();
        let backend = CpuBackend::new();

        let source_idx = csr.node_to_index[&n1];
        let result = backend.bfs(&csr, source_idx);

        if let ComputeResult::BfsLevels(levels) = result {
            // n1 is the source -> level 0.
            assert_eq!(levels[&n1], 0);

            // n2 is 1 hop from n1.
            assert_eq!(levels[&n2], 1);

            // n3 is 1 hop from n1 (direct edge n1->n3).
            assert_eq!(levels[&n3], 1);

            // n4 is 2 hops from n1 (n1->n3->n4).
            assert_eq!(levels[&n4], 2);
        } else {
            panic!("expected ComputeResult::BfsLevels");
        }
    }

    #[test]
    fn test_cpu_bfs_unreachable() {
        // Build a disconnected graph: n1->n2, n3 (isolated).
        let mut g = MockGraph::new();
        let n1 = g.add_node(vec![]);
        let n2 = g.add_node(vec![]);
        let n3 = g.add_node(vec![]);
        g.add_edge(n1, n2, 1.0);

        let csr = CsrMatrix::from_graph(&g, &[n1, n2, n3]).unwrap();
        let backend = CpuBackend::new();

        let source_idx = csr.node_to_index[&n1];
        let result = backend.bfs(&csr, source_idx);

        if let ComputeResult::BfsLevels(levels) = result {
            assert_eq!(levels[&n1], 0);
            assert_eq!(levels[&n2], 1);
            assert_eq!(levels[&n3], -1); // unreachable
        } else {
            panic!("expected ComputeResult::BfsLevels");
        }
    }

    #[test]
    fn test_cpu_bfs_from_middle() {
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();
        let backend = CpuBackend::new();

        let source_idx = csr.node_to_index[&n2];
        let result = backend.bfs(&csr, source_idx);

        if let ComputeResult::BfsLevels(levels) = result {
            // n1 is not reachable from n2 (no incoming path traversal in BFS).
            assert_eq!(levels[&n1], -1);
            assert_eq!(levels[&n2], 0);
            assert_eq!(levels[&n3], 1);
            assert_eq!(levels[&n4], 2);
        } else {
            panic!("expected ComputeResult::BfsLevels");
        }
    }

    #[test]
    fn test_cpu_sssp_distances() {
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();
        let backend = CpuBackend::new();

        let source_idx = csr.node_to_index[&n1];
        let result = backend.sssp(&csr, source_idx);

        if let ComputeResult::SsspDistances(dists) = result {
            // n1 -> n1: 0.0
            assert!((dists[&n1] - 0.0).abs() < 1e-12);

            // n1 -> n2: 1.0 (direct edge, weight 1.0)
            assert!((dists[&n2] - 1.0).abs() < 1e-12);

            // n1 -> n3: min(0.5 direct, 1.0+2.0 via n2) = 0.5
            assert!((dists[&n3] - 0.5).abs() < 1e-12);

            // n1 -> n4: 0.5 + 3.0 = 3.5 (via n3)
            assert!((dists[&n4] - 3.5).abs() < 1e-12);
        } else {
            panic!("expected ComputeResult::SsspDistances");
        }
    }

    #[test]
    fn test_cpu_sssp_unreachable() {
        // Disconnected graph: n1->n2, n3 (isolated).
        let mut g = MockGraph::new();
        let n1 = g.add_node(vec![]);
        let n2 = g.add_node(vec![]);
        let n3 = g.add_node(vec![]);
        g.add_edge(n1, n2, 5.0);

        let csr = CsrMatrix::from_graph(&g, &[n1, n2, n3]).unwrap();
        let backend = CpuBackend::new();

        let source_idx = csr.node_to_index[&n1];
        let result = backend.sssp(&csr, source_idx);

        if let ComputeResult::SsspDistances(dists) = result {
            assert!((dists[&n1] - 0.0).abs() < 1e-12);
            assert!((dists[&n2] - 5.0).abs() < 1e-12);
            assert!(dists[&n3].is_infinite()); // unreachable
        } else {
            panic!("expected ComputeResult::SsspDistances");
        }
    }

    #[test]
    fn test_cpu_backend_name_and_available() {
        let backend = CpuBackend::new();
        assert_eq!(backend.name(), "CPU");
        assert!(backend.is_available());
    }

    #[test]
    fn test_cpu_backend_default() {
        let backend = CpuBackend::default();
        assert_eq!(backend.name(), "CPU");
    }
}

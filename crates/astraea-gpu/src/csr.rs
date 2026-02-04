use std::collections::HashMap;

use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, NodeId};

/// Compressed Sparse Row representation of a graph's adjacency matrix.
///
/// This format is efficient for sparse matrix operations and GPU transfer.
/// Each row `i` corresponds to a node, and `row_ptr[i]..row_ptr[i+1]` indexes
/// into `col_indices` and `values` to give the outgoing edges (column indices
/// and their weights).
///
/// This is the standard CSR layout used by cuSPARSE, GraphBLAS, and similar
/// libraries, making it a natural bridge between graph topology and linear
/// algebra / GPU kernels.
#[derive(Debug, Clone)]
pub struct CsrMatrix {
    /// Number of nodes (rows/columns in the square adjacency matrix).
    pub num_nodes: usize,
    /// Mapping from NodeId to matrix index (0-based).
    pub node_to_index: HashMap<NodeId, usize>,
    /// Mapping from matrix index back to NodeId.
    pub index_to_node: Vec<NodeId>,
    /// Row pointers (length = num_nodes + 1).
    /// `row_ptr[i]..row_ptr[i+1]` are the positions in `col_indices`/`values` for row `i`.
    pub row_ptr: Vec<usize>,
    /// Column indices (length = number of non-zeros).
    pub col_indices: Vec<usize>,
    /// Edge weights (length = number of non-zeros, parallel to `col_indices`).
    pub values: Vec<f64>,
}

impl CsrMatrix {
    /// Build a CSR matrix from a graph.
    ///
    /// The `nodes` slice determines which nodes are included and their ordering
    /// in the matrix. Only outgoing edges between nodes in the set are included.
    ///
    /// # Algorithm
    ///
    /// 1. Build bidirectional mappings between `NodeId` and 0-based matrix indices.
    /// 2. For each node in order, query outgoing neighbors from the graph.
    /// 3. For each neighbor that is in the node set, record the column index and
    ///    edge weight.
    /// 4. Assemble `row_ptr`, `col_indices`, and `values` arrays.
    pub fn from_graph(graph: &dyn GraphOps, nodes: &[NodeId]) -> astraea_core::error::Result<Self> {
        let num_nodes = nodes.len();

        // Step 1: Build mappings.
        let mut node_to_index = HashMap::with_capacity(num_nodes);
        let mut index_to_node = Vec::with_capacity(num_nodes);

        for (idx, &node_id) in nodes.iter().enumerate() {
            node_to_index.insert(node_id, idx);
            index_to_node.push(node_id);
        }

        // Step 2-4: Build CSR arrays.
        let mut row_ptr = Vec::with_capacity(num_nodes + 1);
        let mut col_indices = Vec::new();
        let mut values = Vec::new();

        for &node_id in nodes {
            row_ptr.push(col_indices.len());

            // Get outgoing neighbors.
            let neighbors = graph.neighbors(node_id, Direction::Outgoing)?;

            for (edge_id, neighbor_id) in neighbors {
                // Only include edges to nodes that are in our node set.
                if let Some(&col_idx) = node_to_index.get(&neighbor_id) {
                    col_indices.push(col_idx);

                    // Retrieve the edge weight.
                    let weight = match graph.get_edge(edge_id)? {
                        Some(edge) => edge.weight,
                        None => 1.0,
                    };
                    values.push(weight);
                }
            }
        }

        // Final sentinel.
        row_ptr.push(col_indices.len());

        Ok(Self {
            num_nodes,
            node_to_index,
            index_to_node,
            row_ptr,
            col_indices,
            values,
        })
    }

    /// Number of non-zero entries in the matrix.
    pub fn nnz(&self) -> usize {
        self.col_indices.len()
    }

    /// Sparse matrix-vector multiply: y = A * x.
    ///
    /// `x` must have length `num_nodes`. Returns a vector `y` of length `num_nodes`
    /// where `y[i] = sum(A[i][j] * x[j])` for all non-zero entries in row `i`.
    ///
    /// # Panics
    ///
    /// Panics if `x.len() != self.num_nodes`.
    pub fn spmv(&self, x: &[f64]) -> Vec<f64> {
        assert_eq!(
            x.len(),
            self.num_nodes,
            "spmv: input vector length {} != num_nodes {}",
            x.len(),
            self.num_nodes
        );

        let mut y = vec![0.0; self.num_nodes];
        for i in 0..self.num_nodes {
            let start = self.row_ptr[i];
            let end = self.row_ptr[i + 1];
            for idx in start..end {
                y[i] += self.values[idx] * x[self.col_indices[idx]];
            }
        }
        y
    }

    /// Get the transpose of this matrix (swap rows and columns).
    ///
    /// If the original matrix represents outgoing edges, the transpose
    /// represents incoming edges. The node mappings are preserved.
    pub fn transpose(&self) -> Self {
        let nnz = self.nnz();

        // Count entries per column (which become rows in the transpose).
        let mut col_counts = vec![0usize; self.num_nodes];
        for &col in &self.col_indices {
            col_counts[col] += 1;
        }

        // Build row_ptr for the transpose via prefix sum.
        let mut t_row_ptr = Vec::with_capacity(self.num_nodes + 1);
        t_row_ptr.push(0);
        for &count in &col_counts {
            let last = *t_row_ptr.last().unwrap();
            t_row_ptr.push(last + count);
        }

        // Fill in col_indices and values for the transpose.
        let mut t_col_indices = vec![0usize; nnz];
        let mut t_values = vec![0.0f64; nnz];
        let mut write_pos = vec![0usize; self.num_nodes];

        for i in 0..self.num_nodes {
            let start = self.row_ptr[i];
            let end = self.row_ptr[i + 1];
            for idx in start..end {
                let col = self.col_indices[idx];
                let dest = t_row_ptr[col] + write_pos[col];
                t_col_indices[dest] = i;
                t_values[dest] = self.values[idx];
                write_pos[col] += 1;
            }
        }

        Self {
            num_nodes: self.num_nodes,
            node_to_index: self.node_to_index.clone(),
            index_to_node: self.index_to_node.clone(),
            row_ptr: t_row_ptr,
            col_indices: t_col_indices,
            values: t_values,
        }
    }

    /// Get the out-degree of each node (number of non-zero entries per row).
    pub fn out_degrees(&self) -> Vec<usize> {
        (0..self.num_nodes)
            .map(|i| self.row_ptr[i + 1] - self.row_ptr[i])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::error::{AstraeaError, Result};
    use astraea_core::traits::GraphOps;
    use astraea_core::types::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A minimal in-memory graph for testing CSR construction.
    ///
    /// Stores nodes and edges in HashMaps, implementing only the GraphOps
    /// methods needed by CsrMatrix::from_graph (neighbors, get_edge) and
    /// stub implementations for everything else.
    struct MockGraph {
        nodes: HashMap<NodeId, Node>,
        edges: HashMap<EdgeId, Edge>,
        /// Adjacency list: node_id -> [(edge_id, neighbor_id)] for outgoing edges.
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
                    // Collect all edges where this node is the target.
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

    /// Build a 4-node test graph:
    ///   n1 -> n2  (weight 1.0)
    ///   n2 -> n3  (weight 2.0)
    ///   n3 -> n4  (weight 3.0)
    ///   n1 -> n3  (weight 0.5)
    fn make_test_graph() -> (MockGraph, [NodeId; 4]) {
        let mut g = MockGraph::new();
        let n1 = g.add_node(vec!["A".into()]);
        let n2 = g.add_node(vec!["B".into()]);
        let n3 = g.add_node(vec!["C".into()]);
        let n4 = g.add_node(vec!["D".into()]);

        g.add_edge(n1, n2, 1.0);
        g.add_edge(n2, n3, 2.0);
        g.add_edge(n3, n4, 3.0);
        g.add_edge(n1, n3, 0.5);

        (g, [n1, n2, n3, n4])
    }

    #[test]
    fn test_csr_construction() {
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();

        assert_eq!(csr.num_nodes, 4);
        assert_eq!(csr.nnz(), 4); // 4 edges total

        // row_ptr should have 5 entries (num_nodes + 1).
        assert_eq!(csr.row_ptr.len(), 5);

        // n1 (index 0) has 2 outgoing edges (to n2 and n3).
        let n1_idx = csr.node_to_index[&n1];
        let n1_start = csr.row_ptr[n1_idx];
        let n1_end = csr.row_ptr[n1_idx + 1];
        assert_eq!(n1_end - n1_start, 2);

        // n2 (index 1) has 1 outgoing edge (to n3).
        let n2_idx = csr.node_to_index[&n2];
        let n2_start = csr.row_ptr[n2_idx];
        let n2_end = csr.row_ptr[n2_idx + 1];
        assert_eq!(n2_end - n2_start, 1);

        // n3 (index 2) has 1 outgoing edge (to n4).
        let n3_idx = csr.node_to_index[&n3];
        let n3_start = csr.row_ptr[n3_idx];
        let n3_end = csr.row_ptr[n3_idx + 1];
        assert_eq!(n3_end - n3_start, 1);

        // n4 (index 3) has 0 outgoing edges.
        let n4_idx = csr.node_to_index[&n4];
        let n4_start = csr.row_ptr[n4_idx];
        let n4_end = csr.row_ptr[n4_idx + 1];
        assert_eq!(n4_end - n4_start, 0);

        // Verify column indices for n1: should point to indices of n2 and n3.
        let n1_cols: Vec<usize> = csr.col_indices[n1_start..n1_end].to_vec();
        assert!(n1_cols.contains(&csr.node_to_index[&n2]));
        assert!(n1_cols.contains(&csr.node_to_index[&n3]));

        // Verify the edge weight from n2 -> n3 is 2.0.
        let n3_col_idx = csr.node_to_index[&n3];
        let n2_edge_pos = (n2_start..n2_end)
            .find(|&idx| csr.col_indices[idx] == n3_col_idx)
            .unwrap();
        assert!((csr.values[n2_edge_pos] - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_csr_spmv() {
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();

        // Multiply by a ones-vector. The result for each row should equal
        // the sum of edge weights in that row.
        let x = vec![1.0; 4];
        let y = csr.spmv(&x);

        // n1: edges to n2 (w=1.0) and n3 (w=0.5) -> sum = 1.5
        let n1_idx = csr.node_to_index[&n1];
        assert!((y[n1_idx] - 1.5).abs() < 1e-12);

        // n2: edge to n3 (w=2.0) -> sum = 2.0
        let n2_idx = csr.node_to_index[&n2];
        assert!((y[n2_idx] - 2.0).abs() < 1e-12);

        // n3: edge to n4 (w=3.0) -> sum = 3.0
        let n3_idx = csr.node_to_index[&n3];
        assert!((y[n3_idx] - 3.0).abs() < 1e-12);

        // n4: no outgoing edges -> sum = 0.0
        let n4_idx = csr.node_to_index[&n4];
        assert!((y[n4_idx] - 0.0).abs() < 1e-12);
    }

    #[test]
    fn test_csr_transpose() {
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();
        let transposed = csr.transpose();

        assert_eq!(transposed.num_nodes, 4);
        assert_eq!(transposed.nnz(), 4); // same number of edges

        // In the transpose, incoming edges become outgoing:
        // n1 has 0 incoming edges in the original graph.
        let n1_idx = transposed.node_to_index[&n1];
        let n1_degree = transposed.row_ptr[n1_idx + 1] - transposed.row_ptr[n1_idx];
        assert_eq!(n1_degree, 0);

        // n2 has 1 incoming edge (from n1).
        let n2_idx = transposed.node_to_index[&n2];
        let n2_degree = transposed.row_ptr[n2_idx + 1] - transposed.row_ptr[n2_idx];
        assert_eq!(n2_degree, 1);

        // n3 has 2 incoming edges (from n1 and n2).
        let n3_idx = transposed.node_to_index[&n3];
        let n3_degree = transposed.row_ptr[n3_idx + 1] - transposed.row_ptr[n3_idx];
        assert_eq!(n3_degree, 2);

        // n4 has 1 incoming edge (from n3).
        let n4_idx = transposed.node_to_index[&n4];
        let n4_degree = transposed.row_ptr[n4_idx + 1] - transposed.row_ptr[n4_idx];
        assert_eq!(n4_degree, 1);

        // Verify that transposing twice gives back the original structure.
        let double_t = transposed.transpose();
        assert_eq!(double_t.row_ptr, csr.row_ptr);
        // Column indices and values may be in different order within each row,
        // so we check row-by-row as sorted pairs.
        for i in 0..csr.num_nodes {
            let orig_start = csr.row_ptr[i];
            let orig_end = csr.row_ptr[i + 1];
            let dt_start = double_t.row_ptr[i];
            let dt_end = double_t.row_ptr[i + 1];

            let mut orig_pairs: Vec<(usize, i64)> = (orig_start..orig_end)
                .map(|idx| {
                    (
                        csr.col_indices[idx],
                        (csr.values[idx] * 1e12) as i64,
                    )
                })
                .collect();
            let mut dt_pairs: Vec<(usize, i64)> = (dt_start..dt_end)
                .map(|idx| {
                    (
                        double_t.col_indices[idx],
                        (double_t.values[idx] * 1e12) as i64,
                    )
                })
                .collect();

            orig_pairs.sort();
            dt_pairs.sort();
            assert_eq!(orig_pairs, dt_pairs);
        }
    }

    #[test]
    fn test_csr_out_degrees() {
        let (graph, nodes) = make_test_graph();
        let [n1, n2, n3, n4] = nodes;

        let csr = CsrMatrix::from_graph(&graph, &[n1, n2, n3, n4]).unwrap();
        let degrees = csr.out_degrees();

        assert_eq!(degrees[csr.node_to_index[&n1]], 2); // n1 -> n2, n3
        assert_eq!(degrees[csr.node_to_index[&n2]], 1); // n2 -> n3
        assert_eq!(degrees[csr.node_to_index[&n3]], 1); // n3 -> n4
        assert_eq!(degrees[csr.node_to_index[&n4]], 0); // no outgoing
    }

    #[test]
    fn test_csr_empty_graph() {
        let g = MockGraph::new();
        let csr = CsrMatrix::from_graph(&g, &[]).unwrap();

        assert_eq!(csr.num_nodes, 0);
        assert_eq!(csr.nnz(), 0);
        assert_eq!(csr.row_ptr, vec![0]);
        assert!(csr.col_indices.is_empty());
        assert!(csr.values.is_empty());
    }

    #[test]
    fn test_csr_single_node_no_edges() {
        let mut g = MockGraph::new();
        let n1 = g.add_node(vec![]);

        let csr = CsrMatrix::from_graph(&g, &[n1]).unwrap();

        assert_eq!(csr.num_nodes, 1);
        assert_eq!(csr.nnz(), 0);
        assert_eq!(csr.row_ptr, vec![0, 0]);
    }
}

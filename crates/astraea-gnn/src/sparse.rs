use std::collections::HashMap;

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, EdgeId, NodeId};

use crate::model::GNNLayer;
use crate::tensor::{Matrix, Tensor};
use crate::message_passing::Activation;

/// Contiguous row-major feature matrix for cache-friendly GNN computation.
///
/// Replaces `HashMap<NodeId, Tensor>` for large graphs. Node i's features
/// are stored at `data[i * feature_dim .. (i+1) * feature_dim]`.
#[derive(Debug, Clone)]
pub struct FeatureMatrix {
    pub data: Vec<f32>,
    pub num_nodes: usize,
    pub feature_dim: usize,
}

impl FeatureMatrix {
    /// Create a zero-filled feature matrix.
    pub fn zeros(num_nodes: usize, feature_dim: usize) -> Self {
        Self {
            data: vec![0.0; num_nodes * feature_dim],
            num_nodes,
            feature_dim,
        }
    }

    /// Build from a HashMap using a fixed node ordering.
    pub fn from_hashmap(
        map: &HashMap<NodeId, Tensor>,
        node_order: &[NodeId],
        feature_dim: usize,
    ) -> Self {
        let num_nodes = node_order.len();
        let mut data = vec![0.0; num_nodes * feature_dim];
        for (i, &nid) in node_order.iter().enumerate() {
            if let Some(tensor) = map.get(&nid) {
                let start = i * feature_dim;
                let copy_len = tensor.data.len().min(feature_dim);
                data[start..start + copy_len].copy_from_slice(&tensor.data[..copy_len]);
            }
        }
        Self {
            data,
            num_nodes,
            feature_dim,
        }
    }

    /// Convert back to HashMap.
    pub fn to_hashmap(&self, node_order: &[NodeId]) -> HashMap<NodeId, Tensor> {
        let mut map = HashMap::with_capacity(self.num_nodes);
        for (i, &nid) in node_order.iter().enumerate() {
            let start = i * self.feature_dim;
            let end = start + self.feature_dim;
            map.insert(nid, Tensor::new(self.data[start..end].to_vec(), false));
        }
        map
    }

    /// Get a slice reference to row i's features.
    pub fn row(&self, i: usize) -> &[f32] {
        let start = i * self.feature_dim;
        &self.data[start..start + self.feature_dim]
    }

    /// Get a mutable slice reference to row i's features.
    pub fn row_mut(&mut self, i: usize) -> &mut [f32] {
        let start = i * self.feature_dim;
        &mut self.data[start..start + self.feature_dim]
    }

    /// Dense matrix multiply: self [N x D] * matrix [D x H] -> result [N x H].
    ///
    /// Note: `matrix` is [rows=D x cols=H] (not transposed).
    /// This computes the equivalent of right-multiplying each row by the matrix.
    /// Since our Matrix uses [output x input] convention for matvec, we need
    /// matrix^T here for right-multiplication.
    pub fn matmul_right(&self, matrix: &Matrix) -> FeatureMatrix {
        // matrix is [output_dim x input_dim] for matvec convention.
        // For right-multiply: result[i] = matrix * row[i] (treating row as column vec).
        let output_dim = matrix.rows;
        let mut result = FeatureMatrix::zeros(self.num_nodes, output_dim);
        for i in 0..self.num_nodes {
            let row = self.row(i);
            let row_tensor = Tensor::new(row.to_vec(), false);
            let out = matrix.matvec(&row_tensor);
            result.row_mut(i).copy_from_slice(&out.data);
        }
        result
    }
}

/// Compressed Sparse Row (CSR) adjacency representation for SpMM.
///
/// Node i's neighbors are at `col_idx[row_ptr[i]..row_ptr[i+1]]` with
/// corresponding edge weights in `weights[row_ptr[i]..row_ptr[i+1]]`.
#[derive(Debug, Clone)]
pub struct CSRAdjacency {
    /// Row pointers: length = num_nodes + 1.
    pub row_ptr: Vec<usize>,
    /// Column indices (local node indices, not NodeIds).
    pub col_idx: Vec<usize>,
    /// Edge weights aligned with col_idx.
    pub weights: Vec<f32>,
    /// Number of nodes.
    pub num_nodes: usize,
}

impl CSRAdjacency {
    /// Build CSR from the graph for a given set of nodes.
    ///
    /// `node_order` defines the local index mapping. Edge weights default to 1.0
    /// if not found in `edge_weights`.
    pub fn from_graph(
        graph: &dyn GraphOps,
        node_order: &[NodeId],
        edge_weights: &HashMap<EdgeId, f32>,
    ) -> Result<Self> {
        let num_nodes = node_order.len();
        let node_to_idx: HashMap<NodeId, usize> = node_order
            .iter()
            .enumerate()
            .map(|(i, &nid)| (nid, i))
            .collect();

        let mut row_ptr = vec![0usize; num_nodes + 1];
        let mut col_idx = Vec::new();
        let mut weights = Vec::new();

        for (i, &nid) in node_order.iter().enumerate() {
            let neighbors = graph.neighbors(nid, Direction::Both)?;
            for (edge_id, neighbor_id) in &neighbors {
                if let Some(&j) = node_to_idx.get(neighbor_id) {
                    col_idx.push(j);
                    let w = edge_weights.get(edge_id).copied().unwrap_or(1.0);
                    weights.push(w);
                }
            }
            row_ptr[i + 1] = col_idx.len();
        }

        Ok(Self {
            row_ptr,
            col_idx,
            weights,
            num_nodes,
        })
    }

    /// Sparse matrix-dense matrix multiply: A [N x N] * B [N x D] -> C [N x D].
    ///
    /// For each row i: C[i] = sum_j (A[i,j] * B[j])
    pub fn spmm(&self, features: &FeatureMatrix) -> FeatureMatrix {
        let feature_dim = features.feature_dim;
        let mut result = FeatureMatrix::zeros(self.num_nodes, feature_dim);

        for i in 0..self.num_nodes {
            let start = self.row_ptr[i];
            let end = self.row_ptr[i + 1];
            let result_row = result.row_mut(i);

            for idx in start..end {
                let j = self.col_idx[idx];
                let w = self.weights[idx];
                let src_row = features.row(j);
                for k in 0..feature_dim {
                    result_row[k] += w * src_row[k];
                }
            }
        }

        result
    }

    /// Return the degree (number of neighbors) for node i.
    pub fn degree(&self, i: usize) -> usize {
        self.row_ptr[i + 1] - self.row_ptr[i]
    }
}

/// SpMM-based message passing for a single GNN layer.
///
/// Computes: `H' = activation(A * (H * W_neigh) + H * W_self + bias)`
///
/// Where A is the CSR adjacency (with edge weights), H is the feature matrix,
/// and W_neigh/W_self are the layer weight matrices.
pub fn message_passing_spmm(
    csr: &CSRAdjacency,
    features: &FeatureMatrix,
    layer: &GNNLayer,
    mean_normalize: bool,
) -> FeatureMatrix {
    let n = features.num_nodes;
    let hidden_dim = layer.bias.len();

    // Step 1: Transform all features: HW_neigh = H * W_neigh
    let hw_neigh = features.matmul_right(&layer.w_neigh);

    // Step 2: Sparse aggregate: AHW = A * HW_neigh
    let mut ahw = csr.spmm(&hw_neigh);

    // Apply mean normalization if requested.
    if mean_normalize {
        for i in 0..n {
            let deg = csr.degree(i);
            if deg > 1 {
                let scale = 1.0 / deg as f32;
                let row = ahw.row_mut(i);
                for k in 0..hidden_dim {
                    row[k] *= scale;
                }
            }
        }
    }

    // Step 3: Self-transform: H * W_self
    let hw_self = features.matmul_right(&layer.w_self);

    // Step 4: Combine: Z = AHW + H * W_self + bias, then activate
    let mut result = FeatureMatrix::zeros(n, hidden_dim);
    for i in 0..n {
        let r = result.row_mut(i);
        let agg = ahw.row(i);
        let self_proj = hw_self.row(i);
        for k in 0..hidden_dim {
            let z = agg[k] + self_proj[k] + layer.bias.data[k];
            r[k] = match layer.activation {
                Activation::ReLU => z.max(0.0),
                Activation::Sigmoid => 1.0 / (1.0 + (-z).exp()),
                Activation::LeakyReLU => if z > 0.0 { z } else { 0.01 * z },
                Activation::Tanh => z.tanh(),
                Activation::ELU => if z > 0.0 { z } else { 1.0 * (z.exp() - 1.0) },
                Activation::None => z,
            };
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_passing::{Aggregation, MessagePassingConfig};
    use crate::model::{self, GNNModel};
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    fn make_linear_graph() -> (Graph, [NodeId; 3]) {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![1.0, 0.0, 0.5]))
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![0.0, 1.0, 0.3]))
            .unwrap();
        let c = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![0.5, 0.5, 1.0]))
            .unwrap();
        graph
            .create_edge(a, b, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(b, c, "LINK".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        (graph, [a, b, c])
    }

    #[test]
    fn test_csr_from_linear_graph() {
        let (graph, [a, b, c]) = make_linear_graph();
        let node_order = vec![a, b, c];
        let csr = CSRAdjacency::from_graph(&graph, &node_order, &HashMap::new()).unwrap();

        assert_eq!(csr.num_nodes, 3);
        // A has 1 neighbor (B), B has 2 (A, C), C has 1 (B)
        assert_eq!(csr.degree(0), 1); // A
        assert_eq!(csr.degree(1), 2); // B
        assert_eq!(csr.degree(2), 1); // C
    }

    #[test]
    fn test_feature_matrix_roundtrip() {
        let n1 = NodeId(1);
        let n2 = NodeId(2);
        let mut map = HashMap::new();
        map.insert(n1, Tensor::new(vec![1.0, 2.0, 3.0], false));
        map.insert(n2, Tensor::new(vec![4.0, 5.0, 6.0], false));

        let order = vec![n1, n2];
        let fm = FeatureMatrix::from_hashmap(&map, &order, 3);
        assert_eq!(fm.num_nodes, 2);
        assert_eq!(fm.feature_dim, 3);
        assert_eq!(fm.row(0), &[1.0, 2.0, 3.0]);
        assert_eq!(fm.row(1), &[4.0, 5.0, 6.0]);

        let back = fm.to_hashmap(&order);
        assert_eq!(back[&n1].data, vec![1.0, 2.0, 3.0]);
        assert_eq!(back[&n2].data, vec![4.0, 5.0, 6.0]);
    }

    #[test]
    fn test_spmm_matches_hashmap() {
        // Verify that SpMM-based message passing produces the same result as
        // the HashMap-based forward pass (for Sum aggregation, no normalization).
        let (graph, [a, b, c]) = make_linear_graph();
        let node_order = vec![a, b, c];

        let model = GNNModel::new(3, 4, 2, 1, Activation::ReLU);

        let mut features_map = HashMap::new();
        features_map.insert(a, Tensor::new(vec![1.0, 0.0, 0.5], false));
        features_map.insert(b, Tensor::new(vec![0.0, 1.0, 0.3], false));
        features_map.insert(c, Tensor::new(vec![0.5, 0.5, 1.0], false));

        let edge_weights = HashMap::new();

        // HashMap-based forward.
        let mp_config = MessagePassingConfig {
            aggregation: Aggregation::Sum,
            activation: Activation::ReLU,
            normalize: false,
            dropout: 0.0,
        };
        let (hashmap_logits, _) =
            model::forward(&model, &graph, &features_map, &edge_weights, &mp_config).unwrap();

        // SpMM-based forward.
        let fm = FeatureMatrix::from_hashmap(&features_map, &node_order, 3);
        let csr = CSRAdjacency::from_graph(&graph, &node_order, &edge_weights).unwrap();
        let hidden = message_passing_spmm(&csr, &fm, &model.layers[0], false);

        // Apply classification head to SpMM result.
        let spmm_logits_fm = hidden.matmul_right(&model.head.w_out);
        for (i, &nid) in node_order.iter().enumerate() {
            let spmm_row = spmm_logits_fm.row(i);
            let hashmap_logit = &hashmap_logits[&nid];

            // Add bias to spmm result for comparison.
            for k in 0..model.num_classes {
                let spmm_val = spmm_row[k] + model.head.b_out.data[k];
                let hashmap_val = hashmap_logit.data[k];
                assert!(
                    (spmm_val - hashmap_val).abs() < 1e-4,
                    "Mismatch at node {:?} dim {}: spmm={}, hashmap={}",
                    nid,
                    k,
                    spmm_val,
                    hashmap_val
                );
            }
        }
    }
}

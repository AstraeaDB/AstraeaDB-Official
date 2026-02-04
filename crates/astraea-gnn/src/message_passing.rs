use std::collections::HashMap;

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, EdgeId, NodeId};

use crate::tensor::Tensor;

/// Aggregation strategy for combining neighbor messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregation {
    /// Sum all incoming messages.
    Sum,
    /// Average (mean) of all incoming messages.
    Mean,
    /// Element-wise maximum of all incoming messages.
    Max,
}

/// Activation function applied after aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// Rectified Linear Unit: max(0, x).
    ReLU,
    /// Sigmoid: 1 / (1 + exp(-x)).
    Sigmoid,
    /// Identity (no activation).
    None,
}

/// Configuration for a message passing layer.
#[derive(Debug, Clone)]
pub struct MessagePassingConfig {
    pub aggregation: Aggregation,
    pub activation: Activation,
    pub normalize: bool,
}

impl Default for MessagePassingConfig {
    fn default() -> Self {
        Self {
            aggregation: Aggregation::Sum,
            activation: Activation::ReLU,
            normalize: false,
        }
    }
}

/// Perform one round of message passing on the graph.
///
/// For each node, aggregates neighbor features weighted by edge weights:
///
///   message_i = AGG_{j in N(i)} (w_ij * h_j)
///   h_i' = activation(message_i)
///
/// Returns updated node features. Nodes with no neighbors (or no neighbors in
/// `node_features`) retain their original features unchanged.
///
/// # Arguments
///
/// * `graph` - The graph providing topology (neighbor edges).
/// * `node_features` - Current feature vector for each node.
/// * `edge_weights` - Learnable weight for each edge, keyed by EdgeId.
/// * `config` - Aggregation, activation, and normalization settings.
pub fn message_passing(
    graph: &dyn GraphOps,
    node_features: &HashMap<NodeId, Tensor>,
    edge_weights: &HashMap<EdgeId, f32>,
    config: &MessagePassingConfig,
) -> Result<HashMap<NodeId, Tensor>> {
    let mut updated_features: HashMap<NodeId, Tensor> = HashMap::new();

    for (&node_id, current_features) in node_features {
        let feature_dim = current_features.len();

        // Collect all neighbors (both directions) with their edge info.
        let neighbors = graph.neighbors(node_id, Direction::Both)?;

        // Gather weighted messages from neighbors that have features.
        let mut messages: Vec<Tensor> = Vec::new();

        for (edge_id, neighbor_id) in &neighbors {
            // Look up the neighbor's features. Skip if not present.
            let neighbor_features = match node_features.get(neighbor_id) {
                Some(f) => f,
                None => continue,
            };

            // Look up edge weight. Default to 1.0 if missing.
            let weight = edge_weights.get(edge_id).copied().unwrap_or(1.0);

            // Compute weighted message: weight * neighbor_features.
            let message = neighbor_features.scale(weight);
            messages.push(message);
        }

        // If no messages were received, keep original features.
        if messages.is_empty() {
            updated_features.insert(node_id, current_features.clone());
            continue;
        }

        // Aggregate messages according to the configured strategy.
        let aggregated = match config.aggregation {
            Aggregation::Sum => {
                let mut acc = Tensor::zeros(feature_dim, false);
                for msg in &messages {
                    acc = acc.add(msg);
                }
                acc
            }
            Aggregation::Mean => {
                let mut acc = Tensor::zeros(feature_dim, false);
                for msg in &messages {
                    acc = acc.add(msg);
                }
                let count = messages.len() as f32;
                acc.scale(1.0 / count)
            }
            Aggregation::Max => {
                // Element-wise max across all messages.
                let mut max_data = vec![f32::NEG_INFINITY; feature_dim];
                for msg in &messages {
                    for (i, val) in msg.data.iter().enumerate() {
                        if *val > max_data[i] {
                            max_data[i] = *val;
                        }
                    }
                }
                Tensor::new(max_data, false)
            }
        };

        // Apply activation function.
        let activated = match config.activation {
            Activation::ReLU => aggregated.relu(),
            Activation::Sigmoid => aggregated.sigmoid(),
            Activation::None => aggregated,
        };

        // Optionally L2-normalize.
        let result = if config.normalize {
            let n = activated.norm();
            if n > 1e-12 {
                activated.scale(1.0 / n)
            } else {
                activated
            }
        } else {
            activated
        };

        updated_features.insert(node_id, result);
    }

    Ok(updated_features)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::traits::GraphOps;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    /// Helper: build a 3-node linear graph: A --e1--> B --e2--> C.
    /// Returns (graph, node_ids, edge_ids).
    fn make_linear_graph() -> (Graph, [NodeId; 3], [EdgeId; 2]) {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));

        let a = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![1.0, 0.0]))
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![0.0, 1.0]))
            .unwrap();
        let c = graph
            .create_node(vec![], serde_json::json!({}), Some(vec![1.0, 1.0]))
            .unwrap();

        let e1 = graph
            .create_edge(
                a,
                b,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        let e2 = graph
            .create_edge(
                b,
                c,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        (graph, [a, b, c], [e1, e2])
    }

    #[test]
    fn test_message_passing_sum() {
        let (graph, nodes, edges) = make_linear_graph();
        let [a, b, c] = nodes;
        let [e1, e2] = edges;

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![1.0, 0.0], false));
        features.insert(b, Tensor::new(vec![0.0, 1.0], false));
        features.insert(c, Tensor::new(vec![1.0, 1.0], false));

        let mut weights = HashMap::new();
        weights.insert(e1, 1.0f32);
        weights.insert(e2, 1.0f32);

        let config = MessagePassingConfig {
            aggregation: Aggregation::Sum,
            activation: Activation::None,
            normalize: false,
        };

        let result = message_passing(&graph, &features, &weights, &config).unwrap();

        // Node A: neighbors via edge e1 -> B. Message = 1.0 * [0,1] = [0,1].
        let a_feat = &result[&a];
        assert!((a_feat.data[0] - 0.0).abs() < 1e-6);
        assert!((a_feat.data[1] - 1.0).abs() < 1e-6);

        // Node B: neighbors via e1 -> A, and via e2 -> C. Sum = 1.0*[1,0] + 1.0*[1,1] = [2,1].
        let b_feat = &result[&b];
        assert!((b_feat.data[0] - 2.0).abs() < 1e-6);
        assert!((b_feat.data[1] - 1.0).abs() < 1e-6);

        // Node C: neighbors via edge e2 -> B. Message = 1.0 * [0,1] = [0,1].
        let c_feat = &result[&c];
        assert!((c_feat.data[0] - 0.0).abs() < 1e-6);
        assert!((c_feat.data[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_message_passing_mean() {
        let (graph, nodes, edges) = make_linear_graph();
        let [a, b, c] = nodes;
        let [e1, e2] = edges;

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![1.0, 0.0], false));
        features.insert(b, Tensor::new(vec![0.0, 1.0], false));
        features.insert(c, Tensor::new(vec![1.0, 1.0], false));

        let mut weights = HashMap::new();
        weights.insert(e1, 1.0f32);
        weights.insert(e2, 1.0f32);

        let config = MessagePassingConfig {
            aggregation: Aggregation::Mean,
            activation: Activation::None,
            normalize: false,
        };

        let result = message_passing(&graph, &features, &weights, &config).unwrap();

        // Node B: neighbors A and C. Mean = ([1,0] + [1,1]) / 2 = [1.0, 0.5].
        let b_feat = &result[&b];
        assert!((b_feat.data[0] - 1.0).abs() < 1e-6);
        assert!((b_feat.data[1] - 0.5).abs() < 1e-6);

        // Nodes A and C each have 1 neighbor, so mean = same as sum.
        let a_feat = &result[&a];
        assert!((a_feat.data[0] - 0.0).abs() < 1e-6);
        assert!((a_feat.data[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_message_passing_with_relu() {
        let (graph, nodes, edges) = make_linear_graph();
        let [a, b, _c] = nodes;
        let [e1, e2] = edges;

        let mut features = HashMap::new();
        // Use negative features to test ReLU clamping.
        features.insert(a, Tensor::new(vec![-1.0, 2.0], false));
        features.insert(b, Tensor::new(vec![3.0, -4.0], false));
        features.insert(_c, Tensor::new(vec![-2.0, 1.0], false));

        let mut weights = HashMap::new();
        weights.insert(e1, 1.0f32);
        weights.insert(e2, 1.0f32);

        let config = MessagePassingConfig {
            aggregation: Aggregation::Sum,
            activation: Activation::ReLU,
            normalize: false,
        };

        let result = message_passing(&graph, &features, &weights, &config).unwrap();

        // Node A: message from B = [3, -4]. After ReLU: [3, 0].
        let a_feat = &result[&a];
        assert!((a_feat.data[0] - 3.0).abs() < 1e-6);
        assert!((a_feat.data[1] - 0.0).abs() < 1e-6);

        // Node B: sum of A and C = [-1,2] + [-2,1] = [-3, 3]. After ReLU: [0, 3].
        let b_feat = &result[&b];
        assert!((b_feat.data[0] - 0.0).abs() < 1e-6);
        assert!((b_feat.data[1] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_message_passing_normalize() {
        let (graph, nodes, edges) = make_linear_graph();
        let [a, b, c] = nodes;
        let [e1, e2] = edges;

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![3.0, 4.0], false));
        features.insert(b, Tensor::new(vec![0.0, 1.0], false));
        features.insert(c, Tensor::new(vec![1.0, 0.0], false));

        let mut weights = HashMap::new();
        weights.insert(e1, 1.0f32);
        weights.insert(e2, 1.0f32);

        let config = MessagePassingConfig {
            aggregation: Aggregation::Sum,
            activation: Activation::None,
            normalize: true,
        };

        let result = message_passing(&graph, &features, &weights, &config).unwrap();

        // All output vectors should have L2 norm close to 1.0.
        for (_, feat) in &result {
            let n = feat.norm();
            assert!(
                (n - 1.0).abs() < 1e-5,
                "expected norm ~1.0, got {}",
                n
            );
        }
    }

    #[test]
    fn test_message_passing_with_edge_weights() {
        let (graph, nodes, edges) = make_linear_graph();
        let [a, b, c] = nodes;
        let [e1, e2] = edges;

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![1.0, 0.0], false));
        features.insert(b, Tensor::new(vec![0.0, 1.0], false));
        features.insert(c, Tensor::new(vec![1.0, 1.0], false));

        // Different edge weights.
        let mut weights = HashMap::new();
        weights.insert(e1, 2.0f32);
        weights.insert(e2, 0.5f32);

        let config = MessagePassingConfig {
            aggregation: Aggregation::Sum,
            activation: Activation::None,
            normalize: false,
        };

        let result = message_passing(&graph, &features, &weights, &config).unwrap();

        // Node B: neighbors A (via e1, weight=2.0) and C (via e2, weight=0.5).
        // Message from A: 2.0 * [1,0] = [2, 0].
        // Message from C: 0.5 * [1,1] = [0.5, 0.5].
        // Sum: [2.5, 0.5].
        let b_feat = &result[&b];
        assert!((b_feat.data[0] - 2.5).abs() < 1e-6);
        assert!((b_feat.data[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_message_passing_isolated_node_keeps_features() {
        // Node with no neighbors should keep its original features.
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![5.0, 10.0], false));

        let config = MessagePassingConfig {
            aggregation: Aggregation::Sum,
            activation: Activation::ReLU,
            normalize: false,
        };

        let result = message_passing(&graph, &features, &HashMap::new(), &config).unwrap();

        let a_feat = &result[&a];
        assert_eq!(a_feat.data, vec![5.0, 10.0]);
    }
}

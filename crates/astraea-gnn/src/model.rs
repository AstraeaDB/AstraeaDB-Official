use std::collections::HashMap;

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, EdgeId, NodeId};
use rand::Rng;

use crate::message_passing::{Activation, Aggregation, MessagePassingConfig};
use crate::tensor::{Matrix, Tensor};

/// A single GNN message-passing layer with learnable weight matrices.
///
/// Computes: `h_i' = activation(W_self * h_i + AGG_j(w_ij * W_neigh * h_j) + bias)`
///
/// This adds learned linear transformations to the message passing, enabling the
/// model to change feature dimensionality between layers and learn which feature
/// combinations are important for classification.
#[derive(Debug, Clone)]
pub struct GNNLayer {
    /// Projects neighbor features: [output_dim x input_dim].
    pub w_neigh: Matrix,
    /// Projects self features (skip connection): [output_dim x input_dim].
    pub w_self: Matrix,
    /// Bias vector: [output_dim].
    pub bias: Tensor,
    /// Activation function applied after aggregation.
    pub activation: Activation,
}

/// Classification head that maps final hidden features to class logits.
///
/// Computes: `logits = W_out * h + b_out`
#[derive(Debug, Clone)]
pub struct ClassificationHead {
    /// Weight matrix: [num_classes x hidden_dim].
    pub w_out: Matrix,
    /// Bias vector: [num_classes].
    pub b_out: Tensor,
}

/// A complete GNN model with multiple message-passing layers and a classification head.
///
/// The model decouples input feature dimension from the number of output classes
/// via learnable weight matrices. Input features of any dimension are projected
/// through hidden layers and then to class logits.
#[derive(Debug, Clone)]
pub struct GNNModel {
    pub layers: Vec<GNNLayer>,
    pub head: ClassificationHead,
    pub input_dim: usize,
    pub hidden_dim: usize,
    pub num_classes: usize,
}

impl GNNModel {
    /// Create a new model with Xavier-initialized weights.
    ///
    /// # Arguments
    /// * `input_dim` - Dimension of input node features (e.g., 165 for Elliptic).
    /// * `hidden_dim` - Hidden representation size (e.g., 64, 128).
    /// * `num_classes` - Number of output classes.
    /// * `num_layers` - Number of message-passing layers (typically 1-3).
    /// * `activation` - Activation function for hidden layers.
    pub fn new(
        input_dim: usize,
        hidden_dim: usize,
        num_classes: usize,
        num_layers: usize,
        activation: Activation,
    ) -> Self {
        let mut rng = rand::thread_rng();
        Self::new_with_rng(input_dim, hidden_dim, num_classes, num_layers, activation, &mut rng)
    }

    /// Create a new model with Xavier-initialized weights using a provided RNG.
    pub fn new_with_rng(
        input_dim: usize,
        hidden_dim: usize,
        num_classes: usize,
        num_layers: usize,
        activation: Activation,
        rng: &mut impl Rng,
    ) -> Self {
        let mut layers = Vec::with_capacity(num_layers);

        for i in 0..num_layers {
            let layer_input = if i == 0 { input_dim } else { hidden_dim };
            // Matrices are [output_dim x input_dim] so matvec(input) -> output.
            layers.push(GNNLayer {
                w_neigh: Matrix::random_xavier(hidden_dim, layer_input, rng),
                w_self: Matrix::random_xavier(hidden_dim, layer_input, rng),
                bias: Tensor::zeros(hidden_dim, false),
                activation,
            });
        }

        let head_input = if num_layers == 0 { input_dim } else { hidden_dim };
        let head = ClassificationHead {
            w_out: Matrix::random_xavier(num_classes, head_input, rng),
            b_out: Tensor::zeros(num_classes, false),
        };

        Self {
            layers,
            head,
            input_dim,
            hidden_dim,
            num_classes,
        }
    }
}

/// Cached intermediate values from the forward pass, needed for backpropagation.
#[derive(Debug, Clone)]
pub struct ForwardCache {
    /// The initial node features passed to the forward pass.
    pub initial_features: HashMap<NodeId, Tensor>,
    /// Per-layer: input features for each node before this layer's transform.
    pub layer_inputs: Vec<HashMap<NodeId, Tensor>>,
    /// Per-layer: pre-activation values for each node (before activation function).
    pub pre_activations: Vec<HashMap<NodeId, Tensor>>,
    /// Per-layer: the aggregated (weighted, transformed) neighbor messages per node,
    /// before adding the self-transform. Needed for W_neigh gradient computation.
    pub neighbor_aggs: Vec<HashMap<NodeId, Tensor>>,
    /// Final logits (output of classification head, before softmax).
    pub logits: HashMap<NodeId, Tensor>,
}

/// Run the full forward pass of a GNNModel, returning logits and cached intermediates.
///
/// For each layer:
///   `h_i' = activation(W_self * h_i + AGG_j(w_ij * W_neigh * h_j) + bias)`
///
/// Then: `logits_i = W_out * h_final_i + b_out`
pub fn forward(
    model: &GNNModel,
    graph: &dyn GraphOps,
    initial_features: &HashMap<NodeId, Tensor>,
    edge_weights: &HashMap<EdgeId, f32>,
    mp_config: &MessagePassingConfig,
) -> Result<(HashMap<NodeId, Tensor>, ForwardCache)> {
    let mut current_features = initial_features.clone();
    let mut cache = ForwardCache {
        initial_features: initial_features.clone(),
        layer_inputs: Vec::with_capacity(model.layers.len()),
        pre_activations: Vec::with_capacity(model.layers.len()),
        neighbor_aggs: Vec::with_capacity(model.layers.len()),
        logits: HashMap::new(),
    };

    for layer in &model.layers {
        // Save input features for this layer (needed for backward).
        cache.layer_inputs.push(current_features.clone());

        let mut new_features = HashMap::new();
        let mut pre_acts = HashMap::new();
        let mut neigh_aggs = HashMap::new();

        for (&node_id, node_feat) in &current_features {
            // Self-transform: W_self * h_i
            let self_proj = layer.w_self.matvec(node_feat);

            // Collect and aggregate neighbor messages.
            let neighbors = graph.neighbors(node_id, Direction::Both)?;
            let output_dim = layer.bias.len();
            let mut agg = Tensor::zeros(output_dim, false);
            let mut msg_count = 0usize;

            for (edge_id, neighbor_id) in &neighbors {
                if let Some(neighbor_feat) = current_features.get(neighbor_id) {
                    let weight = edge_weights.get(edge_id).copied().unwrap_or(1.0);
                    // W_neigh * h_j, then scale by edge weight.
                    let transformed = layer.w_neigh.matvec(neighbor_feat);
                    let weighted = transformed.scale(weight);
                    agg = agg.add(&weighted);
                    msg_count += 1;
                }
            }

            // Apply mean normalization if configured.
            if mp_config.aggregation == Aggregation::Mean && msg_count > 1 {
                agg = agg.scale(1.0 / msg_count as f32);
            }

            neigh_aggs.insert(node_id, agg.clone());

            // Pre-activation: W_self * h_i + agg + bias
            let pre_act = self_proj.add(&agg).add(&layer.bias);
            pre_acts.insert(node_id, pre_act.clone());

            // Apply activation.
            let activated = match layer.activation {
                Activation::ReLU => pre_act.relu(),
                Activation::Sigmoid => pre_act.sigmoid(),
                Activation::LeakyReLU => pre_act.leaky_relu(),
                Activation::Tanh => pre_act.tanh_act(),
                Activation::ELU => pre_act.elu(1.0),
                Activation::None => pre_act,
            };

            // Optional L2 normalization.
            let result = if mp_config.normalize {
                let n = activated.norm();
                if n > 1e-12 {
                    activated.scale(1.0 / n)
                } else {
                    activated
                }
            } else {
                activated
            };

            new_features.insert(node_id, result);
        }

        cache.pre_activations.push(pre_acts);
        cache.neighbor_aggs.push(neigh_aggs);
        current_features = new_features;
    }

    // Classification head: logits = W_out * h_final + b_out
    let mut logits = HashMap::new();
    for (&node_id, feat) in &current_features {
        let logit = model.head.w_out.matvec(feat).add(&model.head.b_out);
        logits.insert(node_id, logit);
    }
    cache.logits = logits.clone();

    Ok((logits, cache))
}

/// Compute predictions from logits (argmax over class dimension).
pub fn predict_from_logits(
    logits: &HashMap<NodeId, Tensor>,
    labeled_nodes: &[NodeId],
    num_classes: usize,
) -> HashMap<NodeId, usize> {
    let mut predictions = HashMap::new();
    for &node_id in labeled_nodes {
        if let Some(logit) = logits.get(&node_id) {
            if logit.is_empty() {
                predictions.insert(node_id, 0);
                continue;
            }
            let mut best_idx = 0;
            let mut best_val = f32::NEG_INFINITY;
            for (i, &val) in logit.data.iter().enumerate() {
                if val > best_val {
                    best_val = val;
                    best_idx = i;
                }
            }
            predictions.insert(node_id, best_idx % num_classes);
        }
    }
    predictions
}

/// Compute cross-entropy loss from logits.
///
/// loss = mean(-log(softmax(logits)[true_class])) over labeled nodes.
pub fn compute_loss_from_logits(
    logits: &HashMap<NodeId, Tensor>,
    labels: &HashMap<NodeId, usize>,
    num_classes: usize,
) -> f32 {
    if labels.is_empty() {
        return 0.0;
    }

    let mut total_loss = 0.0;
    let mut count = 0;

    for (&node_id, &true_class) in labels {
        if let Some(logit) = logits.get(&node_id) {
            if logit.is_empty() {
                continue;
            }
            let logit_len = logit.data.len().min(num_classes);
            let logits_slice = &logit.data[..logit_len];

            let max_logit = logits_slice.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let exp_logits: Vec<f32> = logits_slice.iter().map(|x| (x - max_logit).exp()).collect();
            let sum_exp: f32 = exp_logits.iter().sum();

            let target_idx = true_class % logit_len;
            let prob = exp_logits[target_idx] / sum_exp;
            total_loss += -(prob.max(1e-12)).ln();
            count += 1;
        }
    }

    if count > 0 {
        total_loss / count as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_passing::MessagePassingConfig;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    fn make_test_graph() -> (Graph, NodeId, NodeId, NodeId, NodeId) {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(
                vec!["Class0".into()],
                serde_json::json!({}),
                Some(vec![1.0, 0.0, 0.5]),
            )
            .unwrap();
        let b = graph
            .create_node(
                vec!["Class0".into()],
                serde_json::json!({}),
                Some(vec![0.8, 0.2, 0.4]),
            )
            .unwrap();
        let c = graph
            .create_node(
                vec!["Class1".into()],
                serde_json::json!({}),
                Some(vec![0.2, 0.8, 0.6]),
            )
            .unwrap();
        let d = graph
            .create_node(
                vec!["Class1".into()],
                serde_json::json!({}),
                Some(vec![0.0, 1.0, 0.7]),
            )
            .unwrap();
        graph
            .create_edge(a, b, "SIMILAR".into(), serde_json::json!({}), 2.0, None, None)
            .unwrap();
        graph
            .create_edge(c, d, "SIMILAR".into(), serde_json::json!({}), 2.0, None, None)
            .unwrap();
        graph
            .create_edge(b, c, "BRIDGE".into(), serde_json::json!({}), 0.1, None, None)
            .unwrap();
        (graph, a, b, c, d)
    }

    #[test]
    fn test_gnn_model_creation() {
        let model = GNNModel::new(165, 64, 2, 2, Activation::ReLU);
        assert_eq!(model.layers.len(), 2);
        // Matrices are [output_dim x input_dim] for matvec.
        assert_eq!(model.layers[0].w_neigh.rows, 64);
        assert_eq!(model.layers[0].w_neigh.cols, 165);
        assert_eq!(model.layers[0].w_self.rows, 64);
        assert_eq!(model.layers[0].w_self.cols, 165);
        assert_eq!(model.layers[0].bias.len(), 64);
        // Second layer: [hidden x hidden].
        assert_eq!(model.layers[1].w_neigh.rows, 64);
        assert_eq!(model.layers[1].w_neigh.cols, 64);
        assert_eq!(model.head.w_out.rows, 2);
        assert_eq!(model.head.w_out.cols, 64);
        assert_eq!(model.head.b_out.len(), 2);
    }

    #[test]
    fn test_gnn_forward_produces_correct_dims() {
        let (graph, a, b, c, d) = make_test_graph();
        let model = GNNModel::new(3, 8, 2, 1, Activation::ReLU);

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![1.0, 0.0, 0.5], false));
        features.insert(b, Tensor::new(vec![0.8, 0.2, 0.4], false));
        features.insert(c, Tensor::new(vec![0.2, 0.8, 0.6], false));
        features.insert(d, Tensor::new(vec![0.0, 1.0, 0.7], false));

        let mp_config = MessagePassingConfig::default();
        let edge_weights = HashMap::new(); // will use default 1.0

        let (logits, cache) = forward(&model, &graph, &features, &edge_weights, &mp_config).unwrap();

        // Each node should have 2 logits (num_classes = 2).
        for &node in &[a, b, c, d] {
            let logit = &logits[&node];
            assert_eq!(logit.len(), 2, "Expected 2 logits for node {:?}", node);
        }

        // Cache should have 1 layer of intermediates.
        assert_eq!(cache.layer_inputs.len(), 1);
        assert_eq!(cache.pre_activations.len(), 1);
        assert_eq!(cache.neighbor_aggs.len(), 1);
        assert_eq!(cache.logits.len(), 4);
    }

    #[test]
    fn test_loss_from_logits() {
        let mut logits = HashMap::new();
        let node = NodeId(1);
        // logits: [2.0, 1.0]. True class = 0.
        logits.insert(node, Tensor::new(vec![2.0, 1.0], false));

        let mut labels = HashMap::new();
        labels.insert(node, 0);

        let loss = compute_loss_from_logits(&logits, &labels, 2);
        let expected = -(2.0f32.exp() / (2.0f32.exp() + 1.0f32.exp())).ln();
        assert!(
            (loss - expected).abs() < 1e-4,
            "expected loss ~{}, got {}",
            expected,
            loss
        );
    }

    #[test]
    fn test_predict_from_logits() {
        let mut logits = HashMap::new();
        let n1 = NodeId(1);
        let n2 = NodeId(2);
        logits.insert(n1, Tensor::new(vec![0.1, 0.9], false));
        logits.insert(n2, Tensor::new(vec![0.8, 0.2], false));

        let preds = predict_from_logits(&logits, &[n1, n2], 2);
        assert_eq!(preds[&n1], 1);
        assert_eq!(preds[&n2], 0);
    }
}

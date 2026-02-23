use std::collections::HashMap;

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, EdgeId, NodeId};

use crate::message_passing::{Activation, Aggregation, MessagePassingConfig};
use crate::model::{ForwardCache, GNNModel};
use crate::tensor::{Matrix, Tensor};

/// Accumulated gradients for all model parameters.
#[derive(Debug, Clone)]
pub struct Gradients {
    /// Per-layer gradients for neighbor weight matrices.
    pub d_w_neigh: Vec<Matrix>,
    /// Per-layer gradients for self weight matrices.
    pub d_w_self: Vec<Matrix>,
    /// Per-layer gradients for bias vectors.
    pub d_bias: Vec<Tensor>,
    /// Gradients for edge weights.
    pub d_edge_weights: HashMap<EdgeId, f32>,
    /// Classification head weight gradient.
    pub d_w_out: Matrix,
    /// Classification head bias gradient.
    pub d_b_out: Tensor,
}

/// Compute softmax probabilities from logits (numerically stable).
fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = logits.iter().map(|x| (x - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    exp.iter().map(|e| e / sum).collect()
}

/// Compute the derivative of an activation function given the pre-activation values.
///
/// Returns element-wise: d_activation/d_pre_act * upstream_grad.
fn activation_backward(
    upstream: &Tensor,
    pre_act: &Tensor,
    activation: Activation,
) -> Tensor {
    match activation {
        Activation::ReLU => {
            let data: Vec<f32> = upstream
                .data
                .iter()
                .zip(pre_act.data.iter())
                .map(|(u, &z)| if z > 0.0 { *u } else { 0.0 })
                .collect();
            Tensor::new(data, false)
        }
        Activation::Sigmoid => {
            // sigmoid'(z) = sigmoid(z) * (1 - sigmoid(z))
            let data: Vec<f32> = upstream
                .data
                .iter()
                .zip(pre_act.data.iter())
                .map(|(u, &z)| {
                    let s = 1.0 / (1.0 + (-z).exp());
                    u * s * (1.0 - s)
                })
                .collect();
            Tensor::new(data, false)
        }
        Activation::LeakyReLU => {
            let data: Vec<f32> = upstream
                .data
                .iter()
                .zip(pre_act.data.iter())
                .map(|(u, &z)| if z > 0.0 { *u } else { 0.01 * u })
                .collect();
            Tensor::new(data, false)
        }
        Activation::Tanh => {
            // tanh'(z) = 1 - tanh(z)^2
            let data: Vec<f32> = upstream
                .data
                .iter()
                .zip(pre_act.data.iter())
                .map(|(u, &z)| {
                    let t = z.tanh();
                    u * (1.0 - t * t)
                })
                .collect();
            Tensor::new(data, false)
        }
        Activation::ELU => {
            // ELU'(z) = 1 if z > 0, alpha * exp(z) otherwise
            let alpha = 1.0f32;
            let data: Vec<f32> = upstream
                .data
                .iter()
                .zip(pre_act.data.iter())
                .map(|(u, &z)| if z > 0.0 { *u } else { u * alpha * z.exp() })
                .collect();
            Tensor::new(data, false)
        }
        Activation::None => upstream.clone(),
    }
}

/// Compute analytical gradients via backpropagation through the GNN model.
///
/// This replaces the O(E) finite-difference gradient computation with a single
/// backward pass that is exact to machine precision.
///
/// # Algorithm
///
/// 1. Softmax cross-entropy gradient: `dL/d_logit_k = softmax_k - 1{k == target}`
/// 2. Classification head backward: compute `dL/dW_out`, `dL/db_out`, `dL/dh_final`
/// 3. Per-layer backward (reverse order):
///    - Backprop through activation using saved pre-activations
///    - Compute `dL/dW_self`, `dL/dW_neigh`, `dL/dbias`, `dL/d_edge_weights`
///    - Propagate `dL/dh` to previous layer
pub fn backward(
    model: &GNNModel,
    cache: &ForwardCache,
    labels: &HashMap<NodeId, usize>,
    num_classes: usize,
    graph: &dyn GraphOps,
    edge_weights: &HashMap<EdgeId, f32>,
    mp_config: &MessagePassingConfig,
) -> Result<Gradients> {
    let num_labeled = labels.len() as f32;

    // Initialize gradient accumulators.
    let num_layers = model.layers.len();
    let mut d_w_neigh: Vec<Matrix> = model
        .layers
        .iter()
        .map(|l| Matrix::zeros(l.w_neigh.rows, l.w_neigh.cols))
        .collect();
    let mut d_w_self: Vec<Matrix> = model
        .layers
        .iter()
        .map(|l| Matrix::zeros(l.w_self.rows, l.w_self.cols))
        .collect();
    let mut d_bias: Vec<Tensor> = model
        .layers
        .iter()
        .map(|l| Tensor::zeros(l.bias.len(), false))
        .collect();
    let mut d_edge_weights: HashMap<EdgeId, f32> = HashMap::new();
    let mut d_w_out = Matrix::zeros(model.head.w_out.rows, model.head.w_out.cols);
    let mut d_b_out = Tensor::zeros(model.head.b_out.len(), false);

    // Step 1: Compute dL/d_logits from softmax cross-entropy.
    // dL/d_logit_k = (softmax_k - 1{k == target}) / num_labeled
    let mut d_logits: HashMap<NodeId, Tensor> = HashMap::new();
    for (&node_id, &true_class) in labels {
        if let Some(logit) = cache.logits.get(&node_id) {
            let probs = softmax(&logit.data);
            let mut d = probs.clone();
            let target_idx = true_class % num_classes;
            d[target_idx] -= 1.0;
            // Average over labeled nodes.
            let d: Vec<f32> = d.iter().map(|x| x / num_labeled).collect();
            d_logits.insert(node_id, Tensor::new(d, false));
        }
    }

    // Step 2: Classification head backward.
    // logits = W_out * h_final + b_out
    // dL/dW_out = sum_i(dL/d_logits_i * h_final_i^T)
    // dL/db_out = sum_i(dL/d_logits_i)
    // dL/dh_final_i = W_out^T * dL/d_logits_i
    // Collect all node IDs that have logits.
    let all_nodes: Vec<NodeId> = cache.logits.keys().copied().collect();

    // We need the final hidden features (post-activation of last layer).
    // These can be computed from W_out^-1 * (logits - b_out), but it's simpler
    // to recompute from the cache.
    // The final features are: activation(pre_activations[last_layer])
    let final_hidden: HashMap<NodeId, Tensor> = if num_layers > 0 {
        let last = num_layers - 1;
        cache.pre_activations[last]
            .iter()
            .map(|(&nid, pre_act)| {
                let activated = match model.layers[last].activation {
                    Activation::ReLU => pre_act.relu(),
                    Activation::Sigmoid => pre_act.sigmoid(),
                    Activation::LeakyReLU => pre_act.leaky_relu(),
                    Activation::Tanh => pre_act.tanh_act(),
                    Activation::ELU => pre_act.elu(1.0),
                    Activation::None => pre_act.clone(),
                };
                (nid, activated)
            })
            .collect()
    } else {
        // No layers: final features = initial features.
        cache.initial_features.clone()
    };

    // Accumulate classification head gradients and compute dL/dh_final.
    let mut d_h: HashMap<NodeId, Tensor> = HashMap::new();
    for (&node_id, d_logit) in &d_logits {
        // dW_out += outer(d_logit, h_final)
        if let Some(h_final) = final_hidden.get(&node_id) {
            let outer = Matrix::outer(d_logit, h_final);
            d_w_out = d_w_out.add(&outer);
        }
        // db_out += d_logit
        d_b_out = d_b_out.add(d_logit);
        // dh_final = W_out^T * d_logit
        let dh = model.head.w_out.transpose_matvec(d_logit);
        d_h.insert(node_id, dh);
    }

    // For nodes that are NOT labeled, they still participate in message passing
    // and receive gradients from labeled nodes in their neighborhood.
    // Initialize their dh to zero.
    for &node_id in &all_nodes {
        d_h.entry(node_id).or_insert_with(|| {
            let dim = if num_layers > 0 {
                model.layers[num_layers - 1].bias.len()
            } else {
                model.input_dim
            };
            Tensor::zeros(dim, false)
        });
    }

    // Step 3: Per-layer backward (reverse order).
    for layer_idx in (0..num_layers).rev() {
        let layer = &model.layers[layer_idx];
        let layer_input = &cache.layer_inputs[layer_idx];
        let pre_acts = &cache.pre_activations[layer_idx];

        // Backprop through activation.
        let mut d_pre_act: HashMap<NodeId, Tensor> = HashMap::new();
        for (&node_id, dh) in &d_h {
            if let Some(pre_act) = pre_acts.get(&node_id) {
                let d_act = activation_backward(dh, pre_act, layer.activation);
                d_pre_act.insert(node_id, d_act);
            }
        }

        // Initialize gradients for the previous layer.
        let input_dim = layer.w_self.cols; // input dimension for this layer
        let mut d_h_prev: HashMap<NodeId, Tensor> = HashMap::new();
        for &node_id in &all_nodes {
            d_h_prev.insert(node_id, Tensor::zeros(input_dim, false));
        }

        // Accumulate weight gradients.
        for (&node_id, d_pa) in &d_pre_act {
            // Bias gradient: d_bias += d_pre_act
            d_bias[layer_idx] = d_bias[layer_idx].add(d_pa);

            // Self-transform gradient: d_w_self += outer(d_pre_act, h_input)
            if let Some(h_input) = layer_input.get(&node_id) {
                let outer = Matrix::outer(d_pa, h_input);
                d_w_self[layer_idx] = d_w_self[layer_idx].add(&outer);

                // Propagate to previous layer via self-connection:
                // d_h_prev[i] += W_self^T * d_pre_act[i]
                let d_input = layer.w_self.transpose_matvec(d_pa);
                if let Some(existing) = d_h_prev.get(&node_id) {
                    d_h_prev.insert(node_id, existing.add(&d_input));
                }
            }

            // Neighbor-transform gradient.
            let neighbors = graph.neighbors(node_id, Direction::Both)?;
            // Count only neighbors that have features (matching forward pass behavior).
            let msg_count = neighbors.iter()
                .filter(|(_, nid)| layer_input.contains_key(nid))
                .count();
            for (edge_id, neighbor_id) in &neighbors {
                if let Some(h_j) = layer_input.get(neighbor_id) {
                    let weight = edge_weights.get(edge_id).copied().unwrap_or(1.0);

                    // Scale d_pre_act by edge weight (and mean normalization if applicable).
                    let scale = if mp_config.aggregation == Aggregation::Mean && msg_count > 1 {
                        weight / msg_count as f32
                    } else {
                        weight
                    };

                    let scaled_d_pa = d_pa.scale(scale);

                    // d_w_neigh += outer(scaled_d_pre_act, h_j)
                    let outer = Matrix::outer(&scaled_d_pa, h_j);
                    d_w_neigh[layer_idx] = d_w_neigh[layer_idx].add(&outer);

                    // d_edge_weight[e] += d_pre_act^T * (W_neigh * h_j)
                    let transformed = layer.w_neigh.matvec(h_j);
                    let mean_scale = if mp_config.aggregation == Aggregation::Mean && msg_count > 1 {
                        1.0 / msg_count as f32
                    } else {
                        1.0
                    };
                    let d_w = d_pa.dot(&transformed) * mean_scale;
                    *d_edge_weights.entry(*edge_id).or_insert(0.0) += d_w;

                    // Propagate gradient to neighbor's previous-layer features:
                    // d_h_prev[j] += scale * W_neigh^T * d_pre_act[i]
                    let d_neighbor = layer.w_neigh.transpose_matvec(&scaled_d_pa);
                    if let Some(existing) = d_h_prev.get(neighbor_id) {
                        d_h_prev.insert(*neighbor_id, existing.add(&d_neighbor));
                    }
                }
            }
        }

        d_h = d_h_prev;
    }

    Ok(Gradients {
        d_w_neigh,
        d_w_self,
        d_bias,
        d_edge_weights,
        d_w_out,
        d_b_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_passing::MessagePassingConfig;
    use crate::model::{self, GNNModel};
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
    fn test_backward_gradient_shapes() {
        let (graph, a, b, c, d) = make_test_graph();
        let gnn_model = GNNModel::new(3, 8, 2, 1, Activation::ReLU);

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![1.0, 0.0, 0.5], false));
        features.insert(b, Tensor::new(vec![0.8, 0.2, 0.4], false));
        features.insert(c, Tensor::new(vec![0.2, 0.8, 0.6], false));
        features.insert(d, Tensor::new(vec![0.0, 1.0, 0.7], false));

        let mp_config = MessagePassingConfig::default();
        let edge_weights = HashMap::new();

        let (_logits, cache) =
            model::forward(&gnn_model, &graph, &features, &edge_weights, &mp_config).unwrap();

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        let grads = backward(
            &gnn_model,
            &cache,
            &labels,
            2,
            &graph,
            &edge_weights,
            &mp_config,
        )
        .unwrap();

        // Check gradient shapes match parameter shapes.
        assert_eq!(grads.d_w_neigh.len(), 1);
        assert_eq!(grads.d_w_neigh[0].rows, gnn_model.layers[0].w_neigh.rows);
        assert_eq!(grads.d_w_neigh[0].cols, gnn_model.layers[0].w_neigh.cols);
        assert_eq!(grads.d_w_self[0].rows, gnn_model.layers[0].w_self.rows);
        assert_eq!(grads.d_w_self[0].cols, gnn_model.layers[0].w_self.cols);
        assert_eq!(grads.d_bias[0].len(), gnn_model.layers[0].bias.len());
        assert_eq!(grads.d_w_out.rows, gnn_model.head.w_out.rows);
        assert_eq!(grads.d_w_out.cols, gnn_model.head.w_out.cols);
        assert_eq!(grads.d_b_out.len(), gnn_model.head.b_out.len());

        // All gradients should be finite.
        for val in &grads.d_w_neigh[0].data {
            assert!(val.is_finite(), "d_w_neigh contains non-finite value");
        }
        for val in &grads.d_w_out.data {
            assert!(val.is_finite(), "d_w_out contains non-finite value");
        }
    }

    #[test]
    fn test_backward_vs_numerical() {
        // Compare analytical gradients to finite-difference gradients.
        // Start with a 0-layer model (just classification head) to verify
        // the head gradient, then test a 1-layer model.
        let (graph, a, b, c, d) = make_test_graph();

        use crate::model::ClassificationHead;
        use crate::tensor::Matrix;

        // Build a 0-layer model (just classification head, input_dim=3, num_classes=2).
        let gnn_model = {
            let w_out_data: Vec<f32> = vec![0.1, -0.2, 0.3, -0.1, 0.2, -0.3];
            let head = ClassificationHead {
                w_out: Matrix { data: w_out_data, rows: 2, cols: 3 },
                b_out: Tensor::new(vec![0.05, -0.05], false),
            };
            GNNModel {
                layers: vec![],
                head,
                input_dim: 3,
                hidden_dim: 3,
                num_classes: 2,
            }
        };

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![1.0, 0.0, 0.5], false));
        features.insert(b, Tensor::new(vec![0.8, 0.2, 0.4], false));
        features.insert(c, Tensor::new(vec![0.2, 0.8, 0.6], false));
        features.insert(d, Tensor::new(vec![0.0, 1.0, 0.7], false));

        let mp_config = MessagePassingConfig {
            aggregation: crate::message_passing::Aggregation::Sum,
            activation: Activation::None,
            normalize: false,
            dropout: 0.0,
        };
        let edge_weights = HashMap::new();

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        // Analytical gradient.
        let (fwd_logits, cache) =
            model::forward(&gnn_model, &graph, &features, &edge_weights, &mp_config).unwrap();

        let _loss = model::compute_loss_from_logits(&fwd_logits, &labels, 2);
        let grads = backward(
            &gnn_model,
            &cache,
            &labels,
            2,
            &graph,
            &edge_weights,
            &mp_config,
        )
        .unwrap();

        // Helper: centered finite difference (f(x+eps) - f(x-eps)) / (2*eps).
        // Use a relatively large epsilon because f32 precision limits accuracy.
        let epsilon = 5e-3;
        let num_grad = |mut model_plus: GNNModel, mut model_minus: GNNModel, idx: usize, param: &str| -> f32 {
            match param {
                "w_out" => {
                    model_plus.head.w_out.data[idx] += epsilon;
                    model_minus.head.w_out.data[idx] -= epsilon;
                }
                "b_out" => {
                    model_plus.head.b_out.data[idx] += epsilon;
                    model_minus.head.b_out.data[idx] -= epsilon;
                }
                "w_self" => {
                    model_plus.layers[0].w_self.data[idx] += epsilon;
                    model_minus.layers[0].w_self.data[idx] -= epsilon;
                }
                "w_neigh" => {
                    model_plus.layers[0].w_neigh.data[idx] += epsilon;
                    model_minus.layers[0].w_neigh.data[idx] -= epsilon;
                }
                _ => panic!("unknown param"),
            }
            let (logits_plus, _) = model::forward(&model_plus, &graph, &features, &edge_weights, &mp_config).unwrap();
            let (logits_minus, _) = model::forward(&model_minus, &graph, &features, &edge_weights, &mp_config).unwrap();
            let loss_plus = model::compute_loss_from_logits(&logits_plus, &labels, 2);
            let loss_minus = model::compute_loss_from_logits(&logits_minus, &labels, 2);
            (loss_plus - loss_minus) / (2.0 * epsilon)
        };

        let check = |name: &str, idx: usize, analytical: f32, numerical: f32| {
            let abs_diff = (numerical - analytical).abs();
            let scale = numerical.abs().max(analytical.abs()).max(1e-7);
            let rel_diff = abs_diff / scale;
            assert!(
                rel_diff < 0.05 || abs_diff < 1e-5,
                "{}[{}]: analytical={:.6}, numerical={:.6}, rel_diff={:.6}",
                name, idx, analytical, numerical, rel_diff
            );
        };

        // Check W_out.
        for idx in 0..gnn_model.head.w_out.data.len() {
            let ng = num_grad(gnn_model.clone(), gnn_model.clone(), idx, "w_out");
            check("W_out", idx, grads.d_w_out.data[idx], ng);
        }

        // Check b_out.
        for idx in 0..gnn_model.head.b_out.len() {
            let ng = num_grad(gnn_model.clone(), gnn_model.clone(), idx, "b_out");
            check("b_out", idx, grads.d_b_out.data[idx], ng);
        }

        // Check layer weights if the model has layers.
        if !gnn_model.layers.is_empty() {
            for idx in 0..gnn_model.layers[0].w_self.data.len() {
                let ng = num_grad(gnn_model.clone(), gnn_model.clone(), idx, "w_self");
                check("W_self", idx, grads.d_w_self[0].data[idx], ng);
            }
            for idx in 0..gnn_model.layers[0].w_neigh.data.len() {
                let ng = num_grad(gnn_model.clone(), gnn_model.clone(), idx, "w_neigh");
                check("W_neigh", idx, grads.d_w_neigh[0].data[idx], ng);
            }
        }
    }

    #[test]
    fn test_backward_vs_numerical_with_layer() {
        // Test gradient correctness for a 1-layer model with Activation::None.
        let (graph, a, b, c, d) = make_test_graph();

        use crate::model::{GNNLayer, ClassificationHead};
        use crate::tensor::Matrix;

        let gnn_model = {
            let layer = GNNLayer {
                w_neigh: Matrix { data: vec![0.1, -0.05, 0.02, -0.1, 0.08, -0.03, 0.05, -0.02, 0.07, -0.1, 0.04, 0.06], rows: 4, cols: 3 },
                w_self: Matrix { data: vec![-0.08, 0.1, 0.03, 0.06, -0.04, 0.09, -0.07, 0.05, -0.02, 0.1, -0.06, 0.01], rows: 4, cols: 3 },
                bias: Tensor::new(vec![0.01, -0.01, 0.02, -0.02], false),
                activation: Activation::None,
            };
            let head = ClassificationHead {
                w_out: Matrix { data: vec![0.1, -0.2, 0.15, -0.05, -0.1, 0.2, -0.15, 0.05], rows: 2, cols: 4 },
                b_out: Tensor::new(vec![0.02, -0.02], false),
            };
            GNNModel {
                layers: vec![layer],
                head,
                input_dim: 3,
                hidden_dim: 4,
                num_classes: 2,
            }
        };

        let mut features = HashMap::new();
        features.insert(a, Tensor::new(vec![1.0, 0.0, 0.5], false));
        features.insert(b, Tensor::new(vec![0.8, 0.2, 0.4], false));
        features.insert(c, Tensor::new(vec![0.2, 0.8, 0.6], false));
        features.insert(d, Tensor::new(vec![0.0, 1.0, 0.7], false));

        let mp_config = MessagePassingConfig {
            aggregation: crate::message_passing::Aggregation::Sum,
            activation: Activation::None,
            normalize: false,
            dropout: 0.0,
        };
        let edge_weights = HashMap::new();

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        let (_, cache) =
            model::forward(&gnn_model, &graph, &features, &edge_weights, &mp_config).unwrap();
        let grads = backward(
            &gnn_model, &cache, &labels, 2, &graph, &edge_weights, &mp_config,
        ).unwrap();

        // Use a relatively large epsilon because f32 precision limits
        // the accuracy of finite differences for small gradients.
        let epsilon = 5e-3f32;

        // Centered finite difference helper.
        let num_grad = |mut mp: GNNModel, mut mm: GNNModel, idx: usize, param: &str| -> f32 {
            match param {
                "w_out" => { mp.head.w_out.data[idx] += epsilon; mm.head.w_out.data[idx] -= epsilon; }
                "b_out" => { mp.head.b_out.data[idx] += epsilon; mm.head.b_out.data[idx] -= epsilon; }
                "w_self" => { mp.layers[0].w_self.data[idx] += epsilon; mm.layers[0].w_self.data[idx] -= epsilon; }
                "w_neigh" => { mp.layers[0].w_neigh.data[idx] += epsilon; mm.layers[0].w_neigh.data[idx] -= epsilon; }
                "bias" => { mp.layers[0].bias.data[idx] += epsilon; mm.layers[0].bias.data[idx] -= epsilon; }
                _ => panic!(),
            }
            let (lp, _) = model::forward(&mp, &graph, &features, &edge_weights, &mp_config).unwrap();
            let (lm, _) = model::forward(&mm, &graph, &features, &edge_weights, &mp_config).unwrap();
            let loss_p = model::compute_loss_from_logits(&lp, &labels, 2);
            let loss_m = model::compute_loss_from_logits(&lm, &labels, 2);
            (loss_p - loss_m) / (2.0 * epsilon)
        };

        let check = |name: &str, idx: usize, analytical: f32, numerical: f32| {
            let abs_diff = (numerical - analytical).abs();
            let scale = numerical.abs().max(analytical.abs()).max(1e-7);
            let rel_diff = abs_diff / scale;
            assert!(
                rel_diff < 0.05 || abs_diff < 1e-5,
                "{}[{}]: analytical={:.8}, numerical={:.8}, rel_diff={:.6}",
                name, idx, analytical, numerical, rel_diff
            );
        };

        for idx in 0..gnn_model.head.w_out.data.len() {
            check("W_out", idx, grads.d_w_out.data[idx], num_grad(gnn_model.clone(), gnn_model.clone(), idx, "w_out"));
        }
        for idx in 0..gnn_model.head.b_out.len() {
            check("b_out", idx, grads.d_b_out.data[idx], num_grad(gnn_model.clone(), gnn_model.clone(), idx, "b_out"));
        }
        for idx in 0..gnn_model.layers[0].bias.len() {
            check("bias", idx, grads.d_bias[0].data[idx], num_grad(gnn_model.clone(), gnn_model.clone(), idx, "bias"));
        }
        for idx in 0..gnn_model.layers[0].w_self.data.len() {
            check("W_self", idx, grads.d_w_self[0].data[idx], num_grad(gnn_model.clone(), gnn_model.clone(), idx, "w_self"));
        }
        for idx in 0..gnn_model.layers[0].w_neigh.data.len() {
            check("W_neigh", idx, grads.d_w_neigh[0].data[idx], num_grad(gnn_model.clone(), gnn_model.clone(), idx, "w_neigh"));
        }
    }
}

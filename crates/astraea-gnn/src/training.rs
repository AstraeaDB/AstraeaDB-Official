use std::collections::HashMap;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, EdgeId, NodeId};
use rand::Rng;

use crate::message_passing::{self, MessagePassingConfig};
use crate::tensor::Tensor;

/// Configuration for GNN training.
#[derive(Debug, Clone)]
pub struct TrainingConfig {
    /// Number of message passing layers (rounds of neighbor aggregation).
    pub layers: usize,
    /// Learning rate for gradient descent on edge weights.
    pub learning_rate: f32,
    /// Number of training epochs.
    pub epochs: usize,
    /// Message passing layer configuration.
    pub message_passing: MessagePassingConfig,
}

/// Labels for supervised node classification.
#[derive(Debug, Clone)]
pub struct TrainingData {
    /// Node-to-class mapping for labeled nodes.
    pub labels: HashMap<NodeId, usize>,
    /// Total number of classes.
    pub num_classes: usize,
}

/// Result of training.
#[derive(Debug, Clone)]
pub struct TrainingResult {
    /// Loss value at the end of each epoch.
    pub epoch_losses: Vec<f32>,
    /// Predicted class for each labeled node after training.
    pub final_predictions: HashMap<NodeId, usize>,
    /// Classification accuracy on the labeled set.
    pub accuracy: f32,
}

/// Epsilon for numerical gradient computation (finite differences).
const EPSILON: f32 = 1e-3;

/// Collect all node IDs that appear in the graph by scanning the labeled
/// set and their neighbors. This gives us the working set of nodes.
fn collect_node_ids(
    graph: &dyn GraphOps,
    training_data: &TrainingData,
) -> Result<Vec<NodeId>> {
    let mut all_nodes: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for &node_id in training_data.labels.keys() {
        all_nodes.insert(node_id);

        // Also include neighbors so message passing can flow.
        let neighbors = graph.neighbors(node_id, Direction::Both)?;
        for (_, neighbor_id) in neighbors {
            all_nodes.insert(neighbor_id);
        }
    }

    let mut sorted: Vec<NodeId> = all_nodes.into_iter().collect();
    sorted.sort();
    Ok(sorted)
}

/// Collect all edge IDs connecting the working set of nodes.
fn collect_edge_ids(
    graph: &dyn GraphOps,
    node_ids: &[NodeId],
) -> Result<Vec<EdgeId>> {
    let node_set: std::collections::HashSet<NodeId> = node_ids.iter().copied().collect();
    let mut edge_set: std::collections::HashSet<EdgeId> = std::collections::HashSet::new();

    for &node_id in node_ids {
        let neighbors = graph.neighbors(node_id, Direction::Both)?;
        for (edge_id, neighbor_id) in neighbors {
            if node_set.contains(&neighbor_id) {
                edge_set.insert(edge_id);
            }
        }
    }

    let mut sorted: Vec<EdgeId> = edge_set.into_iter().collect();
    sorted.sort();
    Ok(sorted)
}

/// Initialize node features from embeddings, or create random features if absent.
fn init_node_features(
    graph: &dyn GraphOps,
    node_ids: &[NodeId],
    feature_dim: usize,
) -> Result<HashMap<NodeId, Tensor>> {
    let mut rng = rand::thread_rng();
    let mut features = HashMap::new();

    for &node_id in node_ids {
        let node = graph.get_node(node_id)?;
        let tensor = match node {
            Some(n) if n.embedding.is_some() => {
                let emb = n.embedding.unwrap();
                if emb.len() == feature_dim {
                    Tensor::new(emb, false)
                } else {
                    // Truncate or pad to feature_dim.
                    let mut data = emb;
                    data.resize(feature_dim, 0.0);
                    Tensor::new(data, false)
                }
            }
            _ => {
                // Random initialization in [-0.5, 0.5].
                let data: Vec<f32> = (0..feature_dim)
                    .map(|_| rng.r#gen::<f32>() - 0.5)
                    .collect();
                Tensor::new(data, false)
            }
        };
        features.insert(node_id, tensor);
    }

    Ok(features)
}

/// Initialize edge weights from the graph.
fn init_edge_weights(
    graph: &dyn GraphOps,
    edge_ids: &[EdgeId],
) -> Result<HashMap<EdgeId, f32>> {
    let mut weights = HashMap::new();
    for &edge_id in edge_ids {
        let edge = graph.get_edge(edge_id)?;
        let w = match edge {
            Some(e) => e.weight as f32,
            None => 1.0,
        };
        weights.insert(edge_id, w);
    }
    Ok(weights)
}

/// Run the forward pass: apply N message passing layers, then return
/// final node features.
fn forward_pass(
    graph: &dyn GraphOps,
    initial_features: &HashMap<NodeId, Tensor>,
    edge_weights: &HashMap<EdgeId, f32>,
    config: &MessagePassingConfig,
    layers: usize,
) -> Result<HashMap<NodeId, Tensor>> {
    let mut current = initial_features.clone();

    for _ in 0..layers {
        current = message_passing::message_passing(graph, &current, edge_weights, config)?;
    }

    Ok(current)
}

/// Compute predictions from final node features.
/// For each labeled node, the predicted class is the argmax of the feature vector
/// (we treat feature dimensions as pseudo-logits for classes).
///
/// If the feature dimension is smaller than `num_classes`, we use modular indexing.
fn predict(
    features: &HashMap<NodeId, Tensor>,
    labeled_nodes: &[NodeId],
    num_classes: usize,
) -> HashMap<NodeId, usize> {
    let mut predictions = HashMap::new();

    for &node_id in labeled_nodes {
        if let Some(feat) = features.get(&node_id) {
            if feat.is_empty() {
                predictions.insert(node_id, 0);
                continue;
            }

            // Use features as logits. If feature dim < num_classes, that's fine --
            // we take argmax over available dimensions, then clamp to num_classes.
            let mut best_idx = 0;
            let mut best_val = f32::NEG_INFINITY;
            for (i, &val) in feat.data.iter().enumerate() {
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

/// Compute a simplified cross-entropy-like loss.
///
/// For each labeled node, we interpret the feature vector as unnormalized logits.
/// We compute: loss = -log(softmax(features)[true_class])
///
/// The total loss is the mean over all labeled nodes.
fn compute_loss(
    features: &HashMap<NodeId, Tensor>,
    labels: &HashMap<NodeId, usize>,
    num_classes: usize,
) -> f32 {
    if labels.is_empty() {
        return 0.0;
    }

    let mut total_loss = 0.0;
    let mut count = 0;

    for (&node_id, &true_class) in labels {
        if let Some(feat) = features.get(&node_id) {
            if feat.is_empty() {
                continue;
            }

            // Use first num_classes elements (or all if fewer) as logits.
            let logit_len = feat.data.len().min(num_classes);
            let logits: Vec<f32> = feat.data[..logit_len].to_vec();

            // Softmax with numerical stability (subtract max).
            let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let exp_logits: Vec<f32> = logits.iter().map(|x| (x - max_logit).exp()).collect();
            let sum_exp: f32 = exp_logits.iter().sum();

            // Probability of the true class.
            let target_idx = true_class % logit_len;
            let prob = exp_logits[target_idx] / sum_exp;

            // Cross-entropy: -log(prob). Clamp to avoid log(0).
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

/// Compute accuracy: fraction of labeled nodes whose prediction matches the label.
fn compute_accuracy(
    predictions: &HashMap<NodeId, usize>,
    labels: &HashMap<NodeId, usize>,
) -> f32 {
    if labels.is_empty() {
        return 1.0;
    }

    let mut correct = 0;
    let mut total = 0;

    for (&node_id, &true_class) in labels {
        if let Some(&predicted) = predictions.get(&node_id) {
            if predicted == true_class {
                correct += 1;
            }
            total += 1;
        }
    }

    if total > 0 {
        correct as f32 / total as f32
    } else {
        0.0
    }
}

/// Train a GNN for node classification.
///
/// This implements a simplified GNN training loop using numerical gradients
/// (finite differences) for edge weight updates:
///
/// 1. Initialize node features from embeddings (or random if absent).
/// 2. Initialize edge weights from the graph.
/// 3. For each epoch:
///    a. Forward pass: run N message passing layers.
///    b. Compute loss (cross-entropy on labeled nodes).
///    c. Backward pass: for each edge weight, compute numerical gradient
///       via finite differences: grad = (loss(w+eps) - loss(w)) / eps.
///    d. Update: w -= learning_rate * gradient.
///    e. Record epoch loss.
/// 4. Return training metrics.
///
/// # Errors
///
/// Returns an error if graph operations fail or if the training data is empty.
pub fn train_node_classification(
    graph: &dyn GraphOps,
    training_data: &TrainingData,
    config: &TrainingConfig,
) -> Result<TrainingResult> {
    if training_data.labels.is_empty() {
        return Err(AstraeaError::QueryExecution(
            "training data has no labels".into(),
        ));
    }
    if training_data.num_classes == 0 {
        return Err(AstraeaError::QueryExecution(
            "num_classes must be greater than 0".into(),
        ));
    }

    // Determine feature dimension: use num_classes so softmax logits align.
    let feature_dim = training_data.num_classes;

    // Collect nodes and edges in the working set.
    let node_ids = collect_node_ids(graph, training_data)?;
    let edge_ids = collect_edge_ids(graph, &node_ids)?;

    // Initialize features and weights.
    let initial_features = init_node_features(graph, &node_ids, feature_dim)?;
    let mut edge_weights = init_edge_weights(graph, &edge_ids)?;

    let labeled_nodes: Vec<NodeId> = training_data.labels.keys().copied().collect();
    let mut epoch_losses: Vec<f32> = Vec::with_capacity(config.epochs);

    for _epoch in 0..config.epochs {
        // Forward pass with current weights.
        let features = forward_pass(
            graph,
            &initial_features,
            &edge_weights,
            &config.message_passing,
            config.layers,
        )?;

        // Compute loss.
        let loss = compute_loss(&features, &training_data.labels, training_data.num_classes);
        epoch_losses.push(loss);

        // Backward pass: numerical gradient for each edge weight.
        let mut gradients: HashMap<EdgeId, f32> = HashMap::new();

        for &edge_id in &edge_ids {
            let original_weight = edge_weights[&edge_id];

            // Perturb weight by +epsilon.
            edge_weights.insert(edge_id, original_weight + EPSILON);

            let perturbed_features = forward_pass(
                graph,
                &initial_features,
                &edge_weights,
                &config.message_passing,
                config.layers,
            )?;
            let perturbed_loss = compute_loss(
                &perturbed_features,
                &training_data.labels,
                training_data.num_classes,
            );

            // Gradient via finite differences: (f(w+eps) - f(w)) / eps.
            let grad = (perturbed_loss - loss) / EPSILON;
            gradients.insert(edge_id, grad);

            // Restore original weight.
            edge_weights.insert(edge_id, original_weight);
        }

        // Update edge weights: w -= learning_rate * gradient.
        for &edge_id in &edge_ids {
            let w = edge_weights.get_mut(&edge_id).unwrap();
            let grad = gradients.get(&edge_id).copied().unwrap_or(0.0);
            *w -= config.learning_rate * grad;
        }
    }

    // Final forward pass with trained weights.
    let final_features = forward_pass(
        graph,
        &initial_features,
        &edge_weights,
        &config.message_passing,
        config.layers,
    )?;

    let final_predictions = predict(
        &final_features,
        &labeled_nodes,
        training_data.num_classes,
    );

    let accuracy = compute_accuracy(&final_predictions, &training_data.labels);

    Ok(TrainingResult {
        epoch_losses,
        final_predictions,
        accuracy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_passing::{Activation, Aggregation, MessagePassingConfig};
    use astraea_core::traits::GraphOps;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    /// Build a simple bipartite-ish graph for classification:
    ///
    /// Class 0 cluster: nodes A, B connected to each other.
    /// Class 1 cluster: nodes C, D connected to each other.
    /// Bridge: B -> C with low weight.
    ///
    /// Embeddings encode class membership:
    ///   A, B: [1.0, 0.0] (class 0)
    ///   C, D: [0.0, 1.0] (class 1)
    fn make_classification_graph() -> (Graph, NodeId, NodeId, NodeId, NodeId) {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));

        let a = graph
            .create_node(
                vec!["Class0".into()],
                serde_json::json!({}),
                Some(vec![1.0, 0.0]),
            )
            .unwrap();
        let b = graph
            .create_node(
                vec!["Class0".into()],
                serde_json::json!({}),
                Some(vec![0.8, 0.2]),
            )
            .unwrap();
        let c = graph
            .create_node(
                vec!["Class1".into()],
                serde_json::json!({}),
                Some(vec![0.2, 0.8]),
            )
            .unwrap();
        let d = graph
            .create_node(
                vec!["Class1".into()],
                serde_json::json!({}),
                Some(vec![0.0, 1.0]),
            )
            .unwrap();

        // Intra-class edges with high weight.
        graph
            .create_edge(
                a,
                b,
                "SIMILAR".into(),
                serde_json::json!({}),
                2.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                c,
                d,
                "SIMILAR".into(),
                serde_json::json!({}),
                2.0,
                None,
                None,
            )
            .unwrap();

        // Inter-class bridge with low weight.
        graph
            .create_edge(
                b,
                c,
                "BRIDGE".into(),
                serde_json::json!({}),
                0.1,
                None,
                None,
            )
            .unwrap();

        (graph, a, b, c, d)
    }

    #[test]
    fn test_training_loss_decreases() {
        let (graph, a, b, c, d) = make_classification_graph();

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        let training_data = TrainingData {
            labels,
            num_classes: 2,
        };

        let config = TrainingConfig {
            layers: 1,
            learning_rate: 0.1,
            epochs: 20,
            message_passing: MessagePassingConfig {
                aggregation: Aggregation::Sum,
                activation: Activation::None,
                normalize: false,
            },
        };

        let result = train_node_classification(&graph, &training_data, &config).unwrap();

        assert_eq!(result.epoch_losses.len(), 20);

        // Loss should generally decrease. Compare first vs last epoch.
        let first_loss = result.epoch_losses[0];
        let last_loss = *result.epoch_losses.last().unwrap();
        assert!(
            last_loss <= first_loss + 0.01, // allow tiny float noise
            "expected loss to decrease: first={}, last={}",
            first_loss,
            last_loss
        );
    }

    #[test]
    fn test_training_predictions() {
        let (graph, a, b, c, d) = make_classification_graph();

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        let training_data = TrainingData {
            labels,
            num_classes: 2,
        };

        let config = TrainingConfig {
            layers: 1,
            learning_rate: 0.1,
            epochs: 50,
            message_passing: MessagePassingConfig {
                aggregation: Aggregation::Sum,
                activation: Activation::None,
                normalize: false,
            },
        };

        let result = train_node_classification(&graph, &training_data, &config).unwrap();

        // With clearly separated embeddings and sufficient training,
        // the model should achieve some correct predictions.
        // At minimum, nodes with strong class signals (A and D) should be correct.
        assert!(
            result.accuracy >= 0.5,
            "expected accuracy >= 0.5, got {}",
            result.accuracy
        );

        // Verify predictions map contains all labeled nodes.
        assert!(result.final_predictions.contains_key(&a));
        assert!(result.final_predictions.contains_key(&b));
        assert!(result.final_predictions.contains_key(&c));
        assert!(result.final_predictions.contains_key(&d));
    }

    #[test]
    fn test_training_config() {
        let (graph, a, b, c, d) = make_classification_graph();

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        let training_data = TrainingData {
            labels,
            num_classes: 2,
        };

        // Test with different configurations.

        // Config 1: Mean aggregation, sigmoid activation.
        let config1 = TrainingConfig {
            layers: 2,
            learning_rate: 0.05,
            epochs: 10,
            message_passing: MessagePassingConfig {
                aggregation: Aggregation::Mean,
                activation: Activation::Sigmoid,
                normalize: false,
            },
        };

        let result1 = train_node_classification(&graph, &training_data, &config1).unwrap();
        assert_eq!(result1.epoch_losses.len(), 10);

        // Config 2: Sum aggregation with normalization.
        let config2 = TrainingConfig {
            layers: 1,
            learning_rate: 0.2,
            epochs: 5,
            message_passing: MessagePassingConfig {
                aggregation: Aggregation::Sum,
                activation: Activation::ReLU,
                normalize: true,
            },
        };

        let result2 = train_node_classification(&graph, &training_data, &config2).unwrap();
        assert_eq!(result2.epoch_losses.len(), 5);

        // Both should produce valid results (no NaN or infinite losses).
        for loss in &result1.epoch_losses {
            assert!(loss.is_finite(), "loss should be finite, got {}", loss);
        }
        for loss in &result2.epoch_losses {
            assert!(loss.is_finite(), "loss should be finite, got {}", loss);
        }
    }

    #[test]
    fn test_training_empty_labels_errors() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));

        let training_data = TrainingData {
            labels: HashMap::new(),
            num_classes: 2,
        };

        let config = TrainingConfig {
            layers: 1,
            learning_rate: 0.1,
            epochs: 10,
            message_passing: MessagePassingConfig::default(),
        };

        let result = train_node_classification(&graph, &training_data, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_training_single_epoch() {
        let (graph, a, _b, c, _d) = make_classification_graph();

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(c, 1);

        let training_data = TrainingData {
            labels,
            num_classes: 2,
        };

        let config = TrainingConfig {
            layers: 1,
            learning_rate: 0.01,
            epochs: 1,
            message_passing: MessagePassingConfig {
                aggregation: Aggregation::Sum,
                activation: Activation::None,
                normalize: false,
            },
        };

        let result = train_node_classification(&graph, &training_data, &config).unwrap();
        assert_eq!(result.epoch_losses.len(), 1);
        assert!(result.epoch_losses[0].is_finite());
    }

    #[test]
    fn test_loss_computation_correctness() {
        // Manual test of loss with known features.
        let mut features = HashMap::new();
        let node = NodeId(1);
        // logits: [2.0, 1.0]. True class = 0.
        // softmax: exp(2)/(exp(2)+exp(1)) ~ 0.731, exp(1)/(exp(2)+exp(1)) ~ 0.269
        // loss = -ln(0.731) ~ 0.313
        features.insert(node, Tensor::new(vec![2.0, 1.0], false));

        let mut labels = HashMap::new();
        labels.insert(node, 0);

        let loss = compute_loss(&features, &labels, 2);

        let expected = -(2.0f32.exp() / (2.0f32.exp() + 1.0f32.exp())).ln();
        assert!(
            (loss - expected).abs() < 1e-4,
            "expected loss ~{}, got {}",
            expected,
            loss
        );
    }

    #[test]
    fn test_prediction_argmax() {
        let mut features = HashMap::new();
        let n1 = NodeId(1);
        let n2 = NodeId(2);
        features.insert(n1, Tensor::new(vec![0.1, 0.9, 0.3], false));
        features.insert(n2, Tensor::new(vec![0.8, 0.2, 0.1], false));

        let preds = predict(&features, &[n1, n2], 3);

        assert_eq!(preds[&n1], 1); // index 1 has max value 0.9
        assert_eq!(preds[&n2], 0); // index 0 has max value 0.8
    }
}

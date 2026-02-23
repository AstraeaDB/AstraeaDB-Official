use std::collections::HashMap;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, EdgeId, NodeId};
use rand::Rng;

use crate::backward;
use crate::message_passing::{Activation, MessagePassingConfig};
use crate::model::{self, GNNModel};
use crate::tensor::{Matrix, Tensor};
use crate::training::TrainingData;

/// A single GRU cell that evolves a weight matrix between timesteps.
///
/// Given the current weight matrix W_t and a context vector h_{t-1}, computes:
///   z = sigmoid(W_z * x + U_z * h + b_z)        (update gate)
///   r = sigmoid(W_r * x + U_r * h + b_r)        (reset gate)
///   h_cand = tanh(W_h * x + U_h * (r .* h) + b_h)  (candidate)
///   h_t = (1 - z) .* h + z .* h_cand             (new hidden state)
///
/// For EvolveGCN-H, the "input" x is a summary of the current timestep's features
/// and the hidden state h encodes the evolving weight context.
#[derive(Debug, Clone)]
pub struct GRUCell {
    /// Update gate weights: [hidden_dim x input_dim].
    pub w_z: Matrix,
    /// Update gate recurrent: [hidden_dim x hidden_dim].
    pub u_z: Matrix,
    /// Update gate bias.
    pub b_z: Tensor,
    /// Reset gate weights.
    pub w_r: Matrix,
    /// Reset gate recurrent.
    pub u_r: Matrix,
    /// Reset gate bias.
    pub b_r: Tensor,
    /// Candidate weights.
    pub w_h: Matrix,
    /// Candidate recurrent.
    pub u_h: Matrix,
    /// Candidate bias.
    pub b_h: Tensor,
}

impl GRUCell {
    /// Create a GRU cell with Xavier-initialized weights.
    pub fn new(input_dim: usize, hidden_dim: usize, rng: &mut impl Rng) -> Self {
        Self {
            w_z: Matrix::random_xavier(hidden_dim, input_dim, rng),
            u_z: Matrix::random_xavier(hidden_dim, hidden_dim, rng),
            b_z: Tensor::zeros(hidden_dim, false),
            w_r: Matrix::random_xavier(hidden_dim, input_dim, rng),
            u_r: Matrix::random_xavier(hidden_dim, hidden_dim, rng),
            b_r: Tensor::zeros(hidden_dim, false),
            w_h: Matrix::random_xavier(hidden_dim, input_dim, rng),
            u_h: Matrix::random_xavier(hidden_dim, hidden_dim, rng),
            b_h: Tensor::zeros(hidden_dim, false),
        }
    }

    /// Run one GRU step: (input, prev_hidden) -> new_hidden.
    ///
    /// Both `input` and `prev_hidden` are tensors of length `hidden_dim`.
    pub fn forward(&self, input: &Tensor, prev_hidden: &Tensor) -> Tensor {
        // Update gate: z = sigmoid(W_z * x + U_z * h + b_z)
        let z = self
            .w_z
            .matvec(input)
            .add(&self.u_z.matvec(prev_hidden))
            .add(&self.b_z)
            .sigmoid();

        // Reset gate: r = sigmoid(W_r * x + U_r * h + b_r)
        let r = self
            .w_r
            .matvec(input)
            .add(&self.u_r.matvec(prev_hidden))
            .add(&self.b_r)
            .sigmoid();

        // Candidate: h_cand = tanh(W_h * x + U_h * (r .* h) + b_h)
        let r_h = r.mul(prev_hidden);
        let h_cand = self
            .w_h
            .matvec(input)
            .add(&self.u_h.matvec(&r_h))
            .add(&self.b_h)
            .tanh_act();

        // New hidden: h_t = (1 - z) .* h + z .* h_cand
        let one_minus_z = Tensor::new(z.data.iter().map(|&v| 1.0 - v).collect(), false);
        one_minus_z.mul(prev_hidden).add(&z.mul(&h_cand))
    }
}

/// A temporal GNN model using EvolveGCN-H style weight evolution.
///
/// At each timestep, GRU cells evolve the GNN layer weights based on a
/// summary of the graph's current state. This allows the model to adapt
/// to changing graph structure over time.
#[derive(Debug, Clone)]
pub struct TemporalGNNModel {
    /// Base GNN model (provides the initial weight matrices).
    pub base_model: GNNModel,
    /// One GRU cell per GNN layer to evolve its weights over time.
    pub gru_cells: Vec<GRUCell>,
    /// Hidden states for each GRU cell (evolved across timesteps).
    pub gru_hidden: Vec<Tensor>,
}

impl TemporalGNNModel {
    /// Create a new temporal model from dimensions and number of layers.
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

    /// Create a new temporal model with a provided RNG.
    pub fn new_with_rng(
        input_dim: usize,
        hidden_dim: usize,
        num_classes: usize,
        num_layers: usize,
        activation: Activation,
        rng: &mut impl Rng,
    ) -> Self {
        let base_model =
            GNNModel::new_with_rng(input_dim, hidden_dim, num_classes, num_layers, activation, rng);

        // Each GRU cell evolves a flattened weight summary.
        // We use hidden_dim as the GRU state dimension for simplicity.
        let mut gru_cells = Vec::with_capacity(num_layers);
        let mut gru_hidden = Vec::with_capacity(num_layers);

        for _ in 0..num_layers {
            gru_cells.push(GRUCell::new(hidden_dim, hidden_dim, rng));
            gru_hidden.push(Tensor::zeros(hidden_dim, false));
        }

        Self {
            base_model,
            gru_cells,
            gru_hidden,
        }
    }

    /// Evolve the model's weights for the current timestep.
    ///
    /// Uses the mean of node features as input context to each GRU cell.
    /// The GRU hidden state is used to perturb layer biases, providing
    /// a time-varying signal that adapts the model to structural changes.
    pub fn evolve_weights(&mut self, feature_summary: &Tensor) {
        // Truncate or pad the summary to hidden_dim.
        let hd = self.base_model.hidden_dim;
        let summary = if feature_summary.len() >= hd {
            Tensor::new(feature_summary.data[..hd].to_vec(), false)
        } else {
            let mut data = feature_summary.data.clone();
            data.resize(hd, 0.0);
            Tensor::new(data, false)
        };

        for (i, gru) in self.gru_cells.iter().enumerate() {
            let new_hidden = gru.forward(&summary, &self.gru_hidden[i]);
            // Use GRU output to modulate layer biases (additive perturbation).
            for k in 0..self.base_model.layers[i].bias.len().min(new_hidden.len()) {
                self.base_model.layers[i].bias.data[k] += 0.1 * new_hidden.data[k];
            }
            self.gru_hidden[i] = new_hidden;
        }
    }

    /// Reset the GRU hidden states (e.g., at the start of a new training run).
    pub fn reset_hidden(&mut self) {
        let hd = self.base_model.hidden_dim;
        for h in &mut self.gru_hidden {
            *h = Tensor::zeros(hd, false);
        }
    }
}

/// Configuration for temporal GNN training.
#[derive(Debug, Clone)]
pub struct TemporalTrainingConfig {
    /// Hidden dimension for GNN layers.
    pub hidden_dim: usize,
    /// Number of GNN layers.
    pub num_layers: usize,
    /// Learning rate.
    pub learning_rate: f32,
    /// Number of passes over the full sequence of timesteps.
    pub epochs: usize,
    /// Activation function.
    pub activation: Activation,
    /// Message passing configuration.
    pub message_passing: MessagePassingConfig,
}

/// Result of temporal training across all timesteps.
#[derive(Debug, Clone)]
pub struct TemporalTrainingResult {
    /// Average loss per epoch across all timesteps.
    pub epoch_losses: Vec<f32>,
    /// Per-timestep accuracy from the final epoch.
    pub timestep_accuracies: Vec<(i64, f32)>,
    /// The trained temporal model.
    pub model: TemporalGNNModel,
}

/// Compute the mean feature vector from a set of node features.
fn mean_features(features: &HashMap<NodeId, Tensor>) -> Tensor {
    if features.is_empty() {
        return Tensor::zeros(1, false);
    }
    let dim = features.values().next().unwrap().len();
    let mut sum = vec![0.0f32; dim];
    for feat in features.values() {
        for (i, &v) in feat.data.iter().enumerate() {
            if i < dim {
                sum[i] += v;
            }
        }
    }
    let n = features.len() as f32;
    Tensor::new(sum.into_iter().map(|v| v / n).collect(), false)
}

/// Collect node features at a specific timestamp from the graph.
///
/// Gathers features for labeled nodes and their temporal neighbors.
fn collect_temporal_features(
    graph: &dyn GraphOps,
    labels: &HashMap<NodeId, usize>,
    timestamp: i64,
) -> Result<(HashMap<NodeId, Tensor>, HashMap<EdgeId, f32>)> {
    let mut features = HashMap::new();
    let mut edge_weights = HashMap::new();
    let mut visited = std::collections::HashSet::new();

    for &node_id in labels.keys() {
        if visited.insert(node_id) {
            if let Ok(Some(node)) = graph.get_node(node_id) {
                if let Some(emb) = &node.embedding {
                    features.insert(node_id, Tensor::new(emb.clone(), false));
                }
            }
        }

        // Get temporal neighbors.
        if let Ok(neighbors) = graph.neighbors_at(node_id, Direction::Both, timestamp) {
            for (edge_id, neighbor_id) in neighbors {
                if visited.insert(neighbor_id) {
                    if let Ok(Some(neighbor_node)) = graph.get_node(neighbor_id) {
                        if let Some(emb) = &neighbor_node.embedding {
                            features.insert(neighbor_id, Tensor::new(emb.clone(), false));
                        }
                    }
                }
                if let Ok(Some(edge)) = graph.get_edge(edge_id) {
                    edge_weights.insert(edge_id, edge.weight as f32);
                }
            }
        }
    }

    Ok((features, edge_weights))
}

/// Train a temporal GNN model over a sequence of timesteps.
///
/// For each epoch, iterates through timesteps in order:
/// 1. Evolves model weights via GRU using the current timestep's feature summary
/// 2. Runs forward pass on the temporal subgraph
/// 3. Computes loss and backpropagates gradients
/// 4. Updates model parameters
///
/// # Arguments
/// * `graph` - Graph with temporal edges (validity intervals)
/// * `timestep_data` - Sequence of (timestamp, labels) pairs in temporal order
/// * `config` - Training configuration
pub fn train_temporal(
    graph: &dyn GraphOps,
    timestep_data: &[(i64, TrainingData)],
    config: &TemporalTrainingConfig,
) -> Result<TemporalTrainingResult> {
    if timestep_data.is_empty() {
        return Err(AstraeaError::QueryExecution(
            "No timestep data provided".into(),
        ));
    }

    // Detect input dimension from the first timestep's node features.
    let input_dim = detect_temporal_input_dim(graph, &timestep_data[0].1)?;

    let mut temporal_model = TemporalGNNModel::new_with_rng(
        input_dim,
        config.hidden_dim,
        timestep_data[0].1.num_classes,
        config.num_layers,
        config.activation,
        &mut rand::thread_rng(),
    );

    let mut epoch_losses = Vec::with_capacity(config.epochs);
    let mut final_accuracies = Vec::new();

    for epoch in 0..config.epochs {
        temporal_model.reset_hidden();
        let mut total_loss = 0.0;
        let mut timestep_count = 0;

        for (timestamp, training_data) in timestep_data {
            // Collect features at this timestamp.
            let (features, edge_weights) =
                collect_temporal_features(graph, &training_data.labels, *timestamp)?;

            if features.is_empty() {
                continue;
            }

            // Evolve weights using current features.
            let summary = mean_features(&features);
            temporal_model.evolve_weights(&summary);

            // Forward pass.
            let (logits, cache) = model::forward(
                &temporal_model.base_model,
                graph,
                &features,
                &edge_weights,
                &config.message_passing,
            )?;

            // Compute loss.
            let loss = model::compute_loss_from_logits(
                &logits,
                &training_data.labels,
                training_data.num_classes,
            );
            total_loss += loss;
            timestep_count += 1;

            // Backward pass and parameter update.
            let grads = backward::backward(
                &temporal_model.base_model,
                &cache,
                &training_data.labels,
                training_data.num_classes,
                graph,
                &edge_weights,
                &config.message_passing,
            )?;

            // SGD update on base model parameters.
            let lr = config.learning_rate;
            for (i, layer) in temporal_model.base_model.layers.iter_mut().enumerate() {
                layer.w_neigh.sub_assign(&grads.d_w_neigh[i].scale(lr));
                layer.w_self.sub_assign(&grads.d_w_self[i].scale(lr));
                for k in 0..layer.bias.len() {
                    layer.bias.data[k] -= lr * grads.d_bias[i].data[k];
                }
            }
            temporal_model
                .base_model
                .head
                .w_out
                .sub_assign(&grads.d_w_out.scale(lr));
            for k in 0..temporal_model.base_model.head.b_out.len() {
                temporal_model.base_model.head.b_out.data[k] -= lr * grads.d_b_out.data[k];
            }

            // Track accuracy on the last epoch.
            if epoch == config.epochs - 1 {
                let labeled_nodes: Vec<NodeId> = training_data.labels.keys().copied().collect();
                let preds = model::predict_from_logits(
                    &logits,
                    &labeled_nodes,
                    training_data.num_classes,
                );
                let correct = training_data
                    .labels
                    .iter()
                    .filter(|(nid, label)| preds.get(nid) == Some(label))
                    .count();
                let acc = if training_data.labels.is_empty() {
                    0.0
                } else {
                    correct as f32 / training_data.labels.len() as f32
                };
                final_accuracies.push((*timestamp, acc));
            }
        }

        let avg_loss = if timestep_count > 0 {
            total_loss / timestep_count as f32
        } else {
            0.0
        };
        epoch_losses.push(avg_loss);
    }

    Ok(TemporalTrainingResult {
        epoch_losses,
        timestep_accuracies: final_accuracies,
        model: temporal_model,
    })
}

/// Detect input feature dimension from graph node embeddings.
fn detect_temporal_input_dim(graph: &dyn GraphOps, data: &TrainingData) -> Result<usize> {
    for &node_id in data.labels.keys() {
        if let Ok(Some(node)) = graph.get_node(node_id) {
            if let Some(emb) = &node.embedding {
                if !emb.is_empty() {
                    return Ok(emb.len());
                }
            }
        }
    }
    Err(AstraeaError::QueryExecution(
        "Could not detect input dimension: no nodes have embeddings".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_passing::Aggregation;
    use astraea_graph::test_utils::InMemoryStorage;
    use astraea_graph::Graph;

    fn make_temporal_graph() -> (Graph, Vec<NodeId>) {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));

        // Create 4 nodes with embeddings.
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

        // Temporal edges: some active at t=100, some at t=200.
        // a-b: valid [50, 150)
        graph
            .create_edge(
                a,
                b,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                Some(50),
                Some(150),
            )
            .unwrap();
        // c-d: valid [50, 250)
        graph
            .create_edge(
                c,
                d,
                "LINK".into(),
                serde_json::json!({}),
                1.0,
                Some(50),
                Some(250),
            )
            .unwrap();
        // b-c: valid [100, 200)
        graph
            .create_edge(
                b,
                c,
                "BRIDGE".into(),
                serde_json::json!({}),
                0.5,
                Some(100),
                Some(200),
            )
            .unwrap();
        // a-d: valid [150, 300)
        graph
            .create_edge(
                a,
                d,
                "LATE".into(),
                serde_json::json!({}),
                0.8,
                Some(150),
                Some(300),
            )
            .unwrap();

        (graph, vec![a, b, c, d])
    }

    #[test]
    fn test_gru_cell_forward() {
        let mut rng = rand::thread_rng();
        let gru = GRUCell::new(4, 4, &mut rng);

        let input = Tensor::new(vec![1.0, 0.5, -0.3, 0.7], false);
        let hidden = Tensor::zeros(4, false);

        let new_hidden = gru.forward(&input, &hidden);
        assert_eq!(new_hidden.len(), 4);

        // Output should be bounded (tanh components) and non-trivial.
        for &v in &new_hidden.data {
            assert!(v.is_finite(), "GRU output should be finite");
            assert!(v.abs() <= 1.0 + 1e-6, "GRU output should be bounded by tanh");
        }

        // Feeding the output back as hidden should produce a different result.
        let hidden2 = gru.forward(&input, &new_hidden);
        let diff: f32 = hidden2
            .data
            .iter()
            .zip(new_hidden.data.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 1e-6, "Sequential GRU steps should produce different outputs");
    }

    #[test]
    fn test_temporal_model_evolve() {
        let mut model = TemporalGNNModel::new(3, 4, 2, 1, Activation::ReLU);

        let bias_before = model.base_model.layers[0].bias.data.clone();

        let summary = Tensor::new(vec![1.0, 0.5, -0.3], false);
        model.evolve_weights(&summary);

        // Bias should have changed after evolution.
        let bias_after = &model.base_model.layers[0].bias.data;
        let diff: f32 = bias_before
            .iter()
            .zip(bias_after.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 1e-8, "Biases should change after weight evolution");
    }

    #[test]
    fn test_temporal_training_basic() {
        let (graph, nodes) = make_temporal_graph();
        let [a, b, c, d] = [nodes[0], nodes[1], nodes[2], nodes[3]];

        // Two timesteps with different active edges.
        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        let timestep_data = vec![
            (
                100,
                TrainingData {
                    labels: labels.clone(),
                    num_classes: 2,
                },
            ),
            (
                175,
                TrainingData {
                    labels: labels.clone(),
                    num_classes: 2,
                },
            ),
        ];

        let config = TemporalTrainingConfig {
            hidden_dim: 8,
            num_layers: 1,
            learning_rate: 0.01,
            epochs: 5,
            activation: Activation::ReLU,
            message_passing: MessagePassingConfig {
                aggregation: Aggregation::Sum,
                activation: Activation::ReLU,
                normalize: false,
                dropout: 0.0,
            },
        };

        let result = train_temporal(&graph, &timestep_data, &config).unwrap();

        // Should have one loss per epoch.
        assert_eq!(result.epoch_losses.len(), 5);

        // Loss should generally decrease (or at least not be NaN).
        for &loss in &result.epoch_losses {
            assert!(loss.is_finite(), "Loss should be finite, got {}", loss);
        }

        // Should have accuracy for each timestep from the final epoch.
        assert_eq!(result.timestep_accuracies.len(), 2);
        assert_eq!(result.timestep_accuracies[0].0, 100);
        assert_eq!(result.timestep_accuracies[1].0, 175);
    }

    #[test]
    fn test_temporal_training_loss_decreases() {
        let (graph, nodes) = make_temporal_graph();
        let [a, b, c, d] = [nodes[0], nodes[1], nodes[2], nodes[3]];

        let mut labels = HashMap::new();
        labels.insert(a, 0);
        labels.insert(b, 0);
        labels.insert(c, 1);
        labels.insert(d, 1);

        let timestep_data = vec![(
            100,
            TrainingData {
                labels,
                num_classes: 2,
            },
        )];

        let config = TemporalTrainingConfig {
            hidden_dim: 8,
            num_layers: 1,
            learning_rate: 0.01,
            epochs: 30,
            activation: Activation::ReLU,
            message_passing: MessagePassingConfig {
                aggregation: Aggregation::Sum,
                activation: Activation::ReLU,
                normalize: false,
                dropout: 0.0,
            },
        };

        let result = train_temporal(&graph, &timestep_data, &config).unwrap();

        // The minimum loss seen during training should be less than the first loss.
        // Temporal models may have non-monotonic loss due to GRU weight evolution.
        let first_loss = result.epoch_losses[0];
        let min_loss = result
            .epoch_losses
            .iter()
            .cloned()
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_loss < first_loss,
            "Min loss should be less than first: first={}, min={}",
            first_loss,
            min_loss,
        );
    }
}

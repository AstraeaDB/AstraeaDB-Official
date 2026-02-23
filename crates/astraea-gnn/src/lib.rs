pub mod backward;
pub mod message_passing;
pub mod model;
pub mod sampling;
pub mod sparse;
pub mod tensor;
pub mod temporal;
pub mod training;

pub use temporal::{GRUCell, TemporalGNNModel, TemporalTrainingConfig, TemporalTrainingResult, train_temporal};
pub use tensor::{Tensor, Matrix};
pub use message_passing::{MessagePassingConfig, Aggregation, Activation, message_passing};
pub use model::{GNNLayer, ClassificationHead, GNNModel, ForwardCache, forward, predict_from_logits, compute_loss_from_logits};
pub use backward::{Gradients, backward};
pub use sparse::{FeatureMatrix, CSRAdjacency, message_passing_spmm};
pub use sampling::{SamplingConfig, SampledSubgraph, sample_subgraph};
pub use training::{TrainingConfig, TrainingData, TrainingResult, train_node_classification};

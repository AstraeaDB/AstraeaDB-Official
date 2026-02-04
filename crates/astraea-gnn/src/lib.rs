pub mod tensor;
pub mod message_passing;
pub mod training;

pub use tensor::Tensor;
pub use message_passing::{MessagePassingConfig, Aggregation, Activation, message_passing};
pub use training::{TrainingConfig, TrainingData, TrainingResult, train_node_classification};

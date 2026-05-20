//! Pure-Rust graph neural network primitives over `astraea_core::GraphOps`.
//!
//! Provides a minimal tensor stack (`Tensor`, `Matrix`), `GNNLayer` /
//! `GNNModel` / `ClassificationHead`, message passing
//! (`message_passing`, `MessagePassingConfig`, `Aggregation`,
//! `Activation`), sparse adjacency (`CSRAdjacency`, `FeatureMatrix`),
//! GraphSAGE-style `sampling`, Adam-driven `training`, and a
//! `TemporalGNNModel` with `GRUCell` for time-evolving graphs.
//!
//! Invariants: `Tensor` is 1D only and `Matrix` is row-major
//! `[output_dim x input_dim]`; shape mismatches panic via `assert_eq!`.
//! `train_node_classification` has two paths — `hidden_dim = Some(_)`
//! triggers analytical backprop, `None` triggers the legacy
//! finite-difference path that only tunes edge weights. Training is
//! single-threaded by design (`Tensor::grad` is a `RefCell`, not
//! thread-safe).

pub mod backward;
pub mod message_passing;
pub mod model;
pub mod sampling;
pub mod sparse;
pub mod temporal;
pub mod tensor;
pub mod training;

pub use backward::{Gradients, backward};
pub use message_passing::{Activation, Aggregation, MessagePassingConfig, message_passing};
pub use model::{
    ClassificationHead, ForwardCache, GNNLayer, GNNModel, compute_loss_from_logits, forward,
    predict_from_logits,
};
pub use sampling::{SampledSubgraph, SamplingConfig, sample_subgraph};
pub use sparse::{CSRAdjacency, FeatureMatrix, message_passing_spmm};
pub use temporal::{
    GRUCell, TemporalGNNModel, TemporalTrainingConfig, TemporalTrainingResult, train_temporal,
};
pub use tensor::{Matrix, Tensor};
pub use training::{TrainingConfig, TrainingData, TrainingResult, train_node_classification};

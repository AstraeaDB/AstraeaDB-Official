use std::collections::HashMap;

use astraea_core::types::NodeId;

use crate::csr::CsrMatrix;

/// Result of a GPU (or CPU fallback) computation.
///
/// Each variant corresponds to a specific graph algorithm and contains
/// the output indexed by `NodeId` for easy lookup.
#[derive(Debug, Clone)]
pub enum ComputeResult {
    /// PageRank scores indexed by NodeId.
    /// Scores sum to approximately 1.0 across all nodes.
    PageRank(HashMap<NodeId, f64>),
    /// BFS levels indexed by NodeId.
    /// The source node has level 0. Unreachable nodes have level -1.
    BfsLevels(HashMap<NodeId, i32>),
    /// Single-source shortest path distances indexed by NodeId.
    /// The source node has distance 0.0. Unreachable nodes have `f64::INFINITY`.
    SsspDistances(HashMap<NodeId, f64>),
}

/// Configuration for PageRank computation.
pub struct GpuPageRankConfig {
    /// Damping factor (probability of following a link vs. teleporting).
    /// Standard value is 0.85.
    pub damping: f64,
    /// Maximum number of power iterations.
    pub max_iterations: usize,
    /// Convergence tolerance (L1 norm of rank difference between iterations).
    pub tolerance: f64,
}

impl Default for GpuPageRankConfig {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
        }
    }
}

/// Trait for compute backends (GPU or CPU fallback).
///
/// Implementations perform graph algorithms on CSR-formatted matrices.
/// The CPU backend provides a reference implementation; a future CUDA
/// backend will offload computation to the GPU behind a feature gate.
pub trait GpuBackend: Send + Sync {
    /// Compute PageRank on the given CSR matrix.
    ///
    /// Uses the power iteration method. Returns `ComputeResult::PageRank`
    /// with scores that sum to approximately 1.0.
    fn pagerank(&self, matrix: &CsrMatrix, config: &GpuPageRankConfig) -> ComputeResult;

    /// Compute BFS levels from a source node (given as a matrix index).
    ///
    /// Returns `ComputeResult::BfsLevels` where the source has level 0
    /// and unreachable nodes have level -1.
    fn bfs(&self, matrix: &CsrMatrix, source: usize) -> ComputeResult;

    /// Compute single-source shortest paths from a source node (matrix index).
    ///
    /// Uses Bellman-Ford relaxation on the CSR structure. Returns
    /// `ComputeResult::SsspDistances` where the source has distance 0.0
    /// and unreachable nodes have `f64::INFINITY`.
    fn sssp(&self, matrix: &CsrMatrix, source: usize) -> ComputeResult;

    /// Get the backend name (e.g., "CPU", "CUDA").
    fn name(&self) -> &str;

    /// Check if this backend is available.
    ///
    /// The CPU backend always returns `true`. A GPU backend would check
    /// for CUDA device availability.
    fn is_available(&self) -> bool;
}

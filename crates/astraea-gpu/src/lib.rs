//! Compute-backend abstraction for graph algorithms over CSR matrices.
//!
//! Defines the `GpuBackend` trait, the `ComputeResult` reply enum, and
//! `CsrMatrix` (the canonical `f64` row-major sparse layout, built via
//! `CsrMatrix::from_graph`). Algorithms exposed on `GpuBackend` include
//! PageRank (configured by `GpuPageRankConfig`), BFS, and SSSP.
//!
//! Despite the crate name there is currently **no GPU code**: the only
//! `GpuBackend` implementor is `CpuBackend`, a single-threaded reference
//! implementation that reports `name() == "CPU"` (see astraeadb-issues.md
//! #5). `BFS` and `SSSP` take a `source: usize` matrix index, not a
//! `NodeId` — callers must resolve through `csr.node_to_index` first.
//! `SSSP` is Bellman-Ford and will spin on negative cycles with no
//! detection.

pub mod backend;
pub mod cpu;
pub mod csr;

pub use backend::{ComputeResult, GpuBackend};
pub use cpu::CpuBackend;
pub use csr::CsrMatrix;

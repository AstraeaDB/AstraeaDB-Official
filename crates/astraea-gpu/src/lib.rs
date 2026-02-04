pub mod backend;
pub mod cpu;
pub mod csr;

pub use backend::{ComputeResult, GpuBackend};
pub use cpu::CpuBackend;
pub use csr::CsrMatrix;

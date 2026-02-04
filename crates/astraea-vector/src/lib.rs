//! AstraeaDB Vector Index crate.
//!
//! Provides HNSW (Hierarchical Navigable Small World) based approximate
//! nearest-neighbor search for the Vector-Property Graph model.

pub mod distance;
pub mod hnsw;
pub mod index;

pub use index::HnswVectorIndex;

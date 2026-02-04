//! Apache Arrow Flight server for AstraeaDB.
//!
//! Provides zero-copy data exchange using Arrow IPC format.
//! Supports `do_get` for streaming query results and `do_put` for bulk data import.

pub mod schemas;
pub mod service;

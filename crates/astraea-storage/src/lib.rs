//! AstraeaDB Storage Engine
//!
//! This crate provides the disk-backed storage engine for AstraeaDB, including:
//! - Page-based storage format (`page`)
//! - File I/O manager (`file_manager`)
//! - Buffer pool with LRU eviction (`buffer_pool`)
//! - Write-ahead log for crash recovery (`wal`)
//! - The main `DiskStorageEngine` tying it all together (`engine`)

pub mod buffer_pool;
pub mod engine;
pub mod file_manager;
pub mod page;
pub mod wal;

// Re-export the main engine type for convenience.
pub use engine::DiskStorageEngine;

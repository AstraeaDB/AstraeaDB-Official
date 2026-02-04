//! AstraeaDB Storage Engine
//!
//! This crate provides the disk-backed storage engine for AstraeaDB, including:
//! - Page-based storage format (`page`)
//! - File I/O manager (`file_manager`)
//! - Abstract page I/O trait (`page_io`) for pluggable I/O backends
//! - Buffer pool with LRU eviction (`buffer_pool`)
//! - Write-ahead log for crash recovery (`wal`)
//! - MVCC transaction manager (`mvcc`)
//! - Cold tier storage (`cold_storage`) for serializing partitions to disk
//! - The main `DiskStorageEngine` tying it all together (`engine`)

pub mod buffer_pool;
pub mod cold_storage;
pub mod engine;
pub mod file_manager;
pub mod label_index;
pub mod mvcc;
pub mod page;
pub mod page_io;
pub mod wal;

// Re-export the main engine type for convenience.
pub use engine::DiskStorageEngine;

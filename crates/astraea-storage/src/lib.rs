//! AstraeaDB Storage Engine
//!
//! This crate provides the disk-backed storage engine for AstraeaDB, including:
//! - Page-based storage format (`page`)
//! - File I/O manager (`file_manager`)
//! - Abstract page I/O trait (`page_io`) for pluggable I/O backends
//! - io_uring-based I/O backend (`uring_page_io`) - Linux-only, feature-gated
//! - Buffer pool with LRU eviction (`buffer_pool`)
//! - Write-ahead log for crash recovery (`wal`)
//! - MVCC transaction manager (`mvcc`)
//! - Cold tier storage (`cold_storage`) for serializing partitions to disk
//! - Object store cold storage (`object_store_cold`) for S3/GCS/Azure backends
//! - The main `DiskStorageEngine` tying it all together (`engine`)

pub mod buffer_pool;
pub mod cold_storage;
pub mod engine;
pub mod file_manager;
pub mod label_index;
pub mod mvcc;
pub mod object_store_cold;
pub mod page;
pub mod page_io;
pub mod parquet_cold;
#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub mod uring_page_io;
pub mod wal;

// Re-export the main engine type for convenience.
pub use engine::DiskStorageEngine;

// Re-export cold storage implementations.
pub use cold_storage::{ColdEdge, ColdNode, ColdPartition, ColdStorage, JsonFileColdStorage};
pub use object_store_cold::ObjectStoreColdStorage;
pub use parquet_cold::ParquetColdStorage;

// Re-export io_uring-based page I/O (Linux-only, feature-gated).
#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub use uring_page_io::UringPageIO;

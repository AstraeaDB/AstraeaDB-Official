//! Cold Tier Storage for AstraeaDB.
//!
//! This module implements the Cold Tier (Tier 1) of AstraeaDB's tiered storage
//! architecture. Cold storage is designed for data that is infrequently accessed
//! but must be retained for durability and archival purposes.
//!
//! Data is organized into **partitions** — self-contained collections of nodes
//! and edges that can be independently serialized, transferred, and loaded.
//! Each partition is identified by a string key (e.g., a date range, tenant ID,
//! or graph subcomponent name).
//!
//! ## Backends
//!
//! The [`ColdStorage`] trait defines a backend-agnostic interface for partition
//! persistence. The initial implementation is [`JsonFileColdStorage`], which
//! stores each partition as a human-readable JSON file on the local filesystem.
//! Future backends can target object stores (S3/GCS) or columnar formats
//! (Apache Parquet) for better compression and query performance.
//!
//! ## Conversion
//!
//! The [`ColdNode`] and [`ColdEdge`] types provide `From<&Node>` and
//! `From<&Edge>` conversions so that live graph data can be easily serialized
//! into the cold format without manual field mapping.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::{Edge, Node};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A partition of graph data that can be serialized to cold storage.
///
/// Partitions are the unit of cold-tier I/O: an entire partition is written
/// or read as a single atomic operation. This simplifies consistency and
/// makes it easy to manage data lifecycle (e.g., delete old partitions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdPartition {
    /// A unique key identifying this partition (e.g., "2024-Q1", "tenant-42").
    pub partition_key: String,
    /// The node records in this partition.
    pub nodes: Vec<ColdNode>,
    /// The edge records in this partition.
    pub edges: Vec<ColdEdge>,
}

/// Node data in cold storage format.
///
/// This is a flattened, serialization-friendly representation of a graph node.
/// Unlike the in-memory [`Node`] type, it does not carry runtime indices or
/// adjacency pointers — those are rebuilt when data is hydrated back into the
/// warm/hot tiers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColdNode {
    /// The node's unique identifier.
    pub id: u64,
    /// Labels associated with this node.
    pub labels: Vec<String>,
    /// JSON-encoded property map.
    pub properties: serde_json::Value,
    /// Optional dense embedding vector for semantic search.
    pub embedding: Option<Vec<f32>>,
}

/// Edge data in cold storage format.
///
/// Captures all edge metadata including temporal validity and learnable
/// weights, suitable for long-term archival and later re-hydration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColdEdge {
    /// The edge's unique identifier.
    pub id: u64,
    /// Source node ID.
    pub source: u64,
    /// Target node ID.
    pub target: u64,
    /// The relationship type (e.g., "KNOWS", "WORKS_AT").
    pub edge_type: String,
    /// JSON-encoded property map.
    pub properties: serde_json::Value,
    /// Learnable weight for GNN / differentiable traversal.
    pub weight: f64,
    /// Start of temporal validity (epoch milliseconds), inclusive.
    pub valid_from: Option<i64>,
    /// End of temporal validity (epoch milliseconds), exclusive.
    pub valid_to: Option<i64>,
}

/// Trait for cold storage backends.
///
/// Implementations persist [`ColdPartition`] data to a durable store. The
/// backend must be `Send + Sync` so it can be shared across threads.
pub trait ColdStorage: Send + Sync {
    /// Write a partition to cold storage.
    ///
    /// If a partition with the same key already exists, it is overwritten.
    fn write_partition(&self, partition: &ColdPartition) -> Result<()>;

    /// Read a partition from cold storage by its key.
    ///
    /// Returns `Ok(None)` if no partition with the given key exists.
    fn read_partition(&self, partition_key: &str) -> Result<Option<ColdPartition>>;

    /// List all available partition keys in the store.
    fn list_partitions(&self) -> Result<Vec<String>>;

    /// Delete a partition by key.
    ///
    /// Returns `Ok(true)` if the partition was found and deleted, `Ok(false)`
    /// if it did not exist.
    fn delete_partition(&self, partition_key: &str) -> Result<bool>;
}

/// JSON-file-based cold storage backend (local filesystem).
///
/// Each partition is stored as a pretty-printed JSON file in a directory,
/// with the filename `{partition_key}.json`. This backend is simple and
/// human-readable, making it ideal for development, testing, and small
/// deployments. For production workloads with large graphs, a columnar
/// format like Apache Parquet would be more appropriate.
pub struct JsonFileColdStorage {
    base_dir: PathBuf,
}

impl JsonFileColdStorage {
    /// Create a new `JsonFileColdStorage` rooted at the given directory.
    ///
    /// The directory is created (including parents) if it does not exist.
    pub fn new(base_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(base_dir)?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    /// Compute the filesystem path for a partition key.
    fn partition_path(&self, key: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", key))
    }
}

impl ColdStorage for JsonFileColdStorage {
    fn write_partition(&self, partition: &ColdPartition) -> Result<()> {
        let path = self.partition_path(&partition.partition_key);
        let json = serde_json::to_vec_pretty(partition)
            .map_err(|e| AstraeaError::Serialization(e.to_string()))?;
        std::fs::write(path, json)?;
        Ok(())
    }

    fn read_partition(&self, key: &str) -> Result<Option<ColdPartition>> {
        let path = self.partition_path(key);
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read(path)?;
        let partition: ColdPartition = serde_json::from_slice(&data)
            .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;
        Ok(Some(partition))
    }

    fn list_partitions(&self) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some(stem) = path.file_stem() {
                    keys.push(stem.to_string_lossy().to_string());
                }
            }
        }
        Ok(keys)
    }

    fn delete_partition(&self, key: &str) -> Result<bool> {
        let path = self.partition_path(key);
        if path.exists() {
            std::fs::remove_file(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

// --- Conversion helpers ---

impl From<&Node> for ColdNode {
    fn from(n: &Node) -> Self {
        ColdNode {
            id: n.id.0,
            labels: n.labels.clone(),
            properties: n.properties.clone(),
            embedding: n.embedding.clone(),
        }
    }
}

impl From<&Edge> for ColdEdge {
    fn from(e: &Edge) -> Self {
        ColdEdge {
            id: e.id.0,
            source: e.source.0,
            target: e.target.0,
            edge_type: e.edge_type.clone(),
            properties: e.properties.clone(),
            weight: e.weight,
            valid_from: e.validity.valid_from,
            valid_to: e.validity.valid_to,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::types::{EdgeId, NodeId, ValidityInterval};
    use tempfile::TempDir;

    /// Helper: create a test node.
    fn test_node(id: u64) -> Node {
        Node {
            id: NodeId(id),
            labels: vec!["Person".to_string(), "Employee".to_string()],
            properties: serde_json::json!({"name": "Alice", "age": 30}),
            embedding: Some(vec![0.1, 0.2, 0.3, 0.4]),
        }
    }

    /// Helper: create a test edge with temporal validity.
    fn test_edge(id: u64, src: u64, tgt: u64) -> Edge {
        Edge {
            id: EdgeId(id),
            source: NodeId(src),
            target: NodeId(tgt),
            edge_type: "KNOWS".to_string(),
            properties: serde_json::json!({"since": 2024}),
            weight: 0.85,
            validity: ValidityInterval {
                valid_from: Some(1_700_000_000),
                valid_to: Some(1_800_000_000),
            },
        }
    }

    /// Helper: create a ColdPartition with sample data.
    fn sample_partition(key: &str) -> ColdPartition {
        let n1 = test_node(1);
        let n2 = test_node(2);
        let e1 = test_edge(100, 1, 2);

        ColdPartition {
            partition_key: key.to_string(),
            nodes: vec![ColdNode::from(&n1), ColdNode::from(&n2)],
            edges: vec![ColdEdge::from(&e1)],
        }
    }

    #[test]
    fn test_write_and_read_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonFileColdStorage::new(tmp.path()).unwrap();

        let partition = sample_partition("test-partition");
        storage.write_partition(&partition).unwrap();

        let read_back = storage
            .read_partition("test-partition")
            .unwrap()
            .expect("partition should exist");

        assert_eq!(read_back.partition_key, "test-partition");
        assert_eq!(read_back.nodes.len(), 2);
        assert_eq!(read_back.edges.len(), 1);

        // Verify node data roundtrips correctly.
        assert_eq!(read_back.nodes[0].id, 1);
        assert_eq!(read_back.nodes[0].labels, vec!["Person", "Employee"]);
        assert_eq!(read_back.nodes[0].embedding, Some(vec![0.1, 0.2, 0.3, 0.4]));

        // Verify edge data roundtrips correctly.
        assert_eq!(read_back.edges[0].source, 1);
        assert_eq!(read_back.edges[0].target, 2);
        assert_eq!(read_back.edges[0].edge_type, "KNOWS");
        assert_eq!(read_back.edges[0].weight, 0.85);
        assert_eq!(read_back.edges[0].valid_from, Some(1_700_000_000));
        assert_eq!(read_back.edges[0].valid_to, Some(1_800_000_000));
    }

    #[test]
    fn test_read_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonFileColdStorage::new(tmp.path()).unwrap();

        let result = storage.read_partition("does-not-exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_partitions() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonFileColdStorage::new(tmp.path()).unwrap();

        // Write three partitions.
        for key in ["alpha", "beta", "gamma"] {
            storage.write_partition(&sample_partition(key)).unwrap();
        }

        let mut keys = storage.list_partitions().unwrap();
        keys.sort(); // Filesystem order is not guaranteed.

        assert_eq!(keys.len(), 3);
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_delete_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonFileColdStorage::new(tmp.path()).unwrap();

        storage
            .write_partition(&sample_partition("to-delete"))
            .unwrap();

        // Partition exists before deletion.
        assert!(storage.read_partition("to-delete").unwrap().is_some());

        // Delete it.
        let deleted = storage.delete_partition("to-delete").unwrap();
        assert!(deleted);

        // Gone.
        assert!(storage.read_partition("to-delete").unwrap().is_none());

        // Deleting again returns false.
        let deleted_again = storage.delete_partition("to-delete").unwrap();
        assert!(!deleted_again);
    }

    #[test]
    fn test_conversion_from_node() {
        let node = test_node(42);
        let cold: ColdNode = ColdNode::from(&node);

        assert_eq!(cold.id, 42);
        assert_eq!(cold.labels, vec!["Person", "Employee"]);
        assert_eq!(cold.properties, serde_json::json!({"name": "Alice", "age": 30}));
        assert_eq!(cold.embedding, Some(vec![0.1, 0.2, 0.3, 0.4]));
    }

    #[test]
    fn test_conversion_from_edge() {
        let edge = test_edge(99, 10, 20);
        let cold: ColdEdge = ColdEdge::from(&edge);

        assert_eq!(cold.id, 99);
        assert_eq!(cold.source, 10);
        assert_eq!(cold.target, 20);
        assert_eq!(cold.edge_type, "KNOWS");
        assert_eq!(cold.properties, serde_json::json!({"since": 2024}));
        assert_eq!(cold.weight, 0.85);
        assert_eq!(cold.valid_from, Some(1_700_000_000));
        assert_eq!(cold.valid_to, Some(1_800_000_000));
    }

    #[test]
    fn test_conversion_from_node_no_embedding() {
        let node = Node {
            id: NodeId(7),
            labels: vec!["Thing".to_string()],
            properties: serde_json::json!({}),
            embedding: None,
        };
        let cold: ColdNode = ColdNode::from(&node);

        assert_eq!(cold.id, 7);
        assert!(cold.embedding.is_none());
    }

    #[test]
    fn test_conversion_from_edge_always_valid() {
        let edge = Edge {
            id: EdgeId(1),
            source: NodeId(2),
            target: NodeId(3),
            edge_type: "LINKS".to_string(),
            properties: serde_json::json!(null),
            weight: 1.0,
            validity: ValidityInterval::always(),
        };
        let cold: ColdEdge = ColdEdge::from(&edge);

        assert!(cold.valid_from.is_none());
        assert!(cold.valid_to.is_none());
    }

    #[test]
    fn test_overwrite_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = JsonFileColdStorage::new(tmp.path()).unwrap();

        // Write initial partition.
        let mut partition = sample_partition("mutable");
        storage.write_partition(&partition).unwrap();

        // Overwrite with modified data.
        partition.nodes.push(ColdNode {
            id: 999,
            labels: vec!["Extra".to_string()],
            properties: serde_json::json!({}),
            embedding: None,
        });
        storage.write_partition(&partition).unwrap();

        // Read back and verify the overwritten version.
        let read_back = storage
            .read_partition("mutable")
            .unwrap()
            .expect("partition should exist");
        assert_eq!(read_back.nodes.len(), 3); // 2 original + 1 added
        assert_eq!(read_back.nodes[2].id, 999);
    }
}

//! Object Store-based Cold Storage for AstraeaDB.
//!
//! This module provides an implementation of the [`ColdStorage`] trait backed by
//! the [`object_store`] crate, enabling storage of cold partitions to various
//! cloud object stores including:
//!
//! - **Amazon S3** (via [`object_store::aws::AmazonS3`])
//! - **Google Cloud Storage** (via [`object_store::gcp::GoogleCloudStorage`])
//! - **Azure Blob Storage** (via [`object_store::azure::MicrosoftAzure`])
//! - **Local filesystem** (via [`object_store::local::LocalFileSystem`])
//!
//! This fulfills the Tier 1 (Cold) storage requirement of AstraeaDB's tiered
//! architecture, allowing data to reside in cost-effective object storage
//! while still being accessible for hydration into warmer tiers.
//!
//! ## Path Format
//!
//! Partitions are stored as JSON files at the path `{prefix}{partition_key}.json`.
//! The prefix allows organizing multiple datasets within a single bucket.
//!
//! ## Example
//!
//! ```no_run
//! use astraea_storage::object_store_cold::ObjectStoreColdStorage;
//! use astraea_storage::cold_storage::{ColdStorage, ColdPartition};
//! use std::path::Path;
//!
//! # fn main() -> astraea_core::error::Result<()> {
//! // Create storage backed by local filesystem
//! let storage = ObjectStoreColdStorage::local(Path::new("/tmp/cold-storage"))?;
//!
//! // Write a partition
//! let partition = ColdPartition {
//!     partition_key: "2024-Q1".to_string(),
//!     nodes: vec![],
//!     edges: vec![],
//! };
//! storage.write_partition(&partition)?;
//!
//! // Read it back
//! let loaded = storage.read_partition("2024-Q1")?;
//! assert!(loaded.is_some());
//! # Ok(())
//! # }
//! ```

use crate::cold_storage::{ColdPartition, ColdStorage};
use astraea_core::error::{AstraeaError, Result};
use bytes::Bytes;
use futures::StreamExt;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use std::io::{Error as IoError, ErrorKind};
use std::path::Path;
use std::sync::Arc;

/// Convert an object_store error to an std::io::Error.
fn object_store_err_to_io(err: object_store::Error) -> IoError {
    IoError::new(ErrorKind::Other, err.to_string())
}

/// Object Store-based cold storage backend.
///
/// This implementation stores partitions as JSON files in any object store
/// supported by the `object_store` crate. It provides a unified interface
/// for local filesystem, S3, GCS, and Azure Blob Storage.
///
/// The storage is thread-safe (`Send + Sync`) and can be shared across
/// multiple threads for concurrent read/write operations.
pub struct ObjectStoreColdStorage {
    /// The underlying object store implementation.
    store: Arc<dyn ObjectStore>,
    /// Path prefix for all partitions (e.g., "cold-storage/").
    prefix: String,
}

impl ObjectStoreColdStorage {
    /// Create a new `ObjectStoreColdStorage` with a custom object store.
    ///
    /// # Arguments
    ///
    /// * `store` - An `Arc`-wrapped object store implementation.
    /// * `prefix` - Path prefix for partition files (e.g., "cold-storage/").
    ///              If empty, files are stored at the root.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use astraea_storage::object_store_cold::ObjectStoreColdStorage;
    /// use object_store::memory::InMemory;
    /// use std::sync::Arc;
    ///
    /// let store = Arc::new(InMemory::new());
    /// let storage = ObjectStoreColdStorage::new(store, "my-prefix/");
    /// ```
    pub fn new(store: Arc<dyn ObjectStore>, prefix: &str) -> Self {
        Self {
            store,
            prefix: prefix.to_string(),
        }
    }

    /// Create an `ObjectStoreColdStorage` backed by the local filesystem.
    ///
    /// This is a convenience constructor for local development and testing.
    /// The directory is created if it does not exist.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - The local filesystem directory to use for storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or accessed.
    pub fn local(base_dir: &Path) -> Result<Self> {
        // Create directory if it doesn't exist.
        std::fs::create_dir_all(base_dir)?;

        let store = LocalFileSystem::new_with_prefix(base_dir)
            .map_err(object_store_err_to_io)?;

        Ok(Self {
            store: Arc::new(store),
            prefix: String::new(),
        })
    }

    /// Create an `ObjectStoreColdStorage` backed by Amazon S3.
    ///
    /// This uses environment variables for credentials:
    /// - `AWS_ACCESS_KEY_ID`
    /// - `AWS_SECRET_ACCESS_KEY`
    /// - `AWS_DEFAULT_REGION` (optional)
    ///
    /// # Arguments
    ///
    /// * `bucket` - The S3 bucket name.
    /// * `prefix` - Path prefix within the bucket (e.g., "cold-storage/").
    ///
    /// # Errors
    ///
    /// Returns an error if the S3 client cannot be configured.
    pub fn s3(bucket: &str, prefix: &str) -> Result<Self> {
        use object_store::aws::AmazonS3Builder;

        let store = AmazonS3Builder::from_env()
            .with_bucket_name(bucket)
            .build()
            .map_err(object_store_err_to_io)?;

        Ok(Self {
            store: Arc::new(store),
            prefix: prefix.to_string(),
        })
    }

    /// Create an `ObjectStoreColdStorage` backed by Google Cloud Storage.
    ///
    /// This uses the `GOOGLE_APPLICATION_CREDENTIALS` environment variable
    /// or the default service account when running on GCP.
    ///
    /// # Arguments
    ///
    /// * `bucket` - The GCS bucket name.
    /// * `prefix` - Path prefix within the bucket (e.g., "cold-storage/").
    ///
    /// # Errors
    ///
    /// Returns an error if the GCS client cannot be configured.
    pub fn gcs(bucket: &str, prefix: &str) -> Result<Self> {
        use object_store::gcp::GoogleCloudStorageBuilder;

        let store = GoogleCloudStorageBuilder::from_env()
            .with_bucket_name(bucket)
            .build()
            .map_err(object_store_err_to_io)?;

        Ok(Self {
            store: Arc::new(store),
            prefix: prefix.to_string(),
        })
    }

    /// Create an `ObjectStoreColdStorage` backed by Azure Blob Storage.
    ///
    /// This uses environment variables for credentials:
    /// - `AZURE_STORAGE_ACCOUNT_NAME`
    /// - `AZURE_STORAGE_ACCOUNT_KEY` or `AZURE_STORAGE_SAS_TOKEN`
    ///
    /// # Arguments
    ///
    /// * `container` - The Azure container name.
    /// * `prefix` - Path prefix within the container (e.g., "cold-storage/").
    ///
    /// # Errors
    ///
    /// Returns an error if the Azure client cannot be configured.
    pub fn azure(container: &str, prefix: &str) -> Result<Self> {
        use object_store::azure::MicrosoftAzureBuilder;

        let store = MicrosoftAzureBuilder::from_env()
            .with_container_name(container)
            .build()
            .map_err(object_store_err_to_io)?;

        Ok(Self {
            store: Arc::new(store),
            prefix: prefix.to_string(),
        })
    }

    /// Compute the object store path for a partition key.
    fn partition_path(&self, key: &str) -> ObjectPath {
        let path_str = format!("{}{}.json", self.prefix, key);
        ObjectPath::from(path_str)
    }

    /// Extract partition key from an object path.
    fn key_from_path(&self, path: &ObjectPath) -> Option<String> {
        let path_str = path.as_ref();

        // Remove prefix if present.
        let without_prefix = if !self.prefix.is_empty() {
            path_str.strip_prefix(&self.prefix)?
        } else {
            path_str
        };

        // Remove .json extension.
        without_prefix.strip_suffix(".json").map(|s| s.to_string())
    }

    /// Run an async operation in a blocking context.
    ///
    /// This handles the case where we might already be in a tokio runtime
    /// (use `block_in_place`) or not (create a new runtime).
    fn block_on<F, T>(&self, future: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        // Try to use the existing runtime if available.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // We're inside a tokio runtime, use block_in_place.
            tokio::task::block_in_place(|| handle.block_on(future))
        } else {
            // No runtime, create a new one.
            tokio::runtime::Runtime::new()
                .expect("Failed to create tokio runtime")
                .block_on(future)
        }
    }
}

impl ColdStorage for ObjectStoreColdStorage {
    fn write_partition(&self, partition: &ColdPartition) -> Result<()> {
        let path = self.partition_path(&partition.partition_key);

        // Serialize to JSON.
        let json = serde_json::to_vec_pretty(partition)
            .map_err(|e| AstraeaError::Serialization(e.to_string()))?;

        let bytes = Bytes::from(json);

        // Write to object store.
        self.block_on(async {
            self.store
                .put(&path, bytes.into())
                .await
                .map_err(object_store_err_to_io)
        })?;

        Ok(())
    }

    fn read_partition(&self, partition_key: &str) -> Result<Option<ColdPartition>> {
        let path = self.partition_path(partition_key);

        // Try to read from object store.
        let result = self.block_on(async { self.store.get(&path).await });

        match result {
            Ok(get_result) => {
                // Read the bytes.
                let bytes = self.block_on(async {
                    get_result
                        .bytes()
                        .await
                        .map_err(object_store_err_to_io)
                })?;

                // Deserialize from JSON.
                let partition: ColdPartition = serde_json::from_slice(&bytes)
                    .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

                Ok(Some(partition))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(object_store_err_to_io(e).into()),
        }
    }

    fn list_partitions(&self) -> Result<Vec<String>> {
        let prefix_path = if self.prefix.is_empty() {
            None
        } else {
            Some(ObjectPath::from(self.prefix.clone()))
        };

        let keys = self.block_on(async {
            let mut keys = Vec::new();

            let mut stream = match &prefix_path {
                Some(p) => self.store.list(Some(p)),
                None => self.store.list(None),
            };

            while let Some(result) = stream.next().await {
                match result {
                    Ok(meta) => {
                        // Only include .json files.
                        if meta.location.as_ref().ends_with(".json") {
                            if let Some(key) = self.key_from_path(&meta.location) {
                                keys.push(key);
                            }
                        }
                    }
                    Err(e) => {
                        return Err(object_store_err_to_io(e));
                    }
                }
            }

            Ok::<_, IoError>(keys)
        })?;

        Ok(keys)
    }

    fn delete_partition(&self, partition_key: &str) -> Result<bool> {
        let path = self.partition_path(partition_key);

        // First check if it exists.
        let exists = self.block_on(async {
            match self.store.head(&path).await {
                Ok(_) => Ok(true),
                Err(object_store::Error::NotFound { .. }) => Ok(false),
                Err(e) => Err(object_store_err_to_io(e)),
            }
        })?;

        if !exists {
            return Ok(false);
        }

        // Delete the partition.
        self.block_on(async {
            self.store
                .delete(&path)
                .await
                .map_err(object_store_err_to_io)
        })?;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cold_storage::{ColdEdge, ColdNode};
    use tempfile::TempDir;

    /// Helper: create a sample partition for testing.
    fn sample_partition(key: &str) -> ColdPartition {
        ColdPartition {
            partition_key: key.to_string(),
            nodes: vec![
                ColdNode {
                    id: 1,
                    labels: vec!["Person".to_string()],
                    properties: serde_json::json!({"name": "Alice"}),
                    embedding: Some(vec![0.1, 0.2, 0.3]),
                },
                ColdNode {
                    id: 2,
                    labels: vec!["Person".to_string()],
                    properties: serde_json::json!({"name": "Bob"}),
                    embedding: None,
                },
            ],
            edges: vec![ColdEdge {
                id: 100,
                source: 1,
                target: 2,
                edge_type: "KNOWS".to_string(),
                properties: serde_json::json!({"since": 2020}),
                weight: 0.9,
                valid_from: Some(1_600_000_000),
                valid_to: None,
            }],
        }
    }

    #[test]
    fn test_write_and_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        let partition = sample_partition("test-roundtrip");
        storage.write_partition(&partition).unwrap();

        let read_back = storage
            .read_partition("test-roundtrip")
            .unwrap()
            .expect("partition should exist");

        assert_eq!(read_back.partition_key, "test-roundtrip");
        assert_eq!(read_back.nodes.len(), 2);
        assert_eq!(read_back.edges.len(), 1);

        // Verify node data.
        assert_eq!(read_back.nodes[0].id, 1);
        assert_eq!(read_back.nodes[0].labels, vec!["Person"]);
        assert_eq!(
            read_back.nodes[0].embedding,
            Some(vec![0.1f32, 0.2f32, 0.3f32])
        );

        // Verify edge data.
        assert_eq!(read_back.edges[0].source, 1);
        assert_eq!(read_back.edges[0].target, 2);
        assert_eq!(read_back.edges[0].weight, 0.9);
    }

    #[test]
    fn test_read_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        let result = storage.read_partition("does-not-exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_partitions() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        // Write multiple partitions.
        for key in ["alpha", "beta", "gamma"] {
            storage.write_partition(&sample_partition(key)).unwrap();
        }

        let mut keys = storage.list_partitions().unwrap();
        keys.sort();

        assert_eq!(keys.len(), 3);
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_delete_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        storage
            .write_partition(&sample_partition("to-delete"))
            .unwrap();

        // Verify it exists.
        assert!(storage.read_partition("to-delete").unwrap().is_some());

        // Delete it.
        let deleted = storage.delete_partition("to-delete").unwrap();
        assert!(deleted);

        // Verify it's gone.
        assert!(storage.read_partition("to-delete").unwrap().is_none());

        // Deleting again returns false.
        let deleted_again = storage.delete_partition("to-delete").unwrap();
        assert!(!deleted_again);
    }

    #[test]
    fn test_overwrite_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        // Write initial partition.
        let mut partition = sample_partition("overwrite-test");
        storage.write_partition(&partition).unwrap();

        // Verify initial state.
        let read1 = storage
            .read_partition("overwrite-test")
            .unwrap()
            .unwrap();
        assert_eq!(read1.nodes.len(), 2);

        // Overwrite with additional node.
        partition.nodes.push(ColdNode {
            id: 999,
            labels: vec!["Extra".to_string()],
            properties: serde_json::json!({}),
            embedding: None,
        });
        storage.write_partition(&partition).unwrap();

        // Verify overwritten state.
        let read2 = storage
            .read_partition("overwrite-test")
            .unwrap()
            .unwrap();
        assert_eq!(read2.nodes.len(), 3);
        assert_eq!(read2.nodes[2].id, 999);
    }

    #[test]
    fn test_with_prefix() {
        let tmp = TempDir::new().unwrap();

        // Create a LocalFileSystem store without prefix.
        let store = Arc::new(
            LocalFileSystem::new_with_prefix(tmp.path()).unwrap(),
        );

        // Use ObjectStoreColdStorage with a prefix.
        let storage = ObjectStoreColdStorage::new(store, "cold-data/");

        let partition = sample_partition("prefixed-partition");
        storage.write_partition(&partition).unwrap();

        // Verify it can be read back.
        let read_back = storage
            .read_partition("prefixed-partition")
            .unwrap()
            .expect("partition should exist");
        assert_eq!(read_back.partition_key, "prefixed-partition");

        // Verify the file is at the expected path.
        let expected_path = tmp.path().join("cold-data/prefixed-partition.json");
        assert!(expected_path.exists());
    }

    #[test]
    fn test_empty_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        let empty = ColdPartition {
            partition_key: "empty".to_string(),
            nodes: vec![],
            edges: vec![],
        };

        storage.write_partition(&empty).unwrap();

        let read_back = storage
            .read_partition("empty")
            .unwrap()
            .expect("partition should exist");

        assert_eq!(read_back.partition_key, "empty");
        assert!(read_back.nodes.is_empty());
        assert!(read_back.edges.is_empty());
    }

    #[test]
    fn test_list_empty_store() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        let keys = storage.list_partitions().unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_partition_key_with_special_characters() {
        let tmp = TempDir::new().unwrap();
        let storage = ObjectStoreColdStorage::local(tmp.path()).unwrap();

        // Keys with hyphens and underscores should work.
        let partition = sample_partition("tenant_42-2024-Q1");
        storage.write_partition(&partition).unwrap();

        let read_back = storage
            .read_partition("tenant_42-2024-Q1")
            .unwrap()
            .expect("partition should exist");
        assert_eq!(read_back.partition_key, "tenant_42-2024-Q1");
    }
}

//! Parquet-based cold storage backend for AstraeaDB.
//!
//! This module provides a cold storage implementation that uses Apache Parquet
//! as the underlying file format. Parquet is a columnar storage format that
//! offers excellent compression and efficient I/O for analytical workloads.
//!
//! Each partition is stored as two separate Parquet files:
//! - `{partition_key}_nodes.parquet` - Contains all node records
//! - `{partition_key}_edges.parquet` - Contains all edge records
//!
//! This separation allows for efficient reads when only nodes or edges are needed.

use crate::cold_storage::{ColdEdge, ColdNode, ColdPartition, ColdStorage};
use astraea_core::error::{AstraeaError, Result};
use arrow::array::{
    Array, ArrayRef, Float32Builder, Float64Array, Int64Array, ListBuilder, RecordBatch,
    StringBuilder, StringArray, UInt64Array,
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Parquet-file-based cold storage backend.
///
/// Each partition is stored as two Parquet files in the base directory:
/// - `{partition_key}_nodes.parquet` for node data
/// - `{partition_key}_edges.parquet` for edge data
///
/// This backend provides better compression and query performance compared
/// to JSON for large datasets, making it suitable for production workloads.
pub struct ParquetColdStorage {
    base_dir: PathBuf,
}

impl ParquetColdStorage {
    /// Create a new `ParquetColdStorage` rooted at the given directory.
    ///
    /// The directory is created (including parents) if it does not exist.
    pub fn new(base_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(base_dir)?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    /// Compute the filesystem path for a partition's nodes file.
    fn nodes_path(&self, key: &str) -> PathBuf {
        self.base_dir.join(format!("{}_nodes.parquet", key))
    }

    /// Compute the filesystem path for a partition's edges file.
    fn edges_path(&self, key: &str) -> PathBuf {
        self.base_dir.join(format!("{}_edges.parquet", key))
    }

    /// Build the Arrow schema for node records.
    fn node_schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::UInt64, false),
            Field::new(
                "labels",
                DataType::List(Arc::new(Field::new_list_field(DataType::Utf8, true))),
                false,
            ),
            Field::new("properties", DataType::Utf8, false),
            Field::new(
                "embedding",
                DataType::List(Arc::new(Field::new_list_field(DataType::Float32, true))),
                true,
            ),
        ])
    }

    /// Build the Arrow schema for edge records.
    fn edge_schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::UInt64, false),
            Field::new("source", DataType::UInt64, false),
            Field::new("target", DataType::UInt64, false),
            Field::new("edge_type", DataType::Utf8, false),
            Field::new("properties", DataType::Utf8, false),
            Field::new("weight", DataType::Float64, false),
            Field::new("valid_from", DataType::Int64, true),
            Field::new("valid_to", DataType::Int64, true),
        ])
    }

    /// Convert a slice of `ColdNode` to an Arrow `RecordBatch`.
    fn nodes_to_record_batch(nodes: &[ColdNode]) -> Result<RecordBatch> {
        let schema = Arc::new(Self::node_schema());

        // Build id array
        let ids: Vec<u64> = nodes.iter().map(|n| n.id).collect();
        let id_array = Arc::new(UInt64Array::from(ids)) as ArrayRef;

        // Build labels array (List<Utf8>)
        let mut labels_builder = ListBuilder::new(StringBuilder::new());
        for node in nodes {
            for label in &node.labels {
                labels_builder.values().append_value(label);
            }
            labels_builder.append(true);
        }
        let labels_array = Arc::new(labels_builder.finish()) as ArrayRef;

        // Build properties array (JSON string)
        let properties: Vec<String> = nodes
            .iter()
            .map(|n| n.properties.to_string())
            .collect();
        let properties_array = Arc::new(StringArray::from(properties)) as ArrayRef;

        // Build embedding array (List<Float32>, nullable)
        let mut embedding_builder = ListBuilder::new(Float32Builder::new());
        for node in nodes {
            match &node.embedding {
                Some(emb) => {
                    for val in emb {
                        embedding_builder.values().append_value(*val);
                    }
                    embedding_builder.append(true);
                }
                None => {
                    embedding_builder.append(false);
                }
            }
        }
        let embedding_array = Arc::new(embedding_builder.finish()) as ArrayRef;

        RecordBatch::try_new(schema, vec![id_array, labels_array, properties_array, embedding_array])
            .map_err(|e| AstraeaError::Serialization(e.to_string()))
    }

    /// Convert a slice of `ColdEdge` to an Arrow `RecordBatch`.
    fn edges_to_record_batch(edges: &[ColdEdge]) -> Result<RecordBatch> {
        let schema = Arc::new(Self::edge_schema());

        let ids: Vec<u64> = edges.iter().map(|e| e.id).collect();
        let id_array = Arc::new(UInt64Array::from(ids)) as ArrayRef;

        let sources: Vec<u64> = edges.iter().map(|e| e.source).collect();
        let source_array = Arc::new(UInt64Array::from(sources)) as ArrayRef;

        let targets: Vec<u64> = edges.iter().map(|e| e.target).collect();
        let target_array = Arc::new(UInt64Array::from(targets)) as ArrayRef;

        let edge_types: Vec<&str> = edges.iter().map(|e| e.edge_type.as_str()).collect();
        let edge_type_array = Arc::new(StringArray::from(edge_types)) as ArrayRef;

        let properties: Vec<String> = edges.iter().map(|e| e.properties.to_string()).collect();
        let properties_array = Arc::new(StringArray::from(properties)) as ArrayRef;

        let weights: Vec<f64> = edges.iter().map(|e| e.weight).collect();
        let weight_array = Arc::new(Float64Array::from(weights)) as ArrayRef;

        let valid_froms: Vec<Option<i64>> = edges.iter().map(|e| e.valid_from).collect();
        let valid_from_array = Arc::new(Int64Array::from(valid_froms)) as ArrayRef;

        let valid_tos: Vec<Option<i64>> = edges.iter().map(|e| e.valid_to).collect();
        let valid_to_array = Arc::new(Int64Array::from(valid_tos)) as ArrayRef;

        RecordBatch::try_new(
            schema,
            vec![
                id_array,
                source_array,
                target_array,
                edge_type_array,
                properties_array,
                weight_array,
                valid_from_array,
                valid_to_array,
            ],
        )
        .map_err(|e| AstraeaError::Serialization(e.to_string()))
    }

    /// Convert an Arrow `RecordBatch` back to `ColdNode` records.
    fn record_batch_to_nodes(batch: &RecordBatch) -> Result<Vec<ColdNode>> {
        let num_rows = batch.num_rows();
        let mut nodes = Vec::with_capacity(num_rows);

        let id_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid id column".to_string()))?;

        let labels_array = batch
            .column(1)
            .as_any()
            .downcast_ref::<arrow::array::ListArray>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid labels column".to_string()))?;

        let properties_array = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid properties column".to_string()))?;

        let embedding_array = batch
            .column(3)
            .as_any()
            .downcast_ref::<arrow::array::ListArray>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid embedding column".to_string()))?;

        for i in 0..num_rows {
            let id = id_array.value(i);

            // Extract labels
            let labels_list = labels_array.value(i);
            let labels_str_array = labels_list
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    AstraeaError::Deserialization("Invalid labels inner array".to_string())
                })?;
            let labels: Vec<String> = (0..labels_str_array.len())
                .map(|j| labels_str_array.value(j).to_string())
                .collect();

            // Extract properties
            let properties_str = properties_array.value(i);
            let properties: serde_json::Value = serde_json::from_str(properties_str)
                .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

            // Extract embedding (nullable)
            let embedding = if embedding_array.is_null(i) {
                None
            } else {
                let emb_list = embedding_array.value(i);
                let emb_f32_array = emb_list
                    .as_any()
                    .downcast_ref::<arrow::array::Float32Array>()
                    .ok_or_else(|| {
                        AstraeaError::Deserialization("Invalid embedding inner array".to_string())
                    })?;
                Some((0..emb_f32_array.len()).map(|j| emb_f32_array.value(j)).collect())
            };

            nodes.push(ColdNode {
                id,
                labels,
                properties,
                embedding,
            });
        }

        Ok(nodes)
    }

    /// Convert an Arrow `RecordBatch` back to `ColdEdge` records.
    fn record_batch_to_edges(batch: &RecordBatch) -> Result<Vec<ColdEdge>> {
        let num_rows = batch.num_rows();
        let mut edges = Vec::with_capacity(num_rows);

        let id_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid id column".to_string()))?;

        let source_array = batch
            .column(1)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid source column".to_string()))?;

        let target_array = batch
            .column(2)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid target column".to_string()))?;

        let edge_type_array = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid edge_type column".to_string()))?;

        let properties_array = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid properties column".to_string()))?;

        let weight_array = batch
            .column(5)
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid weight column".to_string()))?;

        let valid_from_array = batch
            .column(6)
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid valid_from column".to_string()))?;

        let valid_to_array = batch
            .column(7)
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| AstraeaError::Deserialization("Invalid valid_to column".to_string()))?;

        for i in 0..num_rows {
            let properties_str = properties_array.value(i);
            let properties: serde_json::Value = serde_json::from_str(properties_str)
                .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

            edges.push(ColdEdge {
                id: id_array.value(i),
                source: source_array.value(i),
                target: target_array.value(i),
                edge_type: edge_type_array.value(i).to_string(),
                properties,
                weight: weight_array.value(i),
                valid_from: if valid_from_array.is_null(i) {
                    None
                } else {
                    Some(valid_from_array.value(i))
                },
                valid_to: if valid_to_array.is_null(i) {
                    None
                } else {
                    Some(valid_to_array.value(i))
                },
            });
        }

        Ok(edges)
    }

    /// Write a RecordBatch to a Parquet file.
    fn write_parquet(path: &Path, batch: &RecordBatch) -> Result<()> {
        let file = File::create(path)?;
        let mut writer = ArrowWriter::try_new(file, batch.schema(), None)
            .map_err(|e| AstraeaError::Serialization(e.to_string()))?;
        writer
            .write(batch)
            .map_err(|e| AstraeaError::Serialization(e.to_string()))?;
        writer
            .close()
            .map_err(|e| AstraeaError::Serialization(e.to_string()))?;
        Ok(())
    }

    /// Read all RecordBatches from a Parquet file and concatenate them.
    fn read_parquet(path: &Path) -> Result<Option<RecordBatch>> {
        if !path.exists() {
            return Ok(None);
        }

        let file = File::open(path)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)
            .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;
        let reader = builder
            .build()
            .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

        let batches: Vec<RecordBatch> = reader
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

        if batches.is_empty() {
            return Ok(None);
        }

        // Concatenate all batches into one
        let schema = batches[0].schema();
        let concatenated = arrow::compute::concat_batches(&schema, &batches)
            .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

        Ok(Some(concatenated))
    }
}

impl ColdStorage for ParquetColdStorage {
    fn write_partition(&self, partition: &ColdPartition) -> Result<()> {
        let nodes_path = self.nodes_path(&partition.partition_key);
        let edges_path = self.edges_path(&partition.partition_key);

        // Write nodes
        let nodes_batch = Self::nodes_to_record_batch(&partition.nodes)?;
        Self::write_parquet(&nodes_path, &nodes_batch)?;

        // Write edges
        let edges_batch = Self::edges_to_record_batch(&partition.edges)?;
        Self::write_parquet(&edges_path, &edges_batch)?;

        Ok(())
    }

    fn read_partition(&self, partition_key: &str) -> Result<Option<ColdPartition>> {
        let nodes_path = self.nodes_path(partition_key);
        let edges_path = self.edges_path(partition_key);

        // Check if either file exists
        if !nodes_path.exists() && !edges_path.exists() {
            return Ok(None);
        }

        // Read nodes
        let nodes = match Self::read_parquet(&nodes_path)? {
            Some(batch) => Self::record_batch_to_nodes(&batch)?,
            None => Vec::new(),
        };

        // Read edges
        let edges = match Self::read_parquet(&edges_path)? {
            Some(batch) => Self::record_batch_to_edges(&batch)?,
            None => Vec::new(),
        };

        Ok(Some(ColdPartition {
            partition_key: partition_key.to_string(),
            nodes,
            edges,
        }))
    }

    fn list_partitions(&self) -> Result<Vec<String>> {
        let mut keys = std::collections::HashSet::new();

        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "parquet") {
                if let Some(stem) = path.file_stem() {
                    let stem_str = stem.to_string_lossy();
                    // Remove _nodes or _edges suffix to get partition key
                    if let Some(key) = stem_str.strip_suffix("_nodes") {
                        keys.insert(key.to_string());
                    } else if let Some(key) = stem_str.strip_suffix("_edges") {
                        keys.insert(key.to_string());
                    }
                }
            }
        }

        Ok(keys.into_iter().collect())
    }

    fn delete_partition(&self, partition_key: &str) -> Result<bool> {
        let nodes_path = self.nodes_path(partition_key);
        let edges_path = self.edges_path(partition_key);

        let mut deleted = false;

        if nodes_path.exists() {
            std::fs::remove_file(nodes_path)?;
            deleted = true;
        }

        if edges_path.exists() {
            std::fs::remove_file(edges_path)?;
            deleted = true;
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::types::{Edge, EdgeId, Node, NodeId, ValidityInterval};
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

    /// Helper: create a test node without embedding.
    fn test_node_no_embedding(id: u64) -> Node {
        Node {
            id: NodeId(id),
            labels: vec!["Thing".to_string()],
            properties: serde_json::json!({"key": "value"}),
            embedding: None,
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

    /// Helper: create a test edge without temporal validity.
    fn test_edge_always_valid(id: u64, src: u64, tgt: u64) -> Edge {
        Edge {
            id: EdgeId(id),
            source: NodeId(src),
            target: NodeId(tgt),
            edge_type: "LINKS".to_string(),
            properties: serde_json::json!(null),
            weight: 1.0,
            validity: ValidityInterval::always(),
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
    fn test_parquet_write_and_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

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
        assert_eq!(
            read_back.nodes[0].properties,
            serde_json::json!({"name": "Alice", "age": 30})
        );

        // Verify edge data roundtrips correctly.
        assert_eq!(read_back.edges[0].id, 100);
        assert_eq!(read_back.edges[0].source, 1);
        assert_eq!(read_back.edges[0].target, 2);
        assert_eq!(read_back.edges[0].edge_type, "KNOWS");
        assert_eq!(read_back.edges[0].weight, 0.85);
        assert_eq!(read_back.edges[0].valid_from, Some(1_700_000_000));
        assert_eq!(read_back.edges[0].valid_to, Some(1_800_000_000));
        assert_eq!(
            read_back.edges[0].properties,
            serde_json::json!({"since": 2024})
        );
    }

    #[test]
    fn test_parquet_empty_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

        let partition = ColdPartition {
            partition_key: "empty".to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        };

        storage.write_partition(&partition).unwrap();

        let read_back = storage
            .read_partition("empty")
            .unwrap()
            .expect("partition should exist");

        assert_eq!(read_back.partition_key, "empty");
        assert!(read_back.nodes.is_empty());
        assert!(read_back.edges.is_empty());
    }

    #[test]
    fn test_parquet_multiple_partitions() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

        // Write three partitions.
        for key in ["alpha", "beta", "gamma"] {
            storage.write_partition(&sample_partition(key)).unwrap();
        }

        let mut keys = storage.list_partitions().unwrap();
        keys.sort(); // Filesystem order is not guaranteed.

        assert_eq!(keys.len(), 3);
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);

        // Verify each partition can be read back
        for key in ["alpha", "beta", "gamma"] {
            let partition = storage.read_partition(key).unwrap().unwrap();
            assert_eq!(partition.partition_key, key);
            assert_eq!(partition.nodes.len(), 2);
            assert_eq!(partition.edges.len(), 1);
        }
    }

    #[test]
    fn test_parquet_node_data_integrity() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

        // Create nodes with varying data
        let n1 = test_node(1);
        let n2 = test_node_no_embedding(2);
        let n3 = Node {
            id: NodeId(3),
            labels: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            properties: serde_json::json!({
                "nested": {"key": "value"},
                "array": [1, 2, 3],
                "number": 42.5
            }),
            embedding: Some(vec![1.0, 2.0, 3.0, 4.0, 5.0]),
        };

        let partition = ColdPartition {
            partition_key: "integrity".to_string(),
            nodes: vec![ColdNode::from(&n1), ColdNode::from(&n2), ColdNode::from(&n3)],
            edges: Vec::new(),
        };

        storage.write_partition(&partition).unwrap();

        let read_back = storage.read_partition("integrity").unwrap().unwrap();

        // Node 1: with embedding
        assert_eq!(read_back.nodes[0].id, 1);
        assert_eq!(read_back.nodes[0].labels, vec!["Person", "Employee"]);
        assert_eq!(read_back.nodes[0].embedding, Some(vec![0.1, 0.2, 0.3, 0.4]));

        // Node 2: without embedding
        assert_eq!(read_back.nodes[1].id, 2);
        assert_eq!(read_back.nodes[1].labels, vec!["Thing"]);
        assert!(read_back.nodes[1].embedding.is_none());

        // Node 3: complex properties
        assert_eq!(read_back.nodes[2].id, 3);
        assert_eq!(read_back.nodes[2].labels, vec!["A", "B", "C"]);
        assert_eq!(read_back.nodes[2].embedding, Some(vec![1.0, 2.0, 3.0, 4.0, 5.0]));
        assert_eq!(
            read_back.nodes[2].properties,
            serde_json::json!({
                "nested": {"key": "value"},
                "array": [1, 2, 3],
                "number": 42.5
            })
        );
    }

    #[test]
    fn test_parquet_edge_data_integrity() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

        // Create edges with varying data
        let e1 = test_edge(100, 1, 2);
        let e2 = test_edge_always_valid(200, 3, 4);
        let e3 = Edge {
            id: EdgeId(300),
            source: NodeId(5),
            target: NodeId(6),
            edge_type: "COMPLEX_RELATION".to_string(),
            properties: serde_json::json!({
                "metadata": {"created": "2024-01-01"},
                "tags": ["important", "verified"]
            }),
            weight: -0.5,
            validity: ValidityInterval {
                valid_from: Some(0),
                valid_to: None,
            },
        };

        let partition = ColdPartition {
            partition_key: "edge-integrity".to_string(),
            nodes: Vec::new(),
            edges: vec![
                ColdEdge::from(&e1),
                ColdEdge::from(&e2),
                ColdEdge::from(&e3),
            ],
        };

        storage.write_partition(&partition).unwrap();

        let read_back = storage.read_partition("edge-integrity").unwrap().unwrap();

        // Edge 1: with temporal validity
        assert_eq!(read_back.edges[0].id, 100);
        assert_eq!(read_back.edges[0].source, 1);
        assert_eq!(read_back.edges[0].target, 2);
        assert_eq!(read_back.edges[0].edge_type, "KNOWS");
        assert_eq!(read_back.edges[0].weight, 0.85);
        assert_eq!(read_back.edges[0].valid_from, Some(1_700_000_000));
        assert_eq!(read_back.edges[0].valid_to, Some(1_800_000_000));

        // Edge 2: always valid (no temporal bounds)
        assert_eq!(read_back.edges[1].id, 200);
        assert_eq!(read_back.edges[1].edge_type, "LINKS");
        assert_eq!(read_back.edges[1].weight, 1.0);
        assert!(read_back.edges[1].valid_from.is_none());
        assert!(read_back.edges[1].valid_to.is_none());

        // Edge 3: partial temporal validity and complex properties
        assert_eq!(read_back.edges[2].id, 300);
        assert_eq!(read_back.edges[2].edge_type, "COMPLEX_RELATION");
        assert_eq!(read_back.edges[2].weight, -0.5);
        assert_eq!(read_back.edges[2].valid_from, Some(0));
        assert!(read_back.edges[2].valid_to.is_none());
        assert_eq!(
            read_back.edges[2].properties,
            serde_json::json!({
                "metadata": {"created": "2024-01-01"},
                "tags": ["important", "verified"]
            })
        );
    }

    #[test]
    fn test_parquet_read_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

        let result = storage.read_partition("does-not-exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parquet_delete_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

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
    fn test_parquet_overwrite_partition() {
        let tmp = TempDir::new().unwrap();
        let storage = ParquetColdStorage::new(tmp.path()).unwrap();

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

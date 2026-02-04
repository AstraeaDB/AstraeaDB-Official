//! StorageEngine implementation that ties together the FileManager, BufferPool,
//! and WAL to provide a complete disk-backed storage engine for nodes and edges.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::StorageEngine;
use astraea_core::types::*;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buffer_pool::BufferPool;
use crate::file_manager::FileManager;
use crate::page::*;
use crate::wal::{WalRecord, WalWriter};

/// Default buffer pool size (number of page frames).
const DEFAULT_POOL_SIZE: usize = 1024;

/// Serialized node data stored in a page (properties + embedding + labels).
#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedNode {
    labels: Vec<String>,
    properties: serde_json::Value,
    embedding: Option<Vec<f32>>,
}

/// Serialized edge data stored in a page.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedEdge {
    source: u64,
    target: u64,
    edge_type: String,
    properties: serde_json::Value,
    weight: f64,
    validity: ValidityInterval,
}

/// In-memory adjacency index for quick edge lookups by node.
struct AdjacencyIndex {
    /// node_id -> list of (edge_id, direction=outgoing)
    outgoing: HashMap<NodeId, Vec<EdgeId>>,
    /// node_id -> list of (edge_id, direction=incoming)
    incoming: HashMap<NodeId, Vec<EdgeId>>,
}

impl AdjacencyIndex {
    fn new() -> Self {
        Self {
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
        }
    }

    fn add_edge(&mut self, edge_id: EdgeId, source: NodeId, target: NodeId) {
        self.outgoing
            .entry(source)
            .or_default()
            .push(edge_id);
        self.incoming
            .entry(target)
            .or_default()
            .push(edge_id);
    }

    fn remove_edge(&mut self, edge_id: EdgeId, source: NodeId, target: NodeId) {
        if let Some(edges) = self.outgoing.get_mut(&source) {
            edges.retain(|e| *e != edge_id);
        }
        if let Some(edges) = self.incoming.get_mut(&target) {
            edges.retain(|e| *e != edge_id);
        }
    }

    fn get_edges(&self, node_id: NodeId, direction: Direction) -> Vec<EdgeId> {
        match direction {
            Direction::Outgoing => self
                .outgoing
                .get(&node_id)
                .cloned()
                .unwrap_or_default(),
            Direction::Incoming => self
                .incoming
                .get(&node_id)
                .cloned()
                .unwrap_or_default(),
            Direction::Both => {
                let mut result = self
                    .outgoing
                    .get(&node_id)
                    .cloned()
                    .unwrap_or_default();
                if let Some(incoming) = self.incoming.get(&node_id) {
                    result.extend(incoming);
                }
                result
            }
        }
    }
}

/// Disk-backed storage engine for AstraeaDB.
///
/// Uses a page-based file format, a buffer pool for caching, and a WAL for
/// durability. An in-memory index maps NodeId/EdgeId to PageId for quick lookups.
pub struct DiskStorageEngine {
    #[allow(dead_code)]
    file_manager: Arc<FileManager>,
    buffer_pool: BufferPool,
    wal: WalWriter,

    /// In-memory index: NodeId -> PageId where the node record lives.
    node_index: RwLock<HashMap<NodeId, PageId>>,
    /// In-memory index: EdgeId -> PageId where the edge record lives.
    edge_index: RwLock<HashMap<EdgeId, PageId>>,
    /// In-memory adjacency index for edge lookups by node.
    adjacency: RwLock<AdjacencyIndex>,

    /// Path to the data directory (for diagnostics).
    #[allow(dead_code)]
    data_dir: PathBuf,
}

impl DiskStorageEngine {
    /// Create or open a storage engine at the given directory path.
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Result<Self> {
        Self::with_pool_size(data_dir, DEFAULT_POOL_SIZE)
    }

    /// Create or open a storage engine with a custom buffer pool size.
    pub fn with_pool_size<P: AsRef<Path>>(data_dir: P, pool_size: usize) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;

        let db_path = data_dir.join("astraea.db");
        let wal_path = data_dir.join("astraea.wal");

        let file_manager = Arc::new(FileManager::new(&db_path)?);
        let buffer_pool = BufferPool::new(Arc::clone(&file_manager), pool_size);
        let wal = WalWriter::new(&wal_path)?;

        Ok(Self {
            file_manager,
            buffer_pool,
            wal,
            node_index: RwLock::new(HashMap::new()),
            edge_index: RwLock::new(HashMap::new()),
            adjacency: RwLock::new(AdjacencyIndex::new()),
            data_dir,
        })
    }

    /// Serialize a node into bytes for storage in a page.
    fn serialize_node(node: &Node) -> Result<Vec<u8>> {
        let sn = SerializedNode {
            labels: node.labels.clone(),
            properties: node.properties.clone(),
            embedding: node.embedding.clone(),
        };
        serde_json::to_vec(&sn).map_err(|e| AstraeaError::Serialization(e.to_string()))
    }

    /// Deserialize a node from bytes.
    fn deserialize_node(id: NodeId, data: &[u8]) -> Result<Node> {
        let sn: SerializedNode =
            serde_json::from_slice(data).map_err(|e| AstraeaError::Deserialization(e.to_string()))?;
        Ok(Node {
            id,
            labels: sn.labels,
            properties: sn.properties,
            embedding: sn.embedding,
        })
    }

    /// Serialize an edge into bytes for storage in a page.
    fn serialize_edge(edge: &Edge) -> Result<Vec<u8>> {
        let se = SerializedEdge {
            source: edge.source.0,
            target: edge.target.0,
            edge_type: edge.edge_type.clone(),
            properties: edge.properties.clone(),
            weight: edge.weight,
            validity: edge.validity,
        };
        serde_json::to_vec(&se).map_err(|e| AstraeaError::Serialization(e.to_string()))
    }

    /// Deserialize an edge from bytes.
    fn deserialize_edge(id: EdgeId, data: &[u8]) -> Result<Edge> {
        let se: SerializedEdge =
            serde_json::from_slice(data).map_err(|e| AstraeaError::Deserialization(e.to_string()))?;
        Ok(Edge {
            id,
            source: NodeId(se.source),
            target: NodeId(se.target),
            edge_type: se.edge_type,
            properties: se.properties,
            weight: se.weight,
            validity: se.validity,
        })
    }

    /// Write a record (node or edge) into a page. Allocates a new page if needed.
    /// Returns the page ID where the record was written.
    fn write_record(
        &self,
        record_id: u64,
        data: &[u8],
        page_type: PageType,
    ) -> Result<PageId> {
        let total_size = NODE_RECORD_HEADER_SIZE + data.len();

        // Try to find an existing page with enough space, or allocate a new one.
        let page_buf = init_page(PageId(0), page_type);
        let guard = self.buffer_pool.pin_new_page(&page_buf)?;
        let page_id = guard.page_id();

        // Read the current page data.
        let mut buf = guard.data().0;

        // Update the page header with the correct page_id.
        let mut header = PageHeader::read_from(&buf)?;
        header.page_id = page_id;

        // Check space.
        let free = header.free_space();
        if total_size > free {
            self.buffer_pool.unpin_page(page_id, false)?;
            return Err(AstraeaError::Serialization(format!(
                "record too large for a single page: {} bytes, free: {}",
                total_size, free
            )));
        }

        // Write the record header.
        let offset = header.free_space_offset as usize;
        let rec_header = NodeRecordHeader {
            node_id: record_id,
            data_len: data.len() as u32,
            adjacency_offset: 0, // No adjacency stored inline for now.
        };
        rec_header.write_to(&mut buf, offset);

        // Write the record data.
        let data_offset = offset + NODE_RECORD_HEADER_SIZE;
        buf[data_offset..data_offset + data.len()].copy_from_slice(data);

        // Update header.
        header.record_count += 1;
        header.free_space_offset = (data_offset + data.len()) as u16;
        header.checksum = 0;
        header.write_to(&mut buf);
        let checksum = compute_page_checksum(&buf);
        header.checksum = checksum;
        header.write_to(&mut buf);

        // Write back through the guard.
        guard.write_data(&buf);
        self.buffer_pool.unpin_page(page_id, true)?;

        Ok(page_id)
    }

    /// Read a record from a specific page by its record ID.
    fn read_record(&self, page_id: PageId, record_id: u64) -> Result<Option<Vec<u8>>> {
        let guard = self.buffer_pool.pin_page(page_id)?;
        let buf = guard.data();

        let header = PageHeader::read_from(&buf)?;
        let mut offset = PAGE_HEADER_SIZE;

        for _ in 0..header.record_count {
            let rec = NodeRecordHeader::read_from(&buf, offset);
            let data_offset = offset + NODE_RECORD_HEADER_SIZE;
            let data_end = data_offset + rec.data_len as usize;

            if rec.node_id == record_id {
                let data = buf[data_offset..data_end].to_vec();
                self.buffer_pool.unpin_page(page_id, false)?;
                return Ok(Some(data));
            }

            offset = data_end;
        }

        self.buffer_pool.unpin_page(page_id, false)?;
        Ok(None)
    }
}

impl StorageEngine for DiskStorageEngine {
    fn put_node(&self, node: &Node) -> Result<()> {
        // Log to WAL first.
        self.wal.append(&WalRecord::InsertNode(node.clone()))?;

        // Serialize.
        let data = Self::serialize_node(node)?;

        // Check if this node already exists (update case).
        // For simplicity, we always write to a new page and update the index.
        // A more sophisticated engine would do in-place updates when the record fits.
        let page_id = self.write_record(node.id.0, &data, PageType::NodePage)?;

        // Update the in-memory index.
        let mut index = self.node_index.write();
        index.insert(node.id, page_id);

        Ok(())
    }

    fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        let index = self.node_index.read();
        let page_id = match index.get(&id) {
            Some(&pid) => pid,
            None => return Ok(None),
        };
        drop(index);

        let data = self.read_record(page_id, id.0)?;
        match data {
            Some(bytes) => Ok(Some(Self::deserialize_node(id, &bytes)?)),
            None => Ok(None),
        }
    }

    fn delete_node(&self, id: NodeId) -> Result<bool> {
        // Log to WAL.
        self.wal.append(&WalRecord::DeleteNode(id))?;

        let mut index = self.node_index.write();
        let existed = index.remove(&id).is_some();
        // Note: we don't reclaim the page space here. A compaction process
        // would handle that in a production system.
        Ok(existed)
    }

    fn put_edge(&self, edge: &Edge) -> Result<()> {
        // Log to WAL.
        self.wal.append(&WalRecord::InsertEdge(edge.clone()))?;

        // Serialize.
        let data = Self::serialize_edge(edge)?;

        let page_id = self.write_record(edge.id.0, &data, PageType::EdgePage)?;

        // Update indices.
        {
            let mut index = self.edge_index.write();
            index.insert(edge.id, page_id);
        }
        {
            let mut adj = self.adjacency.write();
            adj.add_edge(edge.id, edge.source, edge.target);
        }

        Ok(())
    }

    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
        let index = self.edge_index.read();
        let page_id = match index.get(&id) {
            Some(&pid) => pid,
            None => return Ok(None),
        };
        drop(index);

        let data = self.read_record(page_id, id.0)?;
        match data {
            Some(bytes) => Ok(Some(Self::deserialize_edge(id, &bytes)?)),
            None => Ok(None),
        }
    }

    fn delete_edge(&self, id: EdgeId) -> Result<bool> {
        // We need the edge data to update adjacency.
        let edge = self.get_edge(id)?;

        // Log to WAL.
        self.wal.append(&WalRecord::DeleteEdge(id))?;

        let mut index = self.edge_index.write();
        let existed = index.remove(&id).is_some();

        if let Some(edge) = edge {
            let mut adj = self.adjacency.write();
            adj.remove_edge(id, edge.source, edge.target);
        }

        Ok(existed)
    }

    fn get_edges(&self, node_id: NodeId, direction: Direction) -> Result<Vec<Edge>> {
        let adj = self.adjacency.read();
        let edge_ids = adj.get_edges(node_id, direction);
        drop(adj);

        let mut edges = Vec::new();
        for eid in edge_ids {
            if let Some(edge) = self.get_edge(eid)? {
                edges.push(edge);
            }
        }
        Ok(edges)
    }

    fn flush(&self) -> Result<()> {
        self.buffer_pool.flush_all()?;
        // Log a checkpoint.
        let lsn = self.wal.current_lsn();
        self.wal.append(&WalRecord::Checkpoint(lsn.0))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_engine() -> (DiskStorageEngine, TempDir) {
        let tmp = TempDir::new().unwrap();
        let engine = DiskStorageEngine::with_pool_size(tmp.path(), 64).unwrap();
        (engine, tmp)
    }

    fn test_node(id: u64) -> Node {
        Node {
            id: NodeId(id),
            labels: vec!["Person".to_string()],
            properties: serde_json::json!({"name": "Test", "id": id}),
            embedding: Some(vec![0.1, 0.2, 0.3]),
        }
    }

    fn test_edge(id: u64, src: u64, tgt: u64) -> Edge {
        Edge {
            id: EdgeId(id),
            source: NodeId(src),
            target: NodeId(tgt),
            edge_type: "KNOWS".to_string(),
            properties: serde_json::json!({"since": 2024}),
            weight: 1.0,
            validity: ValidityInterval::always(),
        }
    }

    #[test]
    fn test_put_get_node() {
        let (engine, _tmp) = make_engine();
        let node = test_node(1);

        engine.put_node(&node).unwrap();
        let retrieved = engine.get_node(NodeId(1)).unwrap().unwrap();
        assert_eq!(retrieved.id, NodeId(1));
        assert_eq!(retrieved.labels, vec!["Person".to_string()]);
        assert_eq!(retrieved.embedding, Some(vec![0.1, 0.2, 0.3]));
    }

    #[test]
    fn test_delete_node() {
        let (engine, _tmp) = make_engine();
        engine.put_node(&test_node(1)).unwrap();

        assert!(engine.delete_node(NodeId(1)).unwrap());
        assert!(!engine.delete_node(NodeId(1)).unwrap()); // Already deleted.
        assert!(engine.get_node(NodeId(1)).unwrap().is_none());
    }

    #[test]
    fn test_put_get_edge() {
        let (engine, _tmp) = make_engine();
        let edge = test_edge(100, 1, 2);

        engine.put_edge(&edge).unwrap();
        let retrieved = engine.get_edge(EdgeId(100)).unwrap().unwrap();
        assert_eq!(retrieved.source, NodeId(1));
        assert_eq!(retrieved.target, NodeId(2));
        assert_eq!(retrieved.edge_type, "KNOWS");
    }

    #[test]
    fn test_get_edges_by_direction() {
        let (engine, _tmp) = make_engine();

        // Create edges: 1->2, 1->3, 4->1
        engine.put_edge(&test_edge(10, 1, 2)).unwrap();
        engine.put_edge(&test_edge(11, 1, 3)).unwrap();
        engine.put_edge(&test_edge(12, 4, 1)).unwrap();

        let outgoing = engine.get_edges(NodeId(1), Direction::Outgoing).unwrap();
        assert_eq!(outgoing.len(), 2);

        let incoming = engine.get_edges(NodeId(1), Direction::Incoming).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source, NodeId(4));

        let both = engine.get_edges(NodeId(1), Direction::Both).unwrap();
        assert_eq!(both.len(), 3);
    }

    #[test]
    fn test_delete_edge() {
        let (engine, _tmp) = make_engine();
        engine.put_edge(&test_edge(10, 1, 2)).unwrap();

        assert!(engine.delete_edge(EdgeId(10)).unwrap());
        assert!(engine.get_edge(EdgeId(10)).unwrap().is_none());

        // Adjacency should also be cleaned up.
        let outgoing = engine.get_edges(NodeId(1), Direction::Outgoing).unwrap();
        assert!(outgoing.is_empty());
    }

    #[test]
    fn test_flush() {
        let (engine, _tmp) = make_engine();
        engine.put_node(&test_node(1)).unwrap();
        engine.put_edge(&test_edge(10, 1, 2)).unwrap();
        engine.flush().unwrap(); // Should not panic.
    }
}

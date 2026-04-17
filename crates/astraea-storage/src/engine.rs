//! StorageEngine implementation that ties together the FileManager, BufferPool,
//! and WAL to provide a complete disk-backed storage engine for nodes and edges.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::{StorageEngine, TransactionalEngine};
use astraea_core::types::*;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::buffer_pool::BufferPool;
use crate::file_manager::FileManager;
use crate::label_index::LabelIndex;
use crate::mvcc::{TransactionManager, WriteOp};
use crate::page::*;
use crate::page_io::PageIO;
use crate::wal::{WalReader, WalRecord, WalWriter};

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
    /// In-memory label index for fast label-based lookups.
    label_index: RwLock<LabelIndex>,

    /// MVCC transaction manager for transactional operations.
    txn_manager: TransactionManager,

    /// Path to the data directory (for diagnostics).
    #[allow(dead_code)]
    data_dir: PathBuf,

    /// True while `open()` is replaying the WAL to rebuild in-memory indexes.
    /// During replay, mutation methods skip their `self.wal.append(...)` call
    /// so the log does not double-grow on every restart.
    replaying: AtomicBool,

    /// Pages whose live record has been deleted or overwritten. Reused by
    /// subsequent `write_record` calls before allocating new pages — this is
    /// our incremental compaction path (astraeadb-issues.md #15).
    ///
    /// Not persisted: after a restart, any unreferenced pages on disk are
    /// effectively leaked until a future full-file compaction runs.
    free_pages: Mutex<Vec<PageId>>,
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
        let buffer_pool = BufferPool::new(Arc::clone(&file_manager) as Arc<dyn PageIO>, pool_size);
        let wal = WalWriter::new(&wal_path)?;

        Ok(Self {
            file_manager,
            buffer_pool,
            wal,
            node_index: RwLock::new(HashMap::new()),
            edge_index: RwLock::new(HashMap::new()),
            adjacency: RwLock::new(AdjacencyIndex::new()),
            label_index: RwLock::new(LabelIndex::new()),
            txn_manager: TransactionManager::new(),
            data_dir,
            replaying: AtomicBool::new(false),
            free_pages: Mutex::new(Vec::new()),
        })
    }

    /// How many previously-allocated pages are currently available for reuse
    /// by the next [`write_record`] call. Useful for tests and metrics.
    pub fn free_page_count(&self) -> usize {
        self.free_pages.lock().len()
    }

    /// Open a storage engine at `data_dir`, creating it if missing, and
    /// replay the WAL to rebuild the in-memory indexes.
    ///
    /// Returns `(engine, max_node_id, max_edge_id)` so a caller like
    /// [`astraea_graph::Graph::with_start_ids`] can resume id allocation
    /// from where the previous run left off.
    ///
    /// For a fresh `data_dir` (empty or absent WAL), this is equivalent to
    /// [`DiskStorageEngine::new`] with `max_node_id == max_edge_id == 0`.
    pub fn open<P: AsRef<Path>>(data_dir: P) -> Result<(Self, u64, u64)> {
        let path = data_dir.as_ref().to_path_buf();
        let engine = Self::new(&path)?;
        let wal_path = path.join("astraea.wal");

        // A brand-new data dir has no WAL yet; skip replay.
        if !wal_path.exists() {
            return Ok((engine, 0, 0));
        }

        let reader = WalReader::new(&wal_path);
        let records = reader.read_from(Lsn(0))?;

        engine.replaying.store(true, Ordering::SeqCst);
        let mut max_node_id = 0u64;
        let mut max_edge_id = 0u64;
        let mut inserts = 0usize;
        let mut deletes = 0usize;
        let mut skipped = 0usize;

        for (_lsn, record) in records {
            match record {
                WalRecord::InsertNode(node) => {
                    max_node_id = max_node_id.max(node.id.0);
                    engine.put_node(&node)?;
                    inserts += 1;
                }
                WalRecord::InsertEdge(edge) => {
                    max_edge_id = max_edge_id.max(edge.id.0);
                    engine.put_edge(&edge)?;
                    inserts += 1;
                }
                WalRecord::DeleteNode(id) => {
                    engine.delete_node(id)?;
                    deletes += 1;
                }
                WalRecord::DeleteEdge(id) => {
                    engine.delete_edge(id)?;
                    deletes += 1;
                }
                WalRecord::UpdateNodeProperties(..)
                | WalRecord::Checkpoint(_)
                | WalRecord::BeginTransaction(_)
                | WalRecord::CommitTransaction(_)
                | WalRecord::AbortTransaction(_) => {
                    // Node/edge identity is re-established by InsertNode /
                    // InsertEdge. Property updates and transaction markers do
                    // not affect index rebuild.
                    skipped += 1;
                }
            }
        }
        engine.replaying.store(false, Ordering::SeqCst);

        tracing::info!(
            "WAL replay: {} inserts, {} deletes, {} skipped; next_node_id={}, next_edge_id={}",
            inserts,
            deletes,
            skipped,
            max_node_id + 1,
            max_edge_id + 1,
        );
        Ok((engine, max_node_id, max_edge_id))
    }

    /// Append a record to the WAL unless replay is in progress.
    #[inline]
    fn wal_append(&self, record: &WalRecord) -> Result<()> {
        if self.replaying.load(Ordering::Relaxed) {
            Ok(())
        } else {
            self.wal.append(record).map(|_| ())
        }
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

    /// Write a record (node or edge) into a page. Reuses a previously freed
    /// page if one is available, otherwise allocates a new one.
    /// Returns the page ID where the record was written.
    fn write_record(
        &self,
        record_id: u64,
        data: &[u8],
        page_type: PageType,
    ) -> Result<PageId> {
        let total_size = NODE_RECORD_HEADER_SIZE + data.len();

        // Prefer recycling a freed page; fall back to allocating a new one.
        let recycled = self.free_pages.lock().pop();
        let (guard, page_id) = match recycled {
            Some(pid) => {
                let page_buf = init_page(pid, page_type);
                let g = self.buffer_pool.pin_recycled_page(pid, &page_buf)?;
                (g, pid)
            }
            None => {
                let page_buf = init_page(PageId(0), page_type);
                let g = self.buffer_pool.pin_new_page(&page_buf)?;
                let pid = g.page_id();
                (g, pid)
            }
        };

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
        // Log to WAL first (unless we are replaying — see `open`).
        self.wal_append(&WalRecord::InsertNode(node.clone()))?;

        // If this node already exists, remove its old labels from the index
        // before inserting the new ones, and mark its previous page as free
        // so future writes can reuse that slot.
        if let Ok(Some(old_node)) = self.get_node(node.id) {
            let mut li = self.label_index.write();
            li.remove_node(node.id, &old_node.labels);
            drop(li);
            let old_page = self.node_index.read().get(&node.id).copied();
            if let Some(pid) = old_page {
                self.free_pages.lock().push(pid);
            }
        }

        // Serialize.
        let data = Self::serialize_node(node)?;

        // Write to a recycled page if available, otherwise a freshly allocated
        // one. `write_record` consults `self.free_pages` first.
        let page_id = self.write_record(node.id.0, &data, PageType::NodePage)?;

        // Update the in-memory index.
        let mut index = self.node_index.write();
        index.insert(node.id, page_id);

        // Update the label index.
        let mut li = self.label_index.write();
        li.add_node(node.id, &node.labels);

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
        // Get labels before deleting so we can clean up the label index.
        if let Ok(Some(node)) = self.get_node(id) {
            let mut li = self.label_index.write();
            li.remove_node(id, &node.labels);
        }

        // Log to WAL (unless replaying).
        self.wal_append(&WalRecord::DeleteNode(id))?;

        let mut index = self.node_index.write();
        let removed_page = index.remove(&id);
        drop(index);
        if let Some(page_id) = removed_page {
            // Incremental compaction: the page that held this node is now
            // free for reuse by the next write.
            self.free_pages.lock().push(page_id);
        }
        Ok(removed_page.is_some())
    }

    fn put_edge(&self, edge: &Edge) -> Result<()> {
        // Log to WAL (unless replaying).
        self.wal_append(&WalRecord::InsertEdge(edge.clone()))?;

        // Update path: if this edge already exists, free its previous page
        // and drop its old adjacency entries before re-inserting.
        let old_edge_page = self.edge_index.read().get(&edge.id).copied();
        if old_edge_page.is_some() {
            // Read the old edge so we can rewrite adjacency correctly.
            if let Ok(Some(old_edge)) = self.get_edge(edge.id) {
                let mut adj = self.adjacency.write();
                adj.remove_edge(edge.id, old_edge.source, old_edge.target);
            }
            if let Some(pid) = old_edge_page {
                self.free_pages.lock().push(pid);
            }
        }

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

        // Log to WAL (unless replaying).
        self.wal_append(&WalRecord::DeleteEdge(id))?;

        let removed_page = {
            let mut index = self.edge_index.write();
            index.remove(&id)
        };
        if let Some(pid) = removed_page {
            // Incremental compaction: the page is free for reuse.
            self.free_pages.lock().push(pid);
        }

        if let Some(edge) = edge {
            let mut adj = self.adjacency.write();
            adj.remove_edge(id, edge.source, edge.target);
        }

        Ok(removed_page.is_some())
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

    fn find_nodes_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        Ok(self.label_index.read().get(label))
    }
}

impl TransactionalEngine for DiskStorageEngine {
    fn begin_transaction(&self) -> Result<TransactionId> {
        let current_lsn = self.wal.current_lsn();
        let txn_id = self.txn_manager.begin(current_lsn);

        // Write a BeginTransaction WAL record.
        self.wal
            .append(&WalRecord::BeginTransaction(txn_id.0))?;

        Ok(txn_id)
    }

    fn commit_transaction(&self, txn_id: TransactionId) -> Result<()> {
        // Commit the transaction -- retrieves the buffered write set.
        let write_set = self.txn_manager.commit(txn_id)?;

        // Apply all buffered writes atomically to the real storage.
        for op in write_set {
            match op {
                WriteOp::PutNode(node) => {
                    self.put_node(&node)?;
                }
                WriteOp::DeleteNode(id) => {
                    self.delete_node(id)?;
                }
                WriteOp::PutEdge(edge) => {
                    self.put_edge(&edge)?;
                }
                WriteOp::DeleteEdge(id) => {
                    self.delete_edge(id)?;
                }
            }
        }

        // Write a CommitTransaction WAL record.
        self.wal
            .append(&WalRecord::CommitTransaction(txn_id.0))?;

        Ok(())
    }

    fn abort_transaction(&self, txn_id: TransactionId) -> Result<()> {
        self.txn_manager.abort(txn_id)?;

        // Write an AbortTransaction WAL record.
        self.wal
            .append(&WalRecord::AbortTransaction(txn_id.0))?;

        Ok(())
    }

    fn put_node_tx(&self, node: &Node, txn_id: TransactionId) -> Result<()> {
        self.txn_manager
            .buffer_write(txn_id, node.id.0, WriteOp::PutNode(node.clone()))
    }

    fn delete_node_tx(&self, id: NodeId, txn_id: TransactionId) -> Result<bool> {
        self.txn_manager
            .buffer_write(txn_id, id.0, WriteOp::DeleteNode(id))?;
        // We return true optimistically; the actual deletion happens on commit.
        Ok(true)
    }

    fn put_edge_tx(&self, edge: &Edge, txn_id: TransactionId) -> Result<()> {
        // Use a separate namespace for edge entity IDs to avoid conflicts
        // with node IDs. Edge IDs are offset by a large constant.
        let entity_id = edge.id.0.wrapping_add(1 << 63);
        self.txn_manager
            .buffer_write(txn_id, entity_id, WriteOp::PutEdge(edge.clone()))
    }

    fn delete_edge_tx(&self, id: EdgeId, txn_id: TransactionId) -> Result<bool> {
        let entity_id = id.0.wrapping_add(1 << 63);
        self.txn_manager
            .buffer_write(txn_id, entity_id, WriteOp::DeleteEdge(id))?;
        Ok(true)
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

    // --- Transactional integration tests ---

    use astraea_core::traits::TransactionalEngine;

    #[test]
    fn test_transactional_put_commit() {
        let (engine, _tmp) = make_engine();

        // Begin a transaction.
        let txn = engine.begin_transaction().unwrap();

        // Buffer a node write within the transaction.
        let node = test_node(42);
        engine.put_node_tx(&node, txn).unwrap();

        // Before commit, the node should NOT be visible in the storage.
        assert!(engine.get_node(NodeId(42)).unwrap().is_none());

        // Commit the transaction.
        engine.commit_transaction(txn).unwrap();

        // After commit, the node should be visible.
        let retrieved = engine.get_node(NodeId(42)).unwrap().unwrap();
        assert_eq!(retrieved.id, NodeId(42));
        assert_eq!(retrieved.labels, vec!["Person".to_string()]);
    }

    #[test]
    fn test_transactional_put_abort() {
        let (engine, _tmp) = make_engine();

        // Begin a transaction.
        let txn = engine.begin_transaction().unwrap();

        // Buffer a node write within the transaction.
        let node = test_node(99);
        engine.put_node_tx(&node, txn).unwrap();

        // Abort the transaction.
        engine.abort_transaction(txn).unwrap();

        // The node should NOT exist in storage.
        assert!(engine.get_node(NodeId(99)).unwrap().is_none());
    }

    #[test]
    fn test_transactional_edge_commit() {
        let (engine, _tmp) = make_engine();

        let txn = engine.begin_transaction().unwrap();

        // Buffer a node and edge write.
        let node1 = test_node(1);
        let node2 = test_node(2);
        let edge = test_edge(100, 1, 2);
        engine.put_node_tx(&node1, txn).unwrap();
        engine.put_node_tx(&node2, txn).unwrap();
        engine.put_edge_tx(&edge, txn).unwrap();

        // Nothing visible yet.
        assert!(engine.get_edge(EdgeId(100)).unwrap().is_none());

        // Commit.
        engine.commit_transaction(txn).unwrap();

        // Edge and nodes should now be visible.
        let retrieved_edge = engine.get_edge(EdgeId(100)).unwrap().unwrap();
        assert_eq!(retrieved_edge.source, NodeId(1));
        assert_eq!(retrieved_edge.target, NodeId(2));
        assert!(engine.get_node(NodeId(1)).unwrap().is_some());
        assert!(engine.get_node(NodeId(2)).unwrap().is_some());
    }

    #[test]
    fn test_transactional_delete_commit() {
        let (engine, _tmp) = make_engine();

        // First, insert a node directly.
        engine.put_node(&test_node(50)).unwrap();
        assert!(engine.get_node(NodeId(50)).unwrap().is_some());

        // Now delete it within a transaction.
        let txn = engine.begin_transaction().unwrap();
        engine.delete_node_tx(NodeId(50), txn).unwrap();

        // Before commit, the node should still exist.
        assert!(engine.get_node(NodeId(50)).unwrap().is_some());

        // Commit.
        engine.commit_transaction(txn).unwrap();

        // After commit, the node should be gone.
        assert!(engine.get_node(NodeId(50)).unwrap().is_none());
    }

    #[test]
    fn test_transactional_write_conflict() {
        let (engine, _tmp) = make_engine();

        let txn1 = engine.begin_transaction().unwrap();
        let txn2 = engine.begin_transaction().unwrap();

        // txn1 writes node 7.
        engine.put_node_tx(&test_node(7), txn1).unwrap();

        // txn2 tries to write the same node -- should fail.
        let result = engine.put_node_tx(&test_node(7), txn2);
        assert!(result.is_err());

        // txn1 can still commit.
        engine.commit_transaction(txn1).unwrap();
        assert!(engine.get_node(NodeId(7)).unwrap().is_some());
    }

    #[test]
    fn test_open_replays_wal() {
        // Issue astraeadb-issues.md #1: server restart used to lose the
        // graph. Fixed by wiring DiskStorageEngine::open + WAL replay.
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();

        // Phase 1: write some state.
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 64).unwrap();
            engine.put_node(&test_node(1)).unwrap();
            engine.put_node(&test_node(2)).unwrap();
            engine.put_node(&test_node(3)).unwrap();
            engine.put_edge(&test_edge(10, 1, 2)).unwrap();
            engine.put_edge(&test_edge(11, 2, 3)).unwrap();
            engine.delete_node(NodeId(2)).unwrap();
            // Drop without flush — mimics an unclean shutdown.
        }

        // Phase 2: reopen and verify state survived via WAL replay.
        let (engine, max_node_id, max_edge_id) = DiskStorageEngine::open(data_dir).unwrap();
        assert_eq!(max_node_id, 3, "max_node_id should be 3");
        assert_eq!(max_edge_id, 11, "max_edge_id should be 11");

        assert!(engine.get_node(NodeId(1)).unwrap().is_some(), "node 1 should survive");
        assert!(engine.get_node(NodeId(2)).unwrap().is_none(), "node 2 was deleted");
        assert!(engine.get_node(NodeId(3)).unwrap().is_some(), "node 3 should survive");
        assert!(engine.get_edge(EdgeId(10)).unwrap().is_some(), "edge 10 should survive");
        assert!(engine.get_edge(EdgeId(11)).unwrap().is_some(), "edge 11 should survive");

        // Label index rebuilt — find_nodes_by_label should return node 1 and 3.
        let persons = engine.find_nodes_by_label("Person").unwrap();
        assert_eq!(persons.len(), 2, "two Persons survive");
        assert!(persons.contains(&NodeId(1)));
        assert!(persons.contains(&NodeId(3)));

        // Further mutations after reopen must not double-grow the WAL
        // (replay does not re-append, but new writes do).
        engine.put_node(&test_node(4)).unwrap();
        assert!(engine.get_node(NodeId(4)).unwrap().is_some());
    }

    #[test]
    fn test_compaction_reclaims_pages() {
        // astraeadb-issues.md #15: writes used to leak pages under churn.
        // After this fix, delete + write reuses the freed page slot.
        let (engine, _tmp) = make_engine();

        // Insert 5 nodes — 5 fresh pages allocated.
        for i in 1..=5 {
            engine.put_node(&test_node(i)).unwrap();
        }
        let page_count_after_inserts = engine.file_manager.page_count().unwrap();
        assert_eq!(engine.free_page_count(), 0);

        // Delete 3 nodes — 3 pages become free.
        engine.delete_node(NodeId(2)).unwrap();
        engine.delete_node(NodeId(3)).unwrap();
        engine.delete_node(NodeId(4)).unwrap();
        assert_eq!(engine.free_page_count(), 3, "deletes populate the free list");

        // Insert 3 more nodes — all 3 should come from the free list, so
        // the underlying file does NOT grow.
        engine.put_node(&test_node(6)).unwrap();
        engine.put_node(&test_node(7)).unwrap();
        engine.put_node(&test_node(8)).unwrap();

        assert_eq!(engine.free_page_count(), 0, "free list drained by reuse");
        let page_count_after_reuse = engine.file_manager.page_count().unwrap();
        assert_eq!(
            page_count_after_reuse, page_count_after_inserts,
            "page file did not grow after replacing deleted nodes"
        );

        // All five live nodes (1, 5, 6, 7, 8) are still readable.
        for id in [1, 5, 6, 7, 8] {
            assert!(
                engine.get_node(NodeId(id)).unwrap().is_some(),
                "node {} should be present after compaction reuse",
                id
            );
        }
        // Deleted ones stay gone.
        for id in [2, 3, 4] {
            assert!(
                engine.get_node(NodeId(id)).unwrap().is_none(),
                "node {} should still be deleted",
                id
            );
        }
    }

    #[test]
    fn test_update_frees_old_page() {
        let (engine, _tmp) = make_engine();
        engine.put_node(&test_node(1)).unwrap();
        let before = engine.file_manager.page_count().unwrap();
        // Overwrite the same node — old page should be freed, new page
        // allocated from it (so total page_count stays put).
        engine.put_node(&Node {
            id: NodeId(1),
            labels: vec!["Updated".to_string()],
            properties: serde_json::json!({"new": "content"}),
            embedding: None,
        }).unwrap();
        let after = engine.file_manager.page_count().unwrap();
        assert_eq!(after, before, "update should reuse the freed page");
        let got = engine.get_node(NodeId(1)).unwrap().unwrap();
        assert_eq!(got.labels, vec!["Updated"]);
    }

    #[test]
    fn test_open_on_fresh_dir() {
        let tmp = TempDir::new().unwrap();
        let (engine, max_node, max_edge) = DiskStorageEngine::open(tmp.path()).unwrap();
        assert_eq!(max_node, 0);
        assert_eq!(max_edge, 0);
        engine.put_node(&test_node(1)).unwrap();
        assert!(engine.get_node(NodeId(1)).unwrap().is_some());
    }
}

//! StorageEngine implementation that ties together the FileManager, BufferPool,
//! and WAL to provide a complete disk-backed storage engine for nodes and edges.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::{StorageEngine, TransactionalEngine};
use astraea_core::types::*;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::buffer_pool::BufferPool;
use crate::file_manager::FileManager;
use crate::label_index::LabelIndex;
use crate::mvcc::{TransactionManager, WriteOp};
use crate::page::*;
use crate::page_io::PageIO;
use crate::wal::{WalReader, WalRecord, WalWriter};

/// Default buffer pool size (number of page frames).
const DEFAULT_POOL_SIZE: usize = 1024;

/// Format tag for the v1 binary node-record body.
///
/// Byte 0 of the record body distinguishes the encoding:
///   `0x01` → v1 binary container (see `serialize_node` for the full layout)
///   any other byte → legacy `serde_json`-encoded `SerializedNode` (back-compat)
///
/// Legacy records always begin with `{` (0x7B, the JSON object opener), so
/// `0x01` is unambiguously a v1 record and can never collide with any
/// well-formed pre-Phase-1 data file.  Tags `0x02`–`0xFF` are reserved for
/// future formats.
const RECORD_TAG_V1: u8 = 0x01;

/// Serialized node data stored in a page (properties + embedding + labels).
/// Retained for the legacy JSON back-compat deserialization path; new writes
/// use `SerializedNodeBody` + raw f32 bytes via the v1 binary container.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedNode {
    labels: Vec<String>,
    properties: serde_json::Value,
    embedding: Option<Vec<f32>>,
}

/// JSON-encodable body of a v1 node record: labels and properties only.
/// The embedding is stored separately as raw little-endian f32 bytes in the
/// v1 binary container (see `serialize_node`), so it is excluded here to
/// avoid JSON-encoding the embedding and doubling the per-record footprint.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedNodeBody {
    labels: Vec<String>,
    properties: serde_json::Value,
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
        self.outgoing.entry(source).or_default().push(edge_id);
        self.incoming.entry(target).or_default().push(edge_id);
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
            Direction::Outgoing => self.outgoing.get(&node_id).cloned().unwrap_or_default(),
            Direction::Incoming => self.incoming.get(&node_id).cloned().unwrap_or_default(),
            Direction::Both => {
                let mut result = self.outgoing.get(&node_id).cloned().unwrap_or_default();
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

    /// Serialize a node into bytes for page storage using the v1 binary container.
    ///
    /// # v1 binary container layout (leading byte = `RECORD_TAG_V1` = `0x01`)
    ///
    /// ```text
    /// [tag      : u8  = 0x01]
    /// [json_len : u32 LE   ]  -- byte length of the JSON body that follows
    /// [json body: bytes    ]  -- serde_json of SerializedNodeBody {labels, properties}
    /// [dim      : u32 LE   ]  -- embedding dimension; 0 means no embedding
    /// [f32 LE × dim        ]  -- raw little-endian f32 values; absent when dim == 0
    /// ```
    ///
    /// Encoding a 768-dim embedding as 3,072 raw bytes vs. the ~6–9 KB produced
    /// by JSON decimal-float encoding cuts the per-record footprint by roughly
    /// half and keeps typical nodes well within the 8,159-byte single-page budget.
    /// The f32 values round-trip bit-identically through `from_le_bytes` /
    /// `to_le_bytes`.
    fn serialize_node(node: &Node) -> Result<Vec<u8>> {
        let body = SerializedNodeBody {
            labels: node.labels.clone(),
            properties: node.properties.clone(),
        };
        let json_bytes =
            serde_json::to_vec(&body).map_err(|e| AstraeaError::Serialization(e.to_string()))?;

        let dim = node.embedding.as_ref().map_or(0u32, |v| v.len() as u32);
        let embedding_byte_len = dim as usize * 4;

        // Total capacity: tag(1) + json_len(4) + json body + dim(4) + f32 bytes.
        let mut out = Vec::with_capacity(1 + 4 + json_bytes.len() + 4 + embedding_byte_len);
        out.push(RECORD_TAG_V1);
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&dim.to_le_bytes());
        if let Some(emb) = &node.embedding {
            for &f in emb {
                out.extend_from_slice(&f.to_le_bytes());
            }
        }
        Ok(out)
    }

    /// Deserialize a node from page bytes.
    ///
    /// Reads the leading byte to choose the decoding path:
    /// - `RECORD_TAG_V1` (`0x01`) → v1 binary container (see `serialize_node`).
    /// - any other byte → legacy `serde_json`-encoded `SerializedNode`, which
    ///   always starts with `{` (`0x7B`).  This path handles every data file
    ///   written before Phase 1 with no migration required.
    fn deserialize_node(id: NodeId, data: &[u8]) -> Result<Node> {
        if data.is_empty() {
            return Err(AstraeaError::Deserialization(
                "node record is empty".to_string(),
            ));
        }

        if data[0] == RECORD_TAG_V1 {
            // --- v1 binary container ---
            // Layout: [0x01][json_len:u32 LE][json body][dim:u32 LE][f32 LE…]
            if data.len() < 5 {
                return Err(AstraeaError::Deserialization(
                    "v1 node record too short (missing json_len)".to_string(),
                ));
            }
            let json_len =
                u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
            let json_start = 5;
            let json_end = json_start + json_len;
            if data.len() < json_end + 4 {
                return Err(AstraeaError::Deserialization(
                    "v1 node record: JSON body truncated".to_string(),
                ));
            }
            let body: SerializedNodeBody =
                serde_json::from_slice(&data[json_start..json_end])
                    .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

            let dim_start = json_end;
            let dim = u32::from_le_bytes([
                data[dim_start],
                data[dim_start + 1],
                data[dim_start + 2],
                data[dim_start + 3],
            ]) as usize;

            let embedding = if dim == 0 {
                None
            } else {
                let emb_start = dim_start + 4;
                let emb_end = emb_start + dim * 4;
                if data.len() < emb_end {
                    return Err(AstraeaError::Deserialization(
                        "v1 node record: embedding bytes truncated".to_string(),
                    ));
                }
                let floats: Vec<f32> = data[emb_start..emb_end]
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                Some(floats)
            };

            Ok(Node {
                id,
                labels: body.labels,
                properties: body.properties,
                embedding,
            })
        } else {
            // --- Legacy JSON path (back-compat) ---
            // Records written before v1 begin with `{` (0x7B).  Any
            // unrecognized leading byte also routes here; serde_json will
            // return a Deserialization error rather than silently producing
            // corrupt data.
            let sn: SerializedNode = serde_json::from_slice(data)
                .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;
            Ok(Node {
                id,
                labels: sn.labels,
                properties: sn.properties,
                embedding: sn.embedding,
            })
        }
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
        let se: SerializedEdge = serde_json::from_slice(data)
            .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;
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

    /// Write a single overflow continuation page containing `chunk` of record data,
    /// chaining to `next_encoded` ("+1 encoded" `PageId`, 0 = end of chain).
    /// Returns the allocated page ID.
    ///
    /// # Overflow page layout (after `PageHeader`)
    ///
    /// ```text
    /// [next_overflow_encoded : u64 LE, 8 bytes]   -- 0=end, n=PageId(n-1)
    /// [chunk bytes           : up to OVERFLOW_PAGE_PAYLOAD bytes]
    /// ```
    ///
    /// Called from `write_record` in reverse chunk order (last chunk first) so
    /// each page can write its own "next" pointer before being finalized.
    fn write_overflow_page(&self, chunk: &[u8], next_encoded: u64) -> Result<PageId> {
        debug_assert!(chunk.len() <= OVERFLOW_PAGE_PAYLOAD, "overflow chunk too large");

        // Allocate or recycle a page for this overflow chunk.
        let recycled = self.free_pages.lock().pop();
        let (guard, page_id) = match recycled {
            Some(pid) => {
                let page_buf = init_page(pid, PageType::OverflowPage);
                let g = self.buffer_pool.pin_recycled_page(pid, &page_buf)?;
                (g, pid)
            }
            None => {
                let page_buf = init_page(PageId(0), PageType::OverflowPage);
                let g = self.buffer_pool.pin_new_page(&page_buf)?;
                let pid = g.page_id();
                (g, pid)
            }
        };

        let mut buf = guard.data().0;

        // Patch the real page_id into the header (pin_new_page uses PageId(0) as placeholder).
        let mut header = PageHeader::read_from(&buf)?;
        header.page_id = page_id;

        // Write the continuation link immediately after the page header.
        let link_offset = PAGE_HEADER_SIZE;
        buf[link_offset..link_offset + OVERFLOW_LINK_SIZE]
            .copy_from_slice(&next_encoded.to_le_bytes());

        // Write the chunk payload.
        let chunk_offset = link_offset + OVERFLOW_LINK_SIZE;
        buf[chunk_offset..chunk_offset + chunk.len()].copy_from_slice(chunk);

        // Update and rewrite the page header with a fresh checksum.
        header.free_space_offset = (chunk_offset + chunk.len()) as u16;
        header.checksum = 0;
        header.write_to(&mut buf);
        let checksum = compute_page_checksum(&buf);
        header.checksum = checksum;
        header.write_to(&mut buf);

        guard.write_data(&buf);
        self.buffer_pool.unpin_page(page_id, true)?;

        Ok(page_id)
    }

    /// Follow the overflow chain rooted at `head_pid` and return every page in
    /// the chain in order: `[head_pid, first_overflow, second_overflow, …]`.
    ///
    /// Callers use this to free a multi-page record atomically: they push all
    /// returned page IDs to `self.free_pages` before the head page is reused.
    fn collect_record_pages(&self, head_pid: PageId) -> Result<Vec<PageId>> {
        let mut pages = vec![head_pid];

        // Read the head page to find the first overflow pointer.
        let guard = self.buffer_pool.pin_page(head_pid)?;
        let buf = guard.data();
        let header = PageHeader::read_from(&buf)?;

        let mut next_encoded: u64 = if header.record_count > 0 {
            // The NodeRecordHeader starts right after the page header.
            let rec = NodeRecordHeader::read_from(&buf, PAGE_HEADER_SIZE);
            rec.overflow_page_id as u64
        } else {
            0
        };
        self.buffer_pool.unpin_page(head_pid, false)?;

        // Walk the overflow chain.  "+1 encoded": n means PageId(n-1), 0 means done.
        while next_encoded != 0 {
            let ov_pid = PageId(next_encoded - 1);
            pages.push(ov_pid);

            let ov_guard = self.buffer_pool.pin_page(ov_pid)?;
            let ov_buf = ov_guard.data();
            next_encoded = u64::from_le_bytes(
                ov_buf[PAGE_HEADER_SIZE..PAGE_HEADER_SIZE + OVERFLOW_LINK_SIZE]
                    .try_into()
                    .expect("overflow link: slice len == 8"),
            );
            self.buffer_pool.unpin_page(ov_pid, false)?;
        }

        Ok(pages)
    }

    /// Write a record (node or edge) into one or more pages.
    ///
    /// If `data` fits within `HEAD_PAGE_CAPACITY` (8,159 bytes) the record
    /// occupies a single page and no overflow pages are allocated — identical
    /// to the pre-Phase-2 behaviour.  If `data` is larger, the remainder is
    /// chained through `OverflowPage`s.
    ///
    /// # Single-page layout (unchanged)
    ///
    /// ```text
    /// [PageHeader      : PAGE_HEADER_SIZE bytes]
    /// [NodeRecordHeader: NODE_RECORD_HEADER_SIZE bytes, overflow_page_id=0]
    /// [data bytes      : data.len() bytes]
    /// ```
    ///
    /// # Multi-page layout
    ///
    /// ```text
    /// HEAD page (NodePage / EdgePage):
    ///   [PageHeader      : PAGE_HEADER_SIZE bytes]
    ///   [NodeRecordHeader: NODE_RECORD_HEADER_SIZE bytes, overflow_page_id = first_ovf+1]
    ///   [data[0..HEAD_PAGE_CAPACITY]]
    ///
    /// Each OverflowPage:
    ///   [PageHeader              : PAGE_HEADER_SIZE bytes]
    ///   [next_overflow_encoded   : u64 LE, 8 bytes]    -- 0=end, n=PageId(n-1)
    ///   [data[chunk_start..chunk_end]]
    /// ```
    ///
    /// `NodeRecordHeader.data_len` is always the **total** record byte count
    /// across the entire chain.  The "+1 encoding" for overflow pointers ensures
    /// `0` unambiguously means "no overflow / end of chain" even when
    /// `PageId(0)` is a valid page (the first page in an empty file).
    fn write_record(&self, record_id: u64, data: &[u8], page_type: PageType) -> Result<PageId> {
        // --- Phase 1: build the overflow chain (back-to-front) if needed. ------
        //
        // We write the last chunk first so that each page can embed its "next"
        // pointer before being finalised and unpinned.  This avoids holding
        // multiple pages pinned simultaneously, keeping pool pressure low.
        let first_overflow_encoded: u64 = if data.len() > HEAD_PAGE_CAPACITY {
            let overflow_data = &data[HEAD_PAGE_CAPACITY..];
            let mut next_encoded: u64 = 0; // end sentinel

            // Collect chunk ranges then iterate in reverse.
            let chunk_ranges: Vec<std::ops::Range<usize>> = overflow_data
                .chunks(OVERFLOW_PAGE_PAYLOAD)
                .scan(0usize, |start, chunk| {
                    let range = *start..*start + chunk.len();
                    *start += chunk.len();
                    Some(range)
                })
                .collect();

            for range in chunk_ranges.iter().rev() {
                let pid = self.write_overflow_page(&overflow_data[range.clone()], next_encoded)?;
                next_encoded = pid.0 + 1; // "+1 encoding"
            }
            next_encoded
        } else {
            0
        };

        // --- Phase 2: allocate / recycle the head page. -----------------------
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

        // --- Phase 3: write the head page record. -----------------------------
        let mut buf = guard.data().0;
        let mut header = PageHeader::read_from(&buf)?;
        header.page_id = page_id;

        let head_data = &data[..data.len().min(HEAD_PAGE_CAPACITY)];
        let offset = header.free_space_offset as usize;

        // `overflow_page_id` is stored as a u32 (reusing the former
        // `adjacency_offset` field); the "+1 encoding" is safe here because
        // `first_overflow_encoded` was produced by the loop above using
        // `pid.0 + 1`, so its value always fits in u32 when the total number
        // of pages is below 2^32 − 1 (~4 billion pages = ~32 TiB at 8 KiB/page).
        let overflow_page_id = first_overflow_encoded as u32;

        let rec_header = NodeRecordHeader {
            node_id: record_id,
            data_len: data.len() as u32,
            overflow_page_id,
        };
        rec_header.write_to(&mut buf, offset);

        let data_offset = offset + NODE_RECORD_HEADER_SIZE;
        buf[data_offset..data_offset + head_data.len()].copy_from_slice(head_data);

        header.record_count += 1;
        header.free_space_offset = (data_offset + head_data.len()) as u16;
        header.checksum = 0;
        header.write_to(&mut buf);
        let checksum = compute_page_checksum(&buf);
        header.checksum = checksum;
        header.write_to(&mut buf);

        guard.write_data(&buf);
        self.buffer_pool.unpin_page(page_id, true)?;

        Ok(page_id)
    }

    /// Read a record from the page at `page_id`, reassembling the full data
    /// across any overflow pages in the continuation chain.
    fn read_record(&self, page_id: PageId, record_id: u64) -> Result<Option<Vec<u8>>> {
        let guard = self.buffer_pool.pin_page(page_id)?;
        let buf = guard.data();

        let header = PageHeader::read_from(&buf)?;
        let mut offset = PAGE_HEADER_SIZE;

        for _ in 0..header.record_count {
            let rec = NodeRecordHeader::read_from(&buf, offset);
            let total_data_len = rec.data_len as usize;
            let data_offset = offset + NODE_RECORD_HEADER_SIZE;

            if rec.node_id == record_id {
                // Copy the inline (head-page) portion.
                let head_bytes = total_data_len.min(HEAD_PAGE_CAPACITY);
                let mut data = buf[data_offset..data_offset + head_bytes].to_vec();
                let first_overflow = rec.overflow_page_id as u64;

                // Unpin the head page before touching overflow pages to keep
                // buffer-pool pressure low (important for tiny pools).
                self.buffer_pool.unpin_page(page_id, false)?;

                // Follow the overflow chain until we have all data.
                let mut next_encoded = first_overflow;
                while next_encoded != 0 && data.len() < total_data_len {
                    let ov_pid = PageId(next_encoded - 1);
                    let ov_guard = self.buffer_pool.pin_page(ov_pid)?;
                    let ov_buf = ov_guard.data();

                    // Read the continuation pointer for the next iteration.
                    next_encoded = u64::from_le_bytes(
                        ov_buf[PAGE_HEADER_SIZE..PAGE_HEADER_SIZE + OVERFLOW_LINK_SIZE]
                            .try_into()
                            .expect("overflow link: slice len == 8"),
                    );

                    // Append the chunk payload.
                    let chunk_offset = PAGE_HEADER_SIZE + OVERFLOW_LINK_SIZE;
                    let remaining = total_data_len - data.len();
                    let chunk_len = remaining.min(OVERFLOW_PAGE_PAYLOAD);
                    data.extend_from_slice(&ov_buf[chunk_offset..chunk_offset + chunk_len]);

                    self.buffer_pool.unpin_page(ov_pid, false)?;
                }

                return Ok(Some(data));
            }

            // Advance to the next record in the page.  Each page holds exactly
            // one record, but we honour the general header.record_count loop so
            // future packing strategies need not change read_record.
            // Only the head-page portion is stored inline; skip that many bytes.
            offset = data_offset + total_data_len.min(HEAD_PAGE_CAPACITY);
        }

        self.buffer_pool.unpin_page(page_id, false)?;
        Ok(None)
    }
}

impl StorageEngine for DiskStorageEngine {
    fn put_node(&self, node: &Node) -> Result<()> {
        // Serialize first.  In Phase 2 write_record handles any size via the
        // overflow chain, so there is no per-size pre-flight rejection here.
        // The WAL-poisoning guard (issue #26 §2a) is now satisfied structurally:
        // write_record can no longer fail on size, so a WAL record that is
        // successfully appended is always replayable.
        let data = Self::serialize_node(node)?;

        // Log to WAL (unless we are replaying — see `open`).
        self.wal_append(&WalRecord::InsertNode(node.clone()))?;

        // If this node already exists, remove its old labels from the index
        // before inserting the new ones, and free EVERY page in the old
        // record's chain (head + all overflow pages).
        if let Ok(Some(old_node)) = self.get_node(node.id) {
            let mut li = self.label_index.write();
            li.remove_node(node.id, &old_node.labels);
            drop(li);
            let old_page = self.node_index.read().get(&node.id).copied();
            if let Some(pid) = old_page {
                let chain = self.collect_record_pages(pid)?;
                let mut free = self.free_pages.lock();
                for p in chain {
                    free.push(p);
                }
            }
        }

        // Write to recycled page(s) if available, otherwise freshly allocated.
        // `write_record` pops from `self.free_pages` internally.
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
            // Collect the full overflow chain BEFORE the head page can be
            // recycled by another write, then push every page to free_pages.
            let chain = self.collect_record_pages(page_id)?;
            let mut free = self.free_pages.lock();
            for p in chain {
                free.push(p);
            }
        }
        Ok(removed_page.is_some())
    }

    fn put_edge(&self, edge: &Edge) -> Result<()> {
        // Serialize first.  write_record handles any size via overflow chain,
        // so there is no size pre-flight rejection (issue #26 Phase 2).
        let data = Self::serialize_edge(edge)?;

        // Log to WAL (unless replaying).
        self.wal_append(&WalRecord::InsertEdge(edge.clone()))?;

        // Update path: if this edge already exists, free its previous page(s)
        // and drop its old adjacency entries before re-inserting.
        let old_edge_page = self.edge_index.read().get(&edge.id).copied();
        if old_edge_page.is_some() {
            // Read the old edge so we can rewrite adjacency correctly.
            if let Ok(Some(old_edge)) = self.get_edge(edge.id) {
                let mut adj = self.adjacency.write();
                adj.remove_edge(edge.id, old_edge.source, old_edge.target);
            }
            if let Some(pid) = old_edge_page {
                // Free the full chain (head + any overflow pages).
                let chain = self.collect_record_pages(pid)?;
                let mut free = self.free_pages.lock();
                for p in chain {
                    free.push(p);
                }
            }
        }

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
            // Free the full chain (head + any overflow pages).
            let chain = self.collect_record_pages(pid)?;
            let mut free = self.free_pages.lock();
            for p in chain {
                free.push(p);
            }
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

    fn find_edges_by_type(&self, edge_type: &str) -> Result<Vec<(EdgeId, NodeId, NodeId)>> {
        // Collect all edge IDs first so we release the index lock before
        // calling get_edge (which acquires the buffer-pool lock).
        let edge_ids: Vec<EdgeId> = self.edge_index.read().keys().copied().collect();
        let mut result = Vec::new();
        for eid in edge_ids {
            if let Some(edge) = self.get_edge(eid)?
                && edge.edge_type == edge_type
            {
                result.push((edge.id, edge.source, edge.target));
            }
        }
        Ok(result)
    }

    fn list_all_nodes(&self) -> Result<Vec<NodeId>> {
        Ok(self.node_index.read().keys().copied().collect())
    }
}

impl TransactionalEngine for DiskStorageEngine {
    fn begin_transaction(&self) -> Result<TransactionId> {
        let current_lsn = self.wal.current_lsn();
        let txn_id = self.txn_manager.begin(current_lsn);

        // Write a BeginTransaction WAL record.
        self.wal.append(&WalRecord::BeginTransaction(txn_id.0))?;

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
        self.wal.append(&WalRecord::CommitTransaction(txn_id.0))?;

        Ok(())
    }

    fn abort_transaction(&self, txn_id: TransactionId) -> Result<()> {
        self.txn_manager.abort(txn_id)?;

        // Write an AbortTransaction WAL record.
        self.wal.append(&WalRecord::AbortTransaction(txn_id.0))?;

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

        assert!(
            engine.get_node(NodeId(1)).unwrap().is_some(),
            "node 1 should survive"
        );
        assert!(
            engine.get_node(NodeId(2)).unwrap().is_none(),
            "node 2 was deleted"
        );
        assert!(
            engine.get_node(NodeId(3)).unwrap().is_some(),
            "node 3 should survive"
        );
        assert!(
            engine.get_edge(EdgeId(10)).unwrap().is_some(),
            "edge 10 should survive"
        );
        assert!(
            engine.get_edge(EdgeId(11)).unwrap().is_some(),
            "edge 11 should survive"
        );

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

    /// Verify that WAL replay recovers ALL nodes and edges after an unclean
    /// shutdown, even when the buffer pool is smaller than the working set
    /// (so some pages were evicted to disk mid-run, but none were flushed
    /// as part of an explicit `flush()` call).
    ///
    /// This is the Rust-level acceptance test for astraeadb-issues.md #1.
    #[test]
    fn test_durability_across_drop_and_reopen() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();

        // How many nodes/edges to write.  Choose enough to overflow a tiny
        // pool (8 frames) so that some dirty pages are evicted during the
        // write loop, exercising the "WAL written before page" invariant.
        const N: u64 = 50;

        // Phase 1 — write N nodes and N-1 edges, then DROP without flush.
        // This simulates a kill -9 / SIGKILL crash scenario.
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 8).unwrap();
            for i in 1..=N {
                engine.put_node(&test_node(i)).unwrap();
            }
            for i in 1..N {
                // Edge i*100 from node i to node i+1.
                engine.put_edge(&test_edge(i * 100, i, i + 1)).unwrap();
            }
            // Engine dropped here — no explicit flush().
        }

        // Phase 2 — reopen via WAL replay and assert full state is recovered.
        let (engine, max_node_id, max_edge_id) = DiskStorageEngine::open(data_dir).unwrap();

        assert_eq!(max_node_id, N, "max_node_id must match last written node");
        assert_eq!(
            max_edge_id,
            (N - 1) * 100,
            "max_edge_id must match last written edge"
        );

        // Every node must be readable.
        for i in 1..=N {
            assert!(
                engine.get_node(NodeId(i)).unwrap().is_some(),
                "node {i} must survive WAL replay"
            );
        }
        // Every edge must be readable.
        for i in 1..N {
            assert!(
                engine.get_edge(EdgeId(i * 100)).unwrap().is_some(),
                "edge {} must survive WAL replay",
                i * 100
            );
        }

        // Label index must also have been rebuilt by replay.
        let persons = engine.find_nodes_by_label("Person").unwrap();
        assert_eq!(
            persons.len() as u64,
            N,
            "all {N} Person nodes must appear in label index after replay"
        );

        // New writes after recovery must succeed and not corrupt the log.
        engine.put_node(&test_node(N + 1)).unwrap();
        assert!(
            engine.get_node(NodeId(N + 1)).unwrap().is_some(),
            "fresh write after recovery must be readable"
        );
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
        assert_eq!(
            engine.free_page_count(),
            3,
            "deletes populate the free list"
        );

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
        engine
            .put_node(&Node {
                id: NodeId(1),
                labels: vec!["Updated".to_string()],
                properties: serde_json::json!({"new": "content"}),
                embedding: None,
            })
            .unwrap();
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

    /// Verify that a node with a 768-dim embedding round-trips through a
    /// restart via WAL replay using the v1 binary encoding (issue #26 Phase 1).
    ///
    /// Capacity check: a node with ~2 KB of text properties + a 768-dim f32
    /// embedding encodes to ~5.1 KB in v1 binary format (3,072 raw bytes for
    /// the embedding + JSON body overhead).  The same node would have been
    /// ~9 KB under the legacy JSON encoding (~6,912 bytes for the embedding as
    /// a JSON float array + ~2 KB props) and would have failed `write_record`'s
    /// 8,159-byte single-page budget with the old code.
    #[test]
    fn test_binary_embedding_round_trip_and_restart() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();

        // Standard model dimension.
        let embedding: Vec<f32> =
            (0..768u32).map(|i| i as f32 * 0.001_f32 + 0.5_f32).collect();

        // ~2 KB of text ensures that the old JSON-encoded embedding (~6-9 KB)
        // would overflow the 8,159-byte page budget while the v1 binary
        // container (3,072 bytes for embedding + ~2 KB JSON body) fits.
        let content = "A".repeat(2_000);
        let node = Node {
            id: NodeId(1),
            labels: vec!["EmbeddedNote".to_string()],
            properties: serde_json::json!({ "content": content }),
            embedding: Some(embedding.clone()),
        };

        // Phase 1: write and drop without an explicit flush (simulates crash).
        // Small pool forces page eviction, exercising the WAL replay path.
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 8).unwrap();
            engine.put_node(&node).unwrap();
        }

        // Phase 2: reopen via WAL replay.
        let (engine, max_node_id, _) = DiskStorageEngine::open(data_dir).unwrap();
        assert_eq!(max_node_id, 1, "max_node_id must be recovered");

        let recovered = engine
            .get_node(NodeId(1))
            .unwrap()
            .expect("node must survive restart");

        // Embedding must be present and bit-identical (f32 round-trips through
        // to_le_bytes / from_le_bytes with no precision loss).
        assert!(
            recovered.embedding.is_some(),
            "embedding must be present after restart"
        );
        let got_emb = recovered.embedding.unwrap();
        assert_eq!(got_emb.len(), 768, "embedding dimension preserved");
        for (i, (orig, got)) in embedding.iter().zip(got_emb.iter()).enumerate() {
            assert_eq!(
                orig.to_bits(),
                got.to_bits(),
                "embedding[{i}] must be bit-identical: {orig} != {got}"
            );
        }
        assert_eq!(recovered.labels, vec!["EmbeddedNote".to_string()]);
    }

    /// Verify that a node larger than one page succeeds end-to-end and the WAL
    /// is not poisoned (issue #26 Phase 2: overflow chain).
    ///
    /// In Phase 1 this node would have been rejected with `Err` and a
    /// subsequent `open()` on the same directory would succeed with the
    /// oversized node absent.  In Phase 2 the overflow chain absorbs the
    /// extra data, the insert succeeds, and the node round-trips through a
    /// restart via WAL replay.
    #[test]
    fn test_large_node_succeeds_and_survives_restart() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();

        // ~9 KiB property — more than HEAD_PAGE_CAPACITY (8,159 B) — forces
        // overflow even before any embedding bytes are added.
        let big_content = "x".repeat(9_000);

        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 64).unwrap();

            // Write a normal node first so the WAL has at least one other record.
            engine.put_node(&test_node(1)).unwrap();

            let large = Node {
                id: NodeId(2),
                labels: vec!["LargeNode".to_string()],
                properties: serde_json::json!({ "content": big_content }),
                embedding: None,
            };
            // Must succeed — overflow chain handles the extra size.
            engine.put_node(&large).expect("large node must be accepted via overflow");
            let got = engine.get_node(NodeId(2)).unwrap().unwrap();
            assert_eq!(got.labels, vec!["LargeNode".to_string()]);
        }

        // Reopen via WAL replay — the large node must survive.
        let (engine, max_node_id, _) = DiskStorageEngine::open(data_dir)
            .expect("open() must succeed after an overflow-chain insert");

        assert_eq!(max_node_id, 2);
        assert!(engine.get_node(NodeId(1)).unwrap().is_some(), "node 1 must survive");
        let got = engine.get_node(NodeId(2)).unwrap().expect("large node must survive restart");
        assert_eq!(got.labels, vec!["LargeNode".to_string()]);
        let recovered_content = got.properties["content"].as_str().unwrap();
        assert_eq!(recovered_content.len(), 9_000, "full content must round-trip");
    }

    /// Issue #26 headline regression test (storage-level portion).
    ///
    /// A node with a 768-dim embedding AND ~8 KiB of properties has a v1
    /// binary body of roughly:
    ///   1 (tag) + 4 (json_len) + ~8,040 (JSON props) + 4 (dim) + 3,072 (f32 bytes)
    ///   ≈ 11,121 bytes → spans at least 2 pages.
    ///
    /// Asserts:
    ///   1. `put_node` succeeds.
    ///   2. `get_node` returns the node with `embedding.is_some()` and bit-identical f32s.
    ///   3. The node round-trips through a restart (WAL replay reconstructs the chain).
    #[test]
    fn test_large_embedding_plus_props_round_trip_and_restart() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();

        let embedding: Vec<f32> = (0..768u32).map(|i| i as f32 * 0.001 + 0.5).collect();
        // ~8 KB of JSON text properties.
        let large_prop = "A".repeat(8_000);
        let node = Node {
            id: NodeId(42),
            labels: vec!["BigNode".to_string()],
            properties: serde_json::json!({ "content": large_prop, "idx": 42 }),
            embedding: Some(embedding.clone()),
        };

        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();
            engine.put_node(&node).expect("large embedding+props node must succeed");

            // Verify immediately after write.
            let got = engine.get_node(NodeId(42)).unwrap().unwrap();
            assert!(got.embedding.is_some(), "embedding must be present after write");
            let emb = got.embedding.unwrap();
            assert_eq!(emb.len(), 768);
            for (i, (orig, got)) in embedding.iter().zip(emb.iter()).enumerate() {
                assert_eq!(
                    orig.to_bits(),
                    got.to_bits(),
                    "embedding[{i}] must be bit-identical after write"
                );
            }
        }

        // Restart and verify via WAL replay.
        let (engine, max_node_id, _) = DiskStorageEngine::open(data_dir).unwrap();
        assert_eq!(max_node_id, 42);

        let recovered = engine
            .get_node(NodeId(42))
            .unwrap()
            .expect("node must survive restart");

        assert!(recovered.embedding.is_some(), "embedding must survive restart");
        let got_emb = recovered.embedding.unwrap();
        assert_eq!(got_emb.len(), 768, "embedding dimension preserved after restart");
        for (i, (orig, got)) in embedding.iter().zip(got_emb.iter()).enumerate() {
            assert_eq!(
                orig.to_bits(),
                got.to_bits(),
                "embedding[{i}] must be bit-identical after restart"
            );
        }
        assert_eq!(recovered.labels, vec!["BigNode".to_string()]);
        assert_eq!(
            recovered.properties["content"].as_str().unwrap().len(),
            8_000
        );
    }

    /// Verify that a record spanning 3 or more pages writes and reads back
    /// correctly.  A body of 3 × OVERFLOW_PAGE_PAYLOAD bytes forces the head
    /// page plus at least 2 overflow pages.
    #[test]
    fn test_record_spanning_three_pages() {
        let (engine, _tmp) = make_engine();

        // Build a property string large enough to need 3 pages:
        // HEAD_PAGE_CAPACITY (8159) + 2 × OVERFLOW_PAGE_PAYLOAD (8167) = 24,493 bytes
        // We use ~25 KB to be safely beyond 2 pages.
        let content = "Z".repeat(25_000);
        let node = Node {
            id: NodeId(7),
            labels: vec!["ThreePage".to_string()],
            properties: serde_json::json!({ "content": content }),
            embedding: None,
        };

        engine.put_node(&node).expect("3-page node must be stored");
        let got = engine.get_node(NodeId(7)).unwrap().unwrap();
        assert_eq!(got.labels, vec!["ThreePage".to_string()]);
        assert_eq!(got.properties["content"].as_str().unwrap().len(), 25_000);
    }

    /// Verify that deleting a chained (multi-page) node frees ALL its pages
    /// (head + overflow) and those pages are reusable for subsequent writes.
    #[test]
    fn test_delete_chained_node_frees_all_pages() {
        let (engine, _tmp) = make_engine();

        // Insert a large (2-page) node.
        let content = "M".repeat(10_000); // > HEAD_PAGE_CAPACITY → 2 pages
        let large_node = Node {
            id: NodeId(1),
            labels: vec!["Large".to_string()],
            properties: serde_json::json!({ "content": content }),
            embedding: None,
        };
        engine.put_node(&large_node).unwrap();

        let pages_after_insert = engine.file_manager.page_count().unwrap();
        // 1 head + 1 overflow = 2 pages
        assert_eq!(pages_after_insert, 2, "large node must allocate 2 pages");
        assert_eq!(engine.free_page_count(), 0);

        // Delete it — both pages must go to the free list.
        assert!(engine.delete_node(NodeId(1)).unwrap());
        assert_eq!(engine.free_page_count(), 2, "both pages must be freed");
        assert!(engine.get_node(NodeId(1)).unwrap().is_none());

        // Insert a new large node — must reuse the freed pages, not grow the file.
        let content2 = "N".repeat(10_000);
        let large_node2 = Node {
            id: NodeId(2),
            labels: vec!["Large2".to_string()],
            properties: serde_json::json!({ "content": content2 }),
            embedding: None,
        };
        engine.put_node(&large_node2).unwrap();
        assert_eq!(engine.free_page_count(), 0, "freed pages should be consumed");
        let pages_after_reuse = engine.file_manager.page_count().unwrap();
        assert_eq!(
            pages_after_reuse, pages_after_insert,
            "file must not grow: freed pages were reused"
        );

        // The new node is readable and correct.
        let got = engine.get_node(NodeId(2)).unwrap().unwrap();
        assert_eq!(got.properties["content"].as_str().unwrap().len(), 10_000);
    }

    /// Verify that a restart after deleting a chained node produces a clean
    /// mount (no WAL replay failure, no stale index entries).
    #[test]
    fn test_restart_after_chained_delete_is_clean() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();

        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 16).unwrap();

            // Insert two nodes: one large (overflow) and one small.
            let large_content = "K".repeat(10_000);
            let large = Node {
                id: NodeId(10),
                labels: vec!["Large".to_string()],
                properties: serde_json::json!({ "content": large_content }),
                embedding: None,
            };
            engine.put_node(&large).unwrap();
            engine.put_node(&test_node(20)).unwrap();

            // Delete the large node.
            engine.delete_node(NodeId(10)).unwrap();
        }

        // Reopen — WAL replay must succeed cleanly.
        let (engine, max_node_id, _) = DiskStorageEngine::open(data_dir)
            .expect("open() must succeed after chained-node delete");

        assert_eq!(max_node_id, 20);
        // Large node must be gone.
        assert!(engine.get_node(NodeId(10)).unwrap().is_none(), "deleted large node must not exist");
        // Small node must survive.
        assert!(engine.get_node(NodeId(20)).unwrap().is_some(), "small node must survive");

        // New writes after recovery must work.
        engine.put_node(&test_node(30)).unwrap();
        assert!(engine.get_node(NodeId(30)).unwrap().is_some());
    }

    /// Regression test for WAL cursor-at-zero data-corruption bug.
    ///
    /// Before the fix, `WalWriter::new` opened the file with `.write(true)`,
    /// which left the OS cursor at offset 0. After `DiskStorageEngine::open`
    /// replayed the WAL, the very first `put_node` call would write the new
    /// record's WAL entry starting at byte 0, silently overwriting the existing
    /// InsertNode(A) record, while `current_lsn` still reported the old (larger)
    /// length. A subsequent restart would then replay a corrupted record for A
    /// (wrong bytes / CRC mismatch) and lose the node.
    ///
    /// After the fix (`.append(true)` on the WAL file), every write is
    /// atomically positioned at EOF by the kernel, so existing records survive
    /// across multiple open/write/close cycles.
    #[test]
    fn test_wal_no_overwrite_on_reopen() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path();

        // Session 1: insert nodes A (id=1) and B (id=2), then drop.
        {
            let engine = DiskStorageEngine::with_pool_size(data_dir, 64).unwrap();
            engine.put_node(&test_node(1)).unwrap();
            engine.put_node(&test_node(2)).unwrap();
            // Drop without explicit flush — simulates a clean process exit that
            // did not call flush().  WAL records for A and B are on disk.
        }

        // Session 2: open (WAL replay sees A and B), insert node C (id=3), drop.
        // This is where the bug struck: before the fix, the first put_node
        // call wrote the InsertNode(C) WAL record starting at offset 0, which
        // overwrote the InsertNode(A) record.
        {
            let (engine, max_node_id, _) = DiskStorageEngine::open(data_dir).unwrap();
            assert_eq!(max_node_id, 2, "session 2 must recover A and B (max_node_id=2)");
            assert!(
                engine.get_node(NodeId(1)).unwrap().is_some(),
                "A must be visible after session-1 WAL replay"
            );
            assert!(
                engine.get_node(NodeId(2)).unwrap().is_some(),
                "B must be visible after session-1 WAL replay"
            );
            engine.put_node(&test_node(3)).unwrap();
        }

        // Session 3: final open. ALL THREE of A, B, C must be present.
        // Before the fix this would fail: either a CRC error on A's corrupted
        // record, or A simply missing from the index because the replay saw
        // InsertNode(C) where InsertNode(A) used to be.
        let (engine, max_node_id, _) =
            DiskStorageEngine::open(data_dir).expect("third open must succeed without CRC errors");

        assert_eq!(
            max_node_id, 3,
            "all three nodes written; max_node_id must be 3 (got {max_node_id})"
        );
        assert!(
            engine.get_node(NodeId(1)).unwrap().is_some(),
            "node A (id=1) must survive two reopen cycles"
        );
        assert!(
            engine.get_node(NodeId(2)).unwrap().is_some(),
            "node B (id=2) must survive two reopen cycles"
        );
        assert!(
            engine.get_node(NodeId(3)).unwrap().is_some(),
            "node C (id=3, written in session 2) must survive the final reopen"
        );
    }

    /// Verify that updating a chained node frees its old chain and the file
    /// does not grow unboundedly.
    #[test]
    fn test_update_chained_node_frees_old_chain() {
        let (engine, _tmp) = make_engine();

        let content = "U".repeat(10_000);
        let node = Node {
            id: NodeId(1),
            labels: vec!["UpdateMe".to_string()],
            properties: serde_json::json!({ "content": content }),
            embedding: None,
        };
        engine.put_node(&node).unwrap();
        let pages_after_first = engine.file_manager.page_count().unwrap();
        assert_eq!(pages_after_first, 2); // head + 1 overflow

        // Overwrite with another large node.
        let content2 = "V".repeat(10_000);
        let node2 = Node {
            id: NodeId(1),
            labels: vec!["Updated".to_string()],
            properties: serde_json::json!({ "content": content2 }),
            embedding: None,
        };
        engine.put_node(&node2).unwrap();

        let pages_after_update = engine.file_manager.page_count().unwrap();
        assert_eq!(
            pages_after_update, pages_after_first,
            "update must reuse old chain pages, not grow the file"
        );
        let got = engine.get_node(NodeId(1)).unwrap().unwrap();
        assert_eq!(got.labels, vec!["Updated".to_string()]);
        assert_eq!(got.properties["content"].as_str().unwrap().len(), 10_000);
    }
}

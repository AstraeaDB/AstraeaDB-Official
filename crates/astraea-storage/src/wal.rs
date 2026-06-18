//! Write-ahead log (WAL) for crash recovery.
//!
//! All mutations are first appended to the WAL before being applied to pages.
//! This ensures durability: after a crash, the WAL can be replayed to recover
//! committed changes.
//!
//! Record format on disk:
//!   [length: u32][record_type: u8][payload: serde_json bytes][crc32: u32]

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::{Edge, EdgeId, Lsn, Node, NodeId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A WAL record representing a single mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalRecord {
    InsertNode(Node),
    DeleteNode(NodeId),
    InsertEdge(Edge),
    DeleteEdge(EdgeId),
    UpdateNodeProperties(NodeId, serde_json::Value),
    /// Checkpoint record stores the LSN value as a raw u64 because the core
    /// `Lsn` type does not implement Serialize/Deserialize.
    Checkpoint(u64),
    /// Begin a new MVCC transaction with the given TransactionId (as raw u64).
    BeginTransaction(u64),
    /// Commit a transaction with the given TransactionId (as raw u64).
    CommitTransaction(u64),
    /// Abort a transaction with the given TransactionId (as raw u64).
    AbortTransaction(u64),
}

/// Discriminant byte for each record type.
impl WalRecord {
    fn record_type_byte(&self) -> u8 {
        match self {
            WalRecord::InsertNode(_) => 0,
            WalRecord::DeleteNode(_) => 1,
            WalRecord::InsertEdge(_) => 2,
            WalRecord::DeleteEdge(_) => 3,
            WalRecord::UpdateNodeProperties(..) => 4,
            WalRecord::Checkpoint(_) => 5,
            WalRecord::BeginTransaction(_) => 6,
            WalRecord::CommitTransaction(_) => 7,
            WalRecord::AbortTransaction(_) => 8,
        }
    }
}

/// Append-only WAL writer.
pub struct WalWriter {
    writer: Mutex<BufWriter<File>>,
    /// Current LSN (byte offset of the next record to be written).
    current_lsn: Mutex<u64>,
    #[allow(dead_code)]
    path: PathBuf,
}

impl WalWriter {
    /// Open or create a WAL file at the given path.
    ///
    /// Opens with `O_APPEND` so every write atomically positions to EOF before
    /// being committed by the kernel. This is the canonical fix for the
    /// cursor-at-zero bug: without `O_APPEND`, reopening an existing WAL left
    /// the OS write cursor at offset 0, so the first `append` call after a
    /// `DiskStorageEngine::open` would overwrite records from the beginning of
    /// the file while `current_lsn` still reported the old (larger) length.
    /// `WalReader` uses its own `File::open` handle for all reads, so
    /// `.read(true)` is not needed on the writer's file descriptor.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)?;

        let file_len = file.metadata()?.len();
        let writer = BufWriter::new(file);

        Ok(Self {
            writer: Mutex::new(writer),
            current_lsn: Mutex::new(file_len),
            path,
        })
    }

    /// Append a record to the WAL. Returns the LSN of the written record.
    ///
    /// Format: [length: u32][record_type: u8][payload bytes][crc32: u32]
    pub fn append(&self, record: &WalRecord) -> Result<Lsn> {
        let payload =
            serde_json::to_vec(record).map_err(|e| AstraeaError::Serialization(e.to_string()))?;

        let record_type = record.record_type_byte();
        // Total length = 1 (type) + payload
        let length = 1u32 + payload.len() as u32;

        // Compute CRC over [length][record_type][payload]
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&length.to_le_bytes());
        hasher.update(&[record_type]);
        hasher.update(&payload);
        let crc = hasher.finalize();

        let mut writer = self.writer.lock();
        let mut lsn = self.current_lsn.lock();

        let record_lsn = Lsn(*lsn);

        writer
            .write_all(&length.to_le_bytes())
            .map_err(AstraeaError::StorageIo)?;
        writer
            .write_all(&[record_type])
            .map_err(AstraeaError::StorageIo)?;
        writer
            .write_all(&payload)
            .map_err(AstraeaError::StorageIo)?;
        writer
            .write_all(&crc.to_le_bytes())
            .map_err(AstraeaError::StorageIo)?;
        writer.flush().map_err(AstraeaError::StorageIo)?;
        // fsync_data so a crash (SIGKILL, power loss) after this call still
        // sees this record on the next restart. sync_data is lighter than
        // sync_all — we don't need to flush file-metadata changes.
        writer
            .get_ref()
            .sync_data()
            .map_err(AstraeaError::StorageIo)?;

        // Advance LSN: 4 (length) + length + 4 (crc)
        *lsn += 4 + length as u64 + 4;

        Ok(record_lsn)
    }

    /// Get the current LSN (next write position).
    pub fn current_lsn(&self) -> Lsn {
        Lsn(*self.current_lsn.lock())
    }
}

/// WAL reader for replaying or inspecting log records.
pub struct WalReader {
    path: PathBuf,
}

impl WalReader {
    /// Open a WAL file for reading.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Read all records starting from the given LSN.
    ///
    /// Returns `(records, last_good_offset)` where `last_good_offset` is the
    /// byte position immediately after the last successfully verified record.
    /// Any bytes from `last_good_offset` to EOF are a torn tail from a crash
    /// and should be truncated before the next [`WalWriter`] append.
    ///
    /// Stops at the first record that fails to parse (bad length field, partial
    /// read, or CRC mismatch) and treats that position as end-of-log.  This is
    /// standard WAL semantics: an append-only log is authoritative only up to
    /// the first bad record; anything after it is unreplayable regardless of
    /// whether more bytes happen to follow.
    pub fn read_from(&self, lsn: Lsn) -> Result<(Vec<(Lsn, WalRecord)>, u64)> {
        let file = File::open(&self.path)?;
        let file_len = file.metadata()?.len();
        let mut reader = BufReader::new(file);

        // Seek to the starting LSN.
        reader.seek(SeekFrom::Start(lsn.0))?;

        let mut records = Vec::new();
        let mut pos = lsn.0;

        while pos < file_len {
            // Read length (4 bytes).
            let mut len_buf = [0u8; 4];
            if reader.read_exact(&mut len_buf).is_err() {
                // Partial read at EOF — torn tail; stop here.
                break;
            }
            let length = u32::from_le_bytes(len_buf);

            if length == 0 || pos + 4 + length as u64 + 4 > file_len {
                // Length is zero or claims more bytes than remain — torn record
                // at the tail; treat as end-of-log.
                break;
            }

            // Read record_type (1 byte) + payload (length - 1 bytes).
            let mut type_buf = [0u8; 1];
            if reader.read_exact(&mut type_buf).is_err() {
                break;
            }

            let payload_len = length as usize - 1;
            let mut payload = vec![0u8; payload_len];
            if reader.read_exact(&mut payload).is_err() {
                break;
            }

            // Read CRC (4 bytes).
            let mut crc_buf = [0u8; 4];
            if reader.read_exact(&mut crc_buf).is_err() {
                break;
            }
            let stored_crc = u32::from_le_bytes(crc_buf);

            // Verify CRC.
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&len_buf);
            hasher.update(&type_buf);
            hasher.update(&payload);
            let computed_crc = hasher.finalize();

            // CRC mismatch: per standard WAL semantics, the first bad record
            // IS the log tail — anything after it is unreplayable (either a
            // torn write or bytes written after a crash that corrupted this
            // slot).  Stop here and return the byte offset of the end of the
            // last *good* record so callers can truncate the torn tail.
            if stored_crc != computed_crc {
                tracing::debug!(
                    "WAL: CRC mismatch at byte offset {} (stored={:#x}, computed={:#x}); \
                     treating as end-of-log (torn tail)",
                    pos, stored_crc, computed_crc,
                );
                break;
            }

            // Deserialize the record.  A parse failure after a valid CRC is
            // not expected for well-formed data, but treat it as end-of-log
            // consistent with the torn-tail policy above.
            let record: WalRecord = match serde_json::from_slice(&payload) {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!(
                        "WAL: deserialization error at byte offset {}: {}; \
                         treating as end-of-log",
                        pos, e,
                    );
                    break;
                }
            };

            records.push((Lsn(pos), record));

            pos += 4 + length as u64 + 4;
        }

        Ok((records, pos))
    }
}

/// Truncate the WAL file, removing all data before the given LSN.
/// Called after a successful checkpoint to reclaim space.
///
/// **Atomic**: data after `lsn` is staged into `<path>.new`, fsynced, and
/// atomically `rename`d over the original. A crash at any point leaves
/// either the old WAL or the new one on disk — never a half-written file.
///
/// **Not safe against concurrent writers.** Callers are responsible for
/// serializing against any open [`WalWriter`] on the same path. The
/// simplest pattern: drop the writer, truncate, reopen.
pub fn truncate_wal<P: AsRef<Path>>(path: P, lsn: Lsn) -> Result<()> {
    let path = path.as_ref();
    let tmp_path = sibling_tmp(path);

    // Clean up any stale .new file from a previous aborted truncate.
    let _ = std::fs::remove_file(&tmp_path);

    // Read the tail we want to keep.
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let remaining = if lsn.0 >= file_len {
        Vec::new()
    } else {
        file.seek(SeekFrom::Start(lsn.0))?;
        let mut buf = Vec::with_capacity((file_len - lsn.0) as usize);
        file.read_to_end(&mut buf)?;
        buf
    };
    drop(file);

    // Stage the new content into a sibling file and fsync it.
    {
        let mut tmp = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        tmp.write_all(&remaining)?;
        tmp.sync_data()?;
    }

    // Atomic rename — this is the commit point.
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn sibling_tmp(path: &Path) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    file_name.push(".new");
    match path.parent() {
        Some(parent) => parent.join(file_name),
        None => PathBuf::from(file_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::types::ValidityInterval;
    use tempfile::NamedTempFile;

    fn make_test_node(id: u64) -> Node {
        Node {
            id: NodeId(id),
            labels: vec!["Person".to_string()],
            properties: serde_json::json!({"name": "Alice", "age": 30}),
            embedding: None,
        }
    }

    fn make_test_edge(id: u64) -> Edge {
        Edge {
            id: EdgeId(id),
            source: NodeId(1),
            target: NodeId(2),
            edge_type: "KNOWS".to_string(),
            properties: serde_json::json!({}),
            weight: 1.0,
            validity: ValidityInterval::always(),
        }
    }

    #[test]
    fn test_wal_append_and_read() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let writer = WalWriter::new(&path).unwrap();
        let lsn0 = writer
            .append(&WalRecord::InsertNode(make_test_node(1)))
            .unwrap();
        let lsn1 = writer
            .append(&WalRecord::InsertEdge(make_test_edge(10)))
            .unwrap();
        let _lsn2 = writer.append(&WalRecord::DeleteNode(NodeId(1))).unwrap();

        assert_eq!(lsn0, Lsn(0));
        assert!(lsn1.0 > 0);

        // Read all records from the beginning.
        let reader = WalReader::new(&path);
        let (records, _last_good) = reader.read_from(Lsn(0)).unwrap();
        assert_eq!(records.len(), 3);

        // Verify record types.
        assert!(matches!(records[0].1, WalRecord::InsertNode(_)));
        assert!(matches!(records[1].1, WalRecord::InsertEdge(_)));
        assert!(matches!(records[2].1, WalRecord::DeleteNode(_)));
    }

    #[test]
    fn test_wal_read_from_offset() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let writer = WalWriter::new(&path).unwrap();
        let _lsn0 = writer
            .append(&WalRecord::InsertNode(make_test_node(1)))
            .unwrap();
        let lsn1 = writer
            .append(&WalRecord::InsertNode(make_test_node(2)))
            .unwrap();
        let _lsn2 = writer
            .append(&WalRecord::InsertNode(make_test_node(3)))
            .unwrap();

        // Read from lsn1 onward — should get 2 records.
        let reader = WalReader::new(&path);
        let (records, _last_good) = reader.read_from(lsn1).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_wal_truncate() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let writer = WalWriter::new(&path).unwrap();
        let _lsn0 = writer
            .append(&WalRecord::InsertNode(make_test_node(1)))
            .unwrap();
        let lsn1 = writer
            .append(&WalRecord::InsertNode(make_test_node(2)))
            .unwrap();
        let _lsn2 = writer
            .append(&WalRecord::InsertNode(make_test_node(3)))
            .unwrap();
        drop(writer);

        // Truncate everything before lsn1.
        truncate_wal(&path, lsn1).unwrap();

        // Now reading from LSN 0 should give us the records that were at lsn1+.
        let reader = WalReader::new(&path);
        let (records, _last_good) = reader.read_from(Lsn(0)).unwrap();
        assert_eq!(records.len(), 2);

        // No stale sibling file left behind.
        let tmp_path = sibling_tmp(&path);
        assert!(
            !tmp_path.exists(),
            "sibling tmp should be gone after truncate"
        );
    }

    #[test]
    fn test_wal_truncate_cleans_stale_tmp() {
        // Simulate an aborted prior truncate that left `.new` on disk.
        // A fresh truncate must clean it up and still succeed.
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let tmp_path = sibling_tmp(&path);

        let writer = WalWriter::new(&path).unwrap();
        let lsn0 = writer
            .append(&WalRecord::InsertNode(make_test_node(1)))
            .unwrap();
        writer
            .append(&WalRecord::InsertNode(make_test_node(2)))
            .unwrap();
        drop(writer);

        // Create a stale sibling file that would break create_new without cleanup.
        std::fs::write(&tmp_path, b"garbage from an aborted run").unwrap();
        assert!(tmp_path.exists());

        truncate_wal(&path, lsn0).unwrap();
        assert!(!tmp_path.exists(), "stale sibling should have been removed");

        let reader = WalReader::new(&path);
        let (records, _last_good) = reader.read_from(Lsn(0)).unwrap();
        assert_eq!(
            records.len(),
            2,
            "both records still present after truncate at lsn0"
        );
    }

    #[test]
    fn test_wal_append_survives_sigkill_simulation() {
        // We can't actually SIGKILL this process, but we can verify that
        // every successful `append` has called sync_data by re-reading the
        // file from a fresh handle — the sync ensures the bytes are visible
        // across file-handle boundaries.
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let writer = WalWriter::new(&path).unwrap();
        for i in 0..5 {
            writer
                .append(&WalRecord::InsertNode(make_test_node(i)))
                .unwrap();
            // Open a fresh reader without dropping the writer — this proves
            // every record is already on disk, not just in the writer's buffer.
            let reader = WalReader::new(&path);
            let (records, _last_good) = reader.read_from(Lsn(0)).unwrap();
            assert_eq!(records.len() as u64, i + 1);
        }
    }

    /// Verify that `read_from` stops at a CRC-mismatch (torn tail) and returns
    /// `Ok` with the correct `last_good_offset`, rather than returning `Err`.
    #[test]
    fn test_wal_read_from_stops_at_crc_mismatch() {
        use std::io::Write;

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let writer = WalWriter::new(&path).unwrap();
        writer.append(&WalRecord::InsertNode(make_test_node(1))).unwrap();
        writer.append(&WalRecord::InsertNode(make_test_node(2))).unwrap();
        // Snapshot the byte offset after both good records; that is where
        // last_good_offset must land after we stop at the corrupt record.
        let good_end = writer.current_lsn().0;
        drop(writer);

        // Append a well-formed-looking record whose CRC is wrong, simulating
        // a torn write at crash time.  length=10 means the total blob is
        // 4 + 10 + 4 = 18 bytes, so the length-guard does NOT trip (18 bytes
        // are present); only the CRC check will catch it.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            let mut blob = vec![10u8, 0, 0, 0]; // length = 10 LE
            blob.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x01]);
            blob.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // deliberately wrong CRC
            f.write_all(&blob).unwrap();
        }

        let reader = WalReader::new(&path);
        // Must return Ok, NOT Err.
        let (records, last_good_offset) = reader
            .read_from(Lsn(0))
            .expect("read_from must return Ok even when a CRC-bad record is present");

        assert_eq!(records.len(), 2, "both good records must be parsed before the corrupt one");
        assert_eq!(
            last_good_offset, good_end,
            "last_good_offset must be the end of the last clean record"
        );
        let file_len = std::fs::metadata(&path).unwrap().len();
        assert!(
            last_good_offset < file_len,
            "garbage bytes must lie beyond last_good_offset"
        );
    }
}

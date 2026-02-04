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
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
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
        let payload = serde_json::to_vec(record)
            .map_err(|e| AstraeaError::Serialization(e.to_string()))?;

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
    /// Returns a vector of (Lsn, WalRecord) pairs.
    pub fn read_from(&self, lsn: Lsn) -> Result<Vec<(Lsn, WalRecord)>> {
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
                break;
            }
            let length = u32::from_le_bytes(len_buf);

            if length == 0 || pos + 4 + length as u64 + 4 > file_len {
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

            if stored_crc != computed_crc {
                return Err(AstraeaError::Deserialization(format!(
                    "WAL CRC mismatch at LSN {}: stored={:#x}, computed={:#x}",
                    pos, stored_crc, computed_crc
                )));
            }

            // Deserialize the record.
            let record: WalRecord = serde_json::from_slice(&payload)
                .map_err(|e| AstraeaError::Deserialization(e.to_string()))?;

            records.push((Lsn(pos), record));

            pos += 4 + length as u64 + 4;
        }

        Ok(records)
    }
}

/// Truncate the WAL file, removing all data before the given LSN.
/// This is called after a successful checkpoint to reclaim space.
///
/// Implementation: reads everything from `lsn` onward, truncates the file,
/// and writes the remaining data back. In production this would be done via
/// log rotation, but this is sufficient for the initial implementation.
pub fn truncate_wal<P: AsRef<Path>>(path: P, lsn: Lsn) -> Result<()> {
    let path = path.as_ref();

    // Read remaining records.
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();

    if lsn.0 >= file_len {
        // Truncate to empty.
        let file = OpenOptions::new().write(true).truncate(true).open(path)?;
        drop(file);
        return Ok(());
    }

    file.seek(SeekFrom::Start(lsn.0))?;
    let mut remaining = Vec::new();
    file.read_to_end(&mut remaining)?;
    drop(file);

    // Rewrite the file with only the remaining data.
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(&remaining)?;
    file.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::types::{ValidityInterval};
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
        let lsn0 = writer.append(&WalRecord::InsertNode(make_test_node(1))).unwrap();
        let lsn1 = writer.append(&WalRecord::InsertEdge(make_test_edge(10))).unwrap();
        let _lsn2 = writer.append(&WalRecord::DeleteNode(NodeId(1))).unwrap();

        assert_eq!(lsn0, Lsn(0));
        assert!(lsn1.0 > 0);

        // Read all records from the beginning.
        let reader = WalReader::new(&path);
        let records = reader.read_from(Lsn(0)).unwrap();
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
        let _lsn0 = writer.append(&WalRecord::InsertNode(make_test_node(1))).unwrap();
        let lsn1 = writer.append(&WalRecord::InsertNode(make_test_node(2))).unwrap();
        let _lsn2 = writer.append(&WalRecord::InsertNode(make_test_node(3))).unwrap();

        // Read from lsn1 onward — should get 2 records.
        let reader = WalReader::new(&path);
        let records = reader.read_from(lsn1).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_wal_truncate() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let writer = WalWriter::new(&path).unwrap();
        let _lsn0 = writer.append(&WalRecord::InsertNode(make_test_node(1))).unwrap();
        let lsn1 = writer.append(&WalRecord::InsertNode(make_test_node(2))).unwrap();
        let _lsn2 = writer.append(&WalRecord::InsertNode(make_test_node(3))).unwrap();
        drop(writer);

        // Truncate everything before lsn1.
        truncate_wal(&path, lsn1).unwrap();

        // Now reading from LSN 0 should give us the records that were at lsn1+.
        let reader = WalReader::new(&path);
        let records = reader.read_from(Lsn(0)).unwrap();
        assert_eq!(records.len(), 2);
    }
}

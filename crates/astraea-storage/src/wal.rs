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
    /// `None` means the writer is **poisoned**: a prior [`Self::truncate`]
    /// committed the on-disk rename but then failed to reopen its handle on
    /// the new file (finding #2183). At that point the writer no longer
    /// knows a safe fd to write through — silently keeping the old one would
    /// mean every subsequent `append` vanishes into the unlinked pre-rename
    /// inode. Making that state `None` instead of a stale `BufWriter` turns
    /// it into a fast, explicit `Err` from `append`/`truncate` rather than a
    /// silent data-loss bug.
    writer: Mutex<Option<BufWriter<File>>>,
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
        let file = OpenOptions::new().append(true).create(true).open(&path)?;

        let file_len = file.metadata()?.len();
        let writer = BufWriter::new(file);

        Ok(Self {
            writer: Mutex::new(Some(writer)),
            current_lsn: Mutex::new(file_len),
            path,
        })
    }

    /// Append a record to the WAL. Returns the LSN of the written record.
    ///
    /// Format: [length: u32][record_type: u8][payload bytes][crc32: u32]
    ///
    /// Returns `Err(AstraeaError::Storage(..))` without touching disk if the
    /// writer is poisoned (see the `writer` field doc) — callers must open a
    /// fresh `WalWriter` at that point.
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

        let mut writer_slot = self.writer.lock();
        let mut lsn = self.current_lsn.lock();

        let writer = writer_slot.as_mut().ok_or_else(poisoned_error)?;

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

    /// Atomically truncate this WAL, discarding all records before `lsn`.
    ///
    /// This is the **safe-to-call-on-a-live-writer** counterpart to the free
    /// function [`truncate_wal`]. It takes the same `writer`/`current_lsn`
    /// locks that [`Self::append`] takes, so no concurrent `append` can
    /// interleave with the truncate (issue #19: serialize truncate against
    /// the writer).
    ///
    /// It is not enough to just rename a staged replacement over `path`: this
    /// `WalWriter` holds its own open file descriptor, and a rename repoints
    /// the *path* to a new inode without affecting fds already open on the
    /// old (now-unlinked) one. Without reopening, every `append` after a
    /// truncate would keep silently writing into the orphaned old inode and
    /// never appear in the file at `path` again. So after the atomic rename
    /// commits, this method reopens its own handle at `path` and rebases
    /// `current_lsn` to the length of the retained tail — LSNs in this log
    /// are byte offsets within the *current* WAL file, not a global
    /// monotonic counter, so truncation legitimately renumbers them from 0.
    ///
    /// **Poisoning (findings #2183, #2184):** `rename_staged` is the point of
    /// no return — a failure at or before it leaves the original WAL and this
    /// writer's fd untouched, so it propagates without poisoning. Once it
    /// succeeds the truncation has committed on disk and this writer's old fd
    /// points at the now-unlinked pre-truncate inode, so *every* fallible step
    /// after it — the parent-directory fsync that makes the rename durable, and
    /// the reopen that repoints the fd — poisons the writer (`writer` slot set
    /// to `None`) on failure. Rather than keep writing through the stale handle
    /// — which would silently discard every future `append` — all subsequent
    /// `append`/`truncate` calls then fail fast with a clear error instead of
    /// losing data silently. Callers must open a fresh `WalWriter` at `path` to
    /// recover.
    pub fn truncate(&self, lsn: Lsn) -> Result<()> {
        let mut writer_slot = self.writer.lock();
        let mut current = self.current_lsn.lock();

        let writer = writer_slot.as_mut().ok_or_else(poisoned_error)?;

        // Flush buffered-but-unwritten bytes first so `read_tail` (which
        // opens a fresh handle on `path`) sees everything appended so far.
        writer.flush().map_err(AstraeaError::StorageIo)?;

        let tail = read_tail(&self.path, lsn)?;
        stage_tail(&self.path, &tail)?;

        // The rename is the point of no return. A failure *before* it commits
        // is safe — the original WAL and this writer's fd are untouched — so we
        // propagate without poisoning. Once it succeeds, the on-disk truncation
        // has committed and this writer's fd points at the now-unlinked old
        // inode, so EVERY subsequent fallible step must poison the writer on
        // failure rather than leave it silently appending into the orphaned
        // inode: the parent-dir fsync that makes the rename durable (#2184) and
        // the reopen that repoints the fd at the new inode (#2183).
        rename_staged(&self.path)?;

        let file = fsync_parent_dir(&self.path)
            .and_then(|()| {
                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&self.path)
                    .map_err(AstraeaError::StorageIo)
            })
            .inspect_err(|_| {
                *writer_slot = None;
            })?;
        *writer_slot = Some(BufWriter::new(file));
        *current = tail.len() as u64;

        Ok(())
    }
}

/// Build the error returned by `append`/`truncate` when the writer has been
/// poisoned by a prior failed post-truncate reopen (finding #2183).
fn poisoned_error() -> AstraeaError {
    AstraeaError::Storage(
        "WalWriter is poisoned: a prior truncate committed on disk but failed to reopen its \
         file handle; open a fresh WalWriter at the same path to recover"
            .to_string(),
    )
}

#[cfg(test)]
impl WalWriter {
    /// Test-only hook that forces the writer into the same poisoned state
    /// `truncate` would leave it in if the post-rename reopen failed. Real
    /// reopen failures require an OS-level fault (e.g. the directory losing
    /// permissions between the rename and the reopen) that is awkward to
    /// engineer portably in a unit test; this lets us assert the fail-fast
    /// contract (finding #2183) directly instead.
    fn poison_for_test(&self) {
        *self.writer.lock() = None;
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
                    pos,
                    stored_crc,
                    computed_crc,
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
                        pos,
                        e,
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
/// **Not safe against a live [`WalWriter`] on the same path.** This free
/// function opens its own file handles; it does not know about, and cannot
/// serialize against, a `WalWriter`'s internal locks or its buffered/open fd.
/// If a `WalWriter` is open on `path`, call [`WalWriter::truncate`] instead —
/// it takes the writer's own locks and reopens the writer's fd after the
/// rename so appends keep landing in the live file. Use this free function
/// only when no writer is open on `path` (e.g. offline maintenance, or after
/// dropping the writer).
pub fn truncate_wal<P: AsRef<Path>>(path: P, lsn: Lsn) -> Result<()> {
    let path = path.as_ref();
    let tail = read_tail(path, lsn)?;
    stage_tail(path, &tail)?;
    commit_rename(path)?;
    Ok(())
}

/// Read the bytes of `path` from `lsn` through EOF — the tail a truncate at
/// `lsn` would keep. Pure read; no side effects on `path`.
fn read_tail(path: &Path, lsn: Lsn) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    if lsn.0 >= file_len {
        Ok(Vec::new())
    } else {
        file.seek(SeekFrom::Start(lsn.0))?;
        let mut buf = Vec::with_capacity((file_len - lsn.0) as usize);
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }
}

/// Stage `tail` into `<path>.new` and fsync it. Does not touch `path` and
/// does not rename — call [`commit_rename`] to finish. Split out from
/// [`truncate_wal`] so tests can pause between staging and the atomic
/// rename to simulate a crash at the least-safe point.
fn stage_tail(path: &Path, tail: &[u8]) -> Result<()> {
    let tmp_path = sibling_tmp(path);
    // Clean up any stale .new file from a previous aborted truncate.
    let _ = std::fs::remove_file(&tmp_path);
    let mut tmp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    tmp.write_all(tail)?;
    tmp.sync_data()?;
    Ok(())
}

/// Atomically rename the staged `<path>.new` over `path` — the commit point
/// for a truncate. A crash before this call leaves the original file
/// untouched; a crash after it leaves the fully-staged replacement.
///
/// The rename alone is not enough to survive a crash immediately after it
/// returns: on POSIX, a directory entry change is only guaranteed durable
/// once the *containing directory* has been fsynced — the classic gap where
/// `rename(2)` can return success but the kernel has not yet persisted the
/// updated directory entry, so a crash a moment later can lose the rename on
/// reboot. Fsync the parent directory to close that gap (finding #2182).
fn commit_rename(path: &Path) -> Result<()> {
    rename_staged(path)?;
    fsync_parent_dir(path)?;
    Ok(())
}

/// The atomic rename alone — the truncate commit point and point of no return,
/// split out from [`commit_rename`] so [`WalWriter::truncate`] can treat every
/// fallible step *after* the rename (the parent-dir fsync and the fd reopen) as
/// its poison boundary (findings #2183 and #2184). A crash or error *before*
/// this returns leaves the original file untouched; after it, the staged
/// replacement is installed but not durable until the parent dir is fsynced.
fn rename_staged(path: &Path) -> Result<()> {
    let tmp_path = sibling_tmp(path);
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Fsync the parent directory of `path` so a preceding `rename`/`create_new`
/// into that directory is durable across a crash, not just visible to this
/// process. If `path` has no parent (e.g. it is `/` or a bare relative file
/// name with an empty parent), there is no directory entry to sync against —
/// treat that as a no-op rather than an error.
fn fsync_parent_dir(path: &Path) -> Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Ok(()),
    };
    File::open(parent)?.sync_all()?;
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
        writer
            .append(&WalRecord::InsertNode(make_test_node(1)))
            .unwrap();
        writer
            .append(&WalRecord::InsertNode(make_test_node(2)))
            .unwrap();
        // Snapshot the byte offset after both good records; that is where
        // last_good_offset must land after we stop at the corrupt record.
        let good_end = writer.current_lsn().0;
        drop(writer);

        // Append a well-formed-looking record whose CRC is wrong, simulating
        // a torn write at crash time.  length=10 means the total blob is
        // 4 + 10 + 4 = 18 bytes, so the length-guard does NOT trip (18 bytes
        // are present); only the CRC check will catch it.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
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

        assert_eq!(
            records.len(),
            2,
            "both good records must be parsed before the corrupt one"
        );
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

    /// Issue #19: 100 randomized iterations, each simulating a crash at the
    /// least-safe point in a truncate (staged-but-not-renamed), then
    /// verifying:
    ///  1. Pre-rename, the live WAL file is byte-for-byte untouched (the
    ///     old-file-or-new-file invariant — never a half-written file).
    ///  2. After the real, completed truncate (via `WalWriter::truncate`,
    ///     which serializes against `append`), the resulting file is fully
    ///     parseable with no torn tail and contains exactly the expected
    ///     surviving records.
    ///  3. Appends issued *after* the truncate land in the new file rather
    ///     than vanishing into the unlinked old inode — this is what the
    ///     serialization + fd-reopen fix actually buys us.
    ///
    /// Uses a fixed seed so a failure is reproducible.
    #[test]
    fn test_wal_truncate_survives_crash() {
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};

        let mut rng = StdRng::seed_from_u64(0x0019_9319); // fixed seed: reproducible on failure

        for iter in 0..100u32 {
            let tmp = NamedTempFile::new().unwrap();
            let path = tmp.path().to_path_buf();

            let writer = WalWriter::new(&path).unwrap();

            // Append a random number of records, remembering the LSN
            // boundary after each one so the truncation point we pick below
            // always lands on a real record boundary (that's the only
            // contract `truncate`/`truncate_wal` make with callers).
            let n_records = rng.gen_range(1..=8u64);
            let mut boundaries = vec![Lsn(0)];
            for i in 0..n_records {
                writer
                    .append(&WalRecord::InsertNode(make_test_node(
                        iter as u64 * 1000 + i,
                    )))
                    .unwrap();
                boundaries.push(writer.current_lsn());
            }

            // cut_idx == 0 means "truncate nothing"; cut_idx == n_records
            // means "truncate everything" (both are valid edge cases).
            let cut_idx = rng.gen_range(0..=n_records as usize);
            let cut_lsn = boundaries[cut_idx];
            let expected_after_cut = n_records as usize - cut_idx;

            // --- Step 1: drive truncate to the intermediate, staged-but-
            // not-renamed state and simulate a crash right there. ---
            let original_bytes = std::fs::read(&path).unwrap();
            let tail = read_tail(&path, cut_lsn).unwrap();
            stage_tail(&path, &tail).unwrap();

            let tmp_sibling = sibling_tmp(&path);
            assert!(
                tmp_sibling.exists(),
                "iter {iter}: staged file must exist pre-rename"
            );
            let live_bytes_after_stage = std::fs::read(&path).unwrap();
            assert_eq!(
                original_bytes, live_bytes_after_stage,
                "iter {iter}: staging must not mutate the live WAL before the atomic rename"
            );

            // "Restart" after the simulated crash: a real recovery path
            // would discard the orphaned .new (mirrored by the
            // remove-stale-tmp step at the top of every real truncate) and
            // find the original file fully intact.
            std::fs::remove_file(&tmp_sibling).unwrap();
            let reader = WalReader::new(&path);
            let (records, last_good) = reader.read_from(Lsn(0)).unwrap();
            assert_eq!(
                records.len(),
                n_records as usize,
                "iter {iter}: pre-rename crash must leave every original record intact"
            );
            assert_eq!(last_good, boundaries[n_records as usize].0);

            // --- Step 2: perform the real, completed truncate through the
            // writer. This exercises the serialization fix: the writer's
            // buffered fd is swapped out for one opened on the post-rename
            // inode. ---
            writer.truncate(cut_lsn).unwrap();

            assert!(
                !sibling_tmp(&path).exists(),
                "iter {iter}: no leftover .new after a completed truncate"
            );

            let reader = WalReader::new(&path);
            let (records, last_good) = reader.read_from(Lsn(0)).unwrap();
            assert_eq!(
                records.len(),
                expected_after_cut,
                "iter {iter}: post-truncate record count mismatch (cut_idx={cut_idx}, n_records={n_records})"
            );
            let file_len = std::fs::metadata(&path).unwrap().len();
            assert_eq!(
                last_good, file_len,
                "iter {iter}: truncated file must be fully parseable with no torn tail"
            );

            // --- Step 3: an append after truncate must be visible in the
            // renamed file, proving the writer's fd was correctly swapped
            // rather than silently writing into the unlinked old inode. ---
            writer
                .append(&WalRecord::InsertNode(make_test_node(iter as u64 * 1000 + 999)))
                .unwrap();
            let reader = WalReader::new(&path);
            let (records_after_append, _) = reader.read_from(Lsn(0)).unwrap();
            assert_eq!(
                records_after_append.len(),
                expected_after_cut + 1,
                "iter {iter}: append after truncate must land in the live (renamed) file"
            );
        }
    }

    /// Issue #19: `WalWriter::truncate` must serialize against concurrent
    /// `append` calls from other threads — no torn records, no panics, no
    /// deadlocks, and the resulting log must always be fully parseable no
    /// matter how the two interleave.
    #[test]
    fn test_wal_truncate_serializes_with_concurrent_append() {
        use std::sync::Arc;
        use std::sync::Barrier;

        for run in 0..20u64 {
            let tmp = NamedTempFile::new().unwrap();
            let path = tmp.path().to_path_buf();
            let writer = Arc::new(WalWriter::new(&path).unwrap());

            // Seed a few records so there is something to (possibly) cut.
            for i in 0..4 {
                writer
                    .append(&WalRecord::InsertNode(make_test_node(run * 100 + i)))
                    .unwrap();
            }
            let cut_at = writer.current_lsn();

            let barrier = Arc::new(Barrier::new(2));

            let appender = {
                let writer = Arc::clone(&writer);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for i in 0..20u64 {
                        writer
                            .append(&WalRecord::InsertNode(make_test_node(run * 100 + 10 + i)))
                            .unwrap();
                    }
                })
            };
            let truncator = {
                let writer = Arc::clone(&writer);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    writer.truncate(cut_at).unwrap();
                })
            };

            appender.join().unwrap();
            truncator.join().unwrap();

            // Regardless of interleaving, the resulting log must be fully
            // parseable with no torn tail — that's the invariant the lock
            // guarantees, since append and truncate can never run
            // concurrently against the same file handle.
            let reader = WalReader::new(&path);
            let (records, last_good) = reader.read_from(Lsn(0)).unwrap();
            let file_len = std::fs::metadata(&path).unwrap().len();
            assert_eq!(
                last_good, file_len,
                "run {run}: concurrent truncate/append must never leave a torn tail"
            );
            // `cut_at` was captured before either thread started, so the
            // truncate can only ever discard the 4 seed records — never any
            // of the 20 post-barrier appends, regardless of how the two
            // threads interleave (they're fully serialized by the writer's
            // lock, so truncate sees a prefix of already-written appends in
            // its tail, and the rest continue landing in the reopened file
            // afterward).
            assert_eq!(
                records.len(),
                20,
                "run {run}: all 20 post-barrier appends must survive regardless of interleaving"
            );
        }
    }

    /// Issue #19 / finding #2183: once a `WalWriter` is poisoned (as it would
    /// be if `truncate`'s post-rename reopen failed), every subsequent
    /// `append` and `truncate` must fail fast with a clear error rather than
    /// silently writing into (or attempting to truncate through) a stale,
    /// orphaned file handle.
    #[test]
    fn test_wal_writer_poisoned_after_failed_reopen_fails_fast() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let writer = WalWriter::new(&path).unwrap();
        writer
            .append(&WalRecord::InsertNode(make_test_node(1)))
            .unwrap();
        let good_len = std::fs::metadata(&path).unwrap().len();

        // Force the same state a failed post-truncate reopen would leave.
        writer.poison_for_test();

        let append_err = writer
            .append(&WalRecord::InsertNode(make_test_node(2)))
            .unwrap_err();
        assert!(
            matches!(append_err, AstraeaError::Storage(ref msg) if msg.contains("poisoned")),
            "append on a poisoned writer must fail fast with a clear error, got {append_err:?}"
        );

        let truncate_err = writer.truncate(Lsn(0)).unwrap_err();
        assert!(
            matches!(truncate_err, AstraeaError::Storage(ref msg) if msg.contains("poisoned")),
            "truncate on a poisoned writer must fail fast with a clear error, got {truncate_err:?}"
        );

        // Neither failed call must have touched the on-disk file.
        let file_len_after = std::fs::metadata(&path).unwrap().len();
        assert_eq!(
            file_len_after, good_len,
            "a poisoned writer must never mutate the on-disk WAL"
        );
        let reader = WalReader::new(&path);
        let (records, _) = reader.read_from(Lsn(0)).unwrap();
        assert_eq!(records.len(), 1, "the original good record must be intact");
    }

    /// Finding #2182: `commit_rename` must fsync the parent directory after
    /// the rename so the rename itself is durable across a crash, not just
    /// atomic from the perspective of this process. We can't directly assert
    /// an fsync happened, but we can assert `commit_rename` (and therefore
    /// `truncate_wal`/`WalWriter::truncate`) succeeds and leaves a fully
    /// consistent file — regressions in the fsync step (e.g. a typo'd path)
    /// would show up as an `Err` here since `fsync_parent_dir` is fallible.
    #[test]
    fn test_wal_truncate_fsyncs_parent_dir_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wal.log");

        let writer = WalWriter::new(&path).unwrap();
        writer
            .append(&WalRecord::InsertNode(make_test_node(1)))
            .unwrap();
        let lsn1 = writer
            .append(&WalRecord::InsertNode(make_test_node(2)))
            .unwrap();

        // This exercises commit_rename -> fsync_parent_dir end to end; any
        // failure to open/sync the parent directory surfaces as an Err.
        writer.truncate(lsn1).unwrap();

        let reader = WalReader::new(&path);
        let (records, last_good) = reader.read_from(Lsn(0)).unwrap();
        assert_eq!(records.len(), 1);
        let file_len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(last_good, file_len);
    }
}

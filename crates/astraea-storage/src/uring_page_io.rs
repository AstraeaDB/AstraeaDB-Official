//! io_uring-based async I/O backend for AstraeaDB (Linux-only).
//!
//! This module provides [`UringPageIO`], an implementation of the [`PageIO`] trait
//! that uses Linux's `io_uring` subsystem for high-performance asynchronous I/O.
//!
//! # Why io_uring?
//!
//! Traditional file I/O on Linux uses system calls like `read()` and `write()`,
//! which incur context switches between user space and kernel space. For a database
//! that performs millions of page reads/writes, this overhead is significant.
//!
//! `io_uring` (introduced in Linux 5.1) provides:
//! - **Zero-copy I/O**: Data can be transferred directly without extra copies.
//! - **Batched submissions**: Multiple I/O operations can be submitted in one syscall.
//! - **Polled completions**: Completions can be harvested without syscalls.
//! - **Registered buffers**: Pre-registered memory reduces per-I/O overhead.
//!
//! # Usage
//!
//! This module is only available on Linux with the `io-uring` feature enabled:
//!
//! ```toml
//! [dependencies]
//! astraea-storage = { version = "0.1", features = ["io-uring"] }
//! ```
//!
//! ```rust,ignore
//! use astraea_storage::uring_page_io::UringPageIO;
//! use astraea_storage::page_io::PageIO;
//!
//! let uring_io = UringPageIO::new("/path/to/db.dat")?;
//! let page_id = uring_io.allocate_page()?;
//! uring_io.write_page(page_id, &my_data)?;
//! let data = uring_io.read_page(page_id)?;
//! ```
//!
//! # Implementation Notes
//!
//! This implementation uses a single-entry io_uring ring for simplicity. Each
//! operation (read/write) submits one SQE and waits for the corresponding CQE.
//! This approach provides the foundation for io_uring integration while keeping
//! the implementation straightforward.
//!
//! Future enhancements could include:
//! - **Batched operations**: Submit multiple reads/writes in one syscall.
//! - **Registered file descriptors**: Reduce per-operation fd lookup overhead.
//! - **Registered buffers**: Pre-register page buffers with the kernel.
//! - **IOPOLL mode**: For NVMe drives, use polling instead of interrupts.

#![cfg(all(target_os = "linux", feature = "io-uring"))]

use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use io_uring_crate::{opcode, types, IoUring};
use parking_lot::Mutex;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::PageId;

use crate::page::PAGE_SIZE;
use crate::page_io::PageIO;

/// io_uring-based page I/O implementation.
///
/// This struct manages a database file and uses Linux's io_uring for
/// asynchronous I/O operations. It implements the [`PageIO`] trait,
/// allowing it to be used as a drop-in replacement for [`FileManager`].
///
/// # Thread Safety
///
/// `UringPageIO` is `Send + Sync`. The io_uring ring is protected by a mutex
/// to ensure safe concurrent access from multiple threads. For maximum
/// performance in highly concurrent workloads, consider using a ring per
/// thread or a work-stealing pool of rings.
///
/// # Example
///
/// ```rust,ignore
/// use astraea_storage::uring_page_io::UringPageIO;
/// use astraea_storage::page_io::PageIO;
///
/// let io = UringPageIO::new("/tmp/test.db")?;
/// let page_id = io.allocate_page()?;
///
/// let mut data = [0u8; 8192];
/// data[0] = 0xAB;
/// io.write_page(page_id, &data)?;
///
/// let read_back = io.read_page(page_id)?;
/// assert_eq!(read_back[0], 0xAB);
/// ```
pub struct UringPageIO {
    /// The underlying file handle.
    file: File,
    /// Path to the database file (kept for diagnostics).
    #[allow(dead_code)]
    path: PathBuf,
    /// Current file length in bytes, tracked atomically to avoid syscalls.
    file_len: AtomicU64,
    /// The io_uring instance, protected by a mutex for thread safety.
    ring: Mutex<IoUring>,
}

impl UringPageIO {
    /// Default number of SQ entries for the io_uring ring.
    ///
    /// A single entry is sufficient for synchronous-style operations.
    /// Increase this for batched I/O workloads.
    const RING_ENTRIES: u32 = 8;

    /// Create a new `UringPageIO` instance for the given file path.
    ///
    /// The file is created if it does not exist. The io_uring ring is
    /// initialized with a small number of entries suitable for single
    /// operation submissions.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened or created.
    /// - The io_uring ring cannot be initialized (e.g., kernel too old).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let io = UringPageIO::new("/var/lib/astraea/data.db")?;
    /// ```
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Open the file with O_DIRECT would be ideal for io_uring, but it
        // requires aligned buffers. For simplicity, we use buffered I/O.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        let file_len = file.metadata()?.len();

        // Initialize the io_uring ring.
        // Using a small ring size since we're doing synchronous-style operations.
        let ring = IoUring::new(Self::RING_ENTRIES).map_err(|e| {
            AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to initialize io_uring: {}", e),
            ))
        })?;

        Ok(Self {
            file,
            path,
            file_len: AtomicU64::new(file_len),
            ring: Mutex::new(ring),
        })
    }

    /// Read a page from disk using io_uring.
    ///
    /// Submits a read operation to the io_uring submission queue, then waits
    /// for the completion. The page data is read directly into the returned
    /// buffer.
    ///
    /// # Errors
    ///
    /// Returns [`AstraeaError::PageNotFound`] if the page doesn't exist.
    /// Returns [`AstraeaError::StorageIo`] on I/O errors.
    pub fn read_page(&self, page_id: PageId) -> Result<[u8; PAGE_SIZE]> {
        let offset = page_id.0 * PAGE_SIZE as u64;
        let file_len = self.file_len.load(Ordering::Acquire);

        // Bounds check: ensure the page exists within the file.
        if offset + PAGE_SIZE as u64 > file_len {
            return Err(AstraeaError::PageNotFound(page_id));
        }

        let mut buf = [0u8; PAGE_SIZE];
        let fd = types::Fd(self.file.as_raw_fd());

        let mut ring = self.ring.lock();

        // Build the read operation.
        // SAFETY: The buffer lives until we complete the operation.
        let read_op = opcode::Read::new(fd, buf.as_mut_ptr(), PAGE_SIZE as u32)
            .offset(offset)
            .build()
            .user_data(0x01); // Arbitrary user data for identification.

        // Submit the operation.
        // SAFETY: We're submitting a valid read operation.
        unsafe {
            ring.submission()
                .push(&read_op)
                .map_err(|_| {
                    AstraeaError::StorageIo(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "io_uring submission queue full",
                    ))
                })?;
        }

        ring.submit_and_wait(1).map_err(|e| {
            AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("io_uring submit failed: {}", e),
            ))
        })?;

        // Harvest the completion.
        let cqe = ring.completion().next().ok_or_else(|| {
            AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                "io_uring completion missing",
            ))
        })?;

        let result = cqe.result();
        if result < 0 {
            return Err(AstraeaError::StorageIo(std::io::Error::from_raw_os_error(
                -result,
            )));
        }

        if (result as usize) != PAGE_SIZE {
            return Err(AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "short read: expected {} bytes, got {}",
                    PAGE_SIZE, result
                ),
            )));
        }

        Ok(buf)
    }

    /// Write a page to disk using io_uring.
    ///
    /// Submits a write operation to the io_uring submission queue, then waits
    /// for completion. The write is followed by an fsync to ensure durability.
    ///
    /// # Errors
    ///
    /// Returns [`AstraeaError::StorageIo`] on I/O errors.
    pub fn write_page(&self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        let offset = page_id.0 * PAGE_SIZE as u64;
        let fd = types::Fd(self.file.as_raw_fd());

        let mut ring = self.ring.lock();

        // Build the write operation.
        // SAFETY: The data buffer is valid for the lifetime of this call.
        let write_op = opcode::Write::new(fd, data.as_ptr(), PAGE_SIZE as u32)
            .offset(offset)
            .build()
            .user_data(0x02);

        // Submit the write.
        // SAFETY: We're submitting a valid write operation.
        unsafe {
            ring.submission()
                .push(&write_op)
                .map_err(|_| {
                    AstraeaError::StorageIo(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "io_uring submission queue full",
                    ))
                })?;
        }

        ring.submit_and_wait(1).map_err(|e| {
            AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("io_uring submit failed: {}", e),
            ))
        })?;

        // Harvest the write completion.
        let cqe = ring.completion().next().ok_or_else(|| {
            AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                "io_uring completion missing",
            ))
        })?;

        let result = cqe.result();
        if result < 0 {
            return Err(AstraeaError::StorageIo(std::io::Error::from_raw_os_error(
                -result,
            )));
        }

        if (result as usize) != PAGE_SIZE {
            return Err(AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                format!(
                    "short write: expected {} bytes, wrote {}",
                    PAGE_SIZE, result
                ),
            )));
        }

        // Issue fsync via io_uring for durability.
        let fsync_op = opcode::Fsync::new(fd).build().user_data(0x03);

        // SAFETY: Valid fsync operation.
        unsafe {
            ring.submission()
                .push(&fsync_op)
                .map_err(|_| {
                    AstraeaError::StorageIo(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "io_uring submission queue full",
                    ))
                })?;
        }

        ring.submit_and_wait(1).map_err(|e| {
            AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("io_uring fsync submit failed: {}", e),
            ))
        })?;

        let cqe = ring.completion().next().ok_or_else(|| {
            AstraeaError::StorageIo(std::io::Error::new(
                std::io::ErrorKind::Other,
                "io_uring fsync completion missing",
            ))
        })?;

        let result = cqe.result();
        if result < 0 {
            return Err(AstraeaError::StorageIo(std::io::Error::from_raw_os_error(
                -result,
            )));
        }

        Ok(())
    }

    /// Allocate a new page by extending the file.
    ///
    /// Extends the file by one page (8 KiB) and returns the ID of the new page.
    /// The new page is zero-initialized.
    ///
    /// # Note
    ///
    /// File extension uses `ftruncate` via standard I/O since io_uring's
    /// `ftruncate` support varies by kernel version. The actual zeroing
    /// is implicit (sparse file) or done by the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`AstraeaError::StorageIo`] on I/O errors.
    pub fn allocate_page(&self) -> Result<PageId> {
        // We need to extend the file. Since ftruncate via io_uring requires
        // kernel 5.15+, we use standard ftruncate for portability.
        let current_len = self.file_len.load(Ordering::Acquire);
        let page_id = if current_len == 0 {
            0
        } else {
            current_len / PAGE_SIZE as u64
        };

        let new_len = current_len + PAGE_SIZE as u64;

        // Use nix or libc for ftruncate, or fall back to file.set_len().
        self.file.set_len(new_len)?;
        self.file_len.store(new_len, Ordering::Release);

        // Write zeros to the new page to ensure it's actually allocated
        // (not just a sparse hole). This is important for durability.
        let zeros = [0u8; PAGE_SIZE];
        self.write_page(PageId(page_id), &zeros)?;

        Ok(PageId(page_id))
    }

    /// Return the total number of pages currently in the file.
    pub fn page_count(&self) -> u64 {
        self.file_len.load(Ordering::Acquire) / PAGE_SIZE as u64
    }
}

// SAFETY: UringPageIO is Send + Sync because:
// - `File` is Send + Sync
// - `AtomicU64` is Send + Sync
// - `Mutex<IoUring>` is Send + Sync (IoUring is Send)
// - `PathBuf` is Send + Sync
unsafe impl Send for UringPageIO {}
unsafe impl Sync for UringPageIO {}

/// Implement the [`PageIO`] trait for `UringPageIO`.
///
/// This allows `UringPageIO` to be used as a drop-in replacement for
/// [`FileManager`] in the buffer pool and storage engine.
impl PageIO for UringPageIO {
    fn read_page(&self, page_id: PageId) -> Result<[u8; PAGE_SIZE]> {
        UringPageIO::read_page(self, page_id)
    }

    fn write_page(&self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        UringPageIO::write_page(self, page_id, data)
    }

    fn allocate_page(&self) -> Result<PageId> {
        UringPageIO::allocate_page(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    /// Basic read/write roundtrip test.
    #[test]
    fn test_read_write_roundtrip() {
        let tmp = NamedTempFile::new().unwrap();
        let io = UringPageIO::new(tmp.path()).unwrap();

        // Allocate a page.
        let pid = io.allocate_page().unwrap();
        assert_eq!(pid, PageId(0));
        assert_eq!(io.page_count(), 1);

        // Write identifiable data.
        let mut data = [0u8; PAGE_SIZE];
        data[0] = 0xDE;
        data[1] = 0xAD;
        data[PAGE_SIZE - 2] = 0xBE;
        data[PAGE_SIZE - 1] = 0xEF;
        io.write_page(pid, &data).unwrap();

        // Read it back and verify.
        let read_back = io.read_page(pid).unwrap();
        assert_eq!(read_back[0], 0xDE);
        assert_eq!(read_back[1], 0xAD);
        assert_eq!(read_back[PAGE_SIZE - 2], 0xBE);
        assert_eq!(read_back[PAGE_SIZE - 1], 0xEF);
    }

    /// Test allocating multiple pages.
    #[test]
    fn test_allocate_multiple_pages() {
        let tmp = NamedTempFile::new().unwrap();
        let io = UringPageIO::new(tmp.path()).unwrap();

        let p0 = io.allocate_page().unwrap();
        let p1 = io.allocate_page().unwrap();
        let p2 = io.allocate_page().unwrap();

        assert_eq!(p0, PageId(0));
        assert_eq!(p1, PageId(1));
        assert_eq!(p2, PageId(2));
        assert_eq!(io.page_count(), 3);

        // Write distinct markers to each page.
        for (pid, marker) in [(p0, 0x11u8), (p1, 0x22u8), (p2, 0x33u8)] {
            let mut data = [0u8; PAGE_SIZE];
            data[0] = marker;
            data[PAGE_SIZE - 1] = marker;
            io.write_page(pid, &data).unwrap();
        }

        // Verify each page has its distinct marker.
        assert_eq!(io.read_page(p0).unwrap()[0], 0x11);
        assert_eq!(io.read_page(p1).unwrap()[0], 0x22);
        assert_eq!(io.read_page(p2).unwrap()[0], 0x33);
        assert_eq!(io.read_page(p0).unwrap()[PAGE_SIZE - 1], 0x11);
        assert_eq!(io.read_page(p1).unwrap()[PAGE_SIZE - 1], 0x22);
        assert_eq!(io.read_page(p2).unwrap()[PAGE_SIZE - 1], 0x33);
    }

    /// Reading a non-existent page should fail.
    #[test]
    fn test_read_nonexistent_page() {
        let tmp = NamedTempFile::new().unwrap();
        let io = UringPageIO::new(tmp.path()).unwrap();

        let result = io.read_page(PageId(999));
        assert!(result.is_err());
    }

    /// Test using UringPageIO through the PageIO trait.
    #[test]
    fn test_page_io_trait() {
        let tmp = NamedTempFile::new().unwrap();
        let io = UringPageIO::new(tmp.path()).unwrap();

        // Use through the trait object interface.
        let page_io: &dyn PageIO = &io;

        let pid = page_io.allocate_page().unwrap();
        assert_eq!(pid, PageId(0));

        let mut data = [0u8; PAGE_SIZE];
        data[0] = 0xCA;
        data[1] = 0xFE;
        page_io.write_page(pid, &data).unwrap();

        let read_back = page_io.read_page(pid).unwrap();
        assert_eq!(read_back[0], 0xCA);
        assert_eq!(read_back[1], 0xFE);
    }
}

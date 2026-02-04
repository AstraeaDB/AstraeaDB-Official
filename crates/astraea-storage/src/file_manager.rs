//! Disk I/O for the page store.
//!
//! Provides low-level read/write access to a database file organized as
//! fixed-size pages. Uses standard file I/O with seek + read/write.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::PageId;
use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::page::PAGE_SIZE;
use crate::page_io::PageIO;

/// Manages a single database file organized as fixed-size pages.
pub struct FileManager {
    /// The underlying file handle, protected by a mutex for thread safety.
    file: Mutex<File>,
    /// Path to the database file (kept for diagnostics).
    #[allow(dead_code)]
    path: PathBuf,
}

impl FileManager {
    /// Open or create a database file at the given path.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        Ok(Self {
            file: Mutex::new(file),
            path,
        })
    }

    /// Read a page from disk by its page ID.
    ///
    /// The page is located at file offset `page_id * PAGE_SIZE`.
    pub fn read_page(&self, page_id: PageId) -> Result<[u8; PAGE_SIZE]> {
        let offset = page_id.0 * PAGE_SIZE as u64;
        let mut file = self.file.lock();

        // Check that the page is within file bounds.
        let file_len = file.seek(SeekFrom::End(0))?;
        if offset + PAGE_SIZE as u64 > file_len {
            return Err(AstraeaError::PageNotFound(page_id));
        }

        file.seek(SeekFrom::Start(offset))?;
        let mut buf = [0u8; PAGE_SIZE];
        file.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Write a page to disk at the position determined by its page ID.
    pub fn write_page(&self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        let offset = page_id.0 * PAGE_SIZE as u64;
        let mut file = self.file.lock();
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        file.flush()?;
        Ok(())
    }

    /// Allocate a new page by extending the file. Returns the new page's ID.
    pub fn allocate_page(&self) -> Result<PageId> {
        let mut file = self.file.lock();
        let file_len = file.seek(SeekFrom::End(0))?;

        // The new page ID is the current number of pages.
        let page_id = if file_len == 0 {
            0
        } else {
            file_len / PAGE_SIZE as u64
        };

        // Extend the file by one page of zeros.
        let zeros = [0u8; PAGE_SIZE];
        file.write_all(&zeros)?;
        file.flush()?;

        Ok(PageId(page_id))
    }

    /// Return the total number of pages currently in the file.
    pub fn page_count(&self) -> Result<u64> {
        let mut file = self.file.lock();
        let file_len = file.seek(SeekFrom::End(0))?;
        Ok(file_len / PAGE_SIZE as u64)
    }
}

/// Implement the [`PageIO`] trait for `FileManager`.
///
/// The existing inherent methods already match the trait signatures exactly,
/// so each trait method simply delegates to the corresponding inherent method.
impl PageIO for FileManager {
    fn read_page(&self, page_id: PageId) -> Result<[u8; PAGE_SIZE]> {
        FileManager::read_page(self, page_id)
    }

    fn write_page(&self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        FileManager::write_page(self, page_id, data)
    }

    fn allocate_page(&self) -> Result<PageId> {
        FileManager::allocate_page(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_allocate_and_read_write() {
        let tmp = NamedTempFile::new().unwrap();
        let fm = FileManager::new(tmp.path()).unwrap();

        // Allocate a page.
        let pid = fm.allocate_page().unwrap();
        assert_eq!(pid, PageId(0));
        assert_eq!(fm.page_count().unwrap(), 1);

        // Write some data.
        let mut data = [0u8; PAGE_SIZE];
        data[0] = 0xAB;
        data[PAGE_SIZE - 1] = 0xCD;
        fm.write_page(pid, &data).unwrap();

        // Read it back.
        let read_back = fm.read_page(pid).unwrap();
        assert_eq!(read_back[0], 0xAB);
        assert_eq!(read_back[PAGE_SIZE - 1], 0xCD);
    }

    #[test]
    fn test_read_nonexistent_page() {
        let tmp = NamedTempFile::new().unwrap();
        let fm = FileManager::new(tmp.path()).unwrap();

        // Reading a page that doesn't exist should fail.
        let result = fm.read_page(PageId(999));
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_pages() {
        let tmp = NamedTempFile::new().unwrap();
        let fm = FileManager::new(tmp.path()).unwrap();

        let p0 = fm.allocate_page().unwrap();
        let p1 = fm.allocate_page().unwrap();
        let p2 = fm.allocate_page().unwrap();
        assert_eq!(p0, PageId(0));
        assert_eq!(p1, PageId(1));
        assert_eq!(p2, PageId(2));
        assert_eq!(fm.page_count().unwrap(), 3);

        // Write distinct data to each page and verify.
        for (pid, marker) in [(p0, 0x11u8), (p1, 0x22u8), (p2, 0x33u8)] {
            let mut data = [0u8; PAGE_SIZE];
            data[0] = marker;
            fm.write_page(pid, &data).unwrap();
        }
        assert_eq!(fm.read_page(p0).unwrap()[0], 0x11);
        assert_eq!(fm.read_page(p1).unwrap()[0], 0x22);
        assert_eq!(fm.read_page(p2).unwrap()[0], 0x33);
    }
}

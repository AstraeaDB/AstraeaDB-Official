//! Abstract trait for page-level I/O operations.
//!
//! This module defines the [`PageIO`] trait, which decouples the buffer pool
//! and storage engine from a specific I/O implementation.
//!
//! # Available Implementations
//!
//! - [`FileManager`](crate::file_manager::FileManager): The default, cross-platform
//!   backend using standard file I/O (seek + read/write).
//!
//! - [`UringPageIO`](crate::uring_page_io::UringPageIO): Linux-only, feature-gated
//!   backend using `io_uring` for high-performance asynchronous I/O. Enable with
//!   the `io-uring` feature flag.

use astraea_core::error::Result;
use astraea_core::types::PageId;

use crate::page::PAGE_SIZE;

/// Abstract trait for page-level I/O operations.
///
/// This trait allows swapping the underlying I/O implementation:
/// - [`FileManager`](crate::file_manager::FileManager) uses standard file I/O
///   (seek + read/write) — the default, cross-platform backend.
/// - [`UringPageIO`](crate::uring_page_io::UringPageIO) uses `io_uring` for
///   high-performance async Linux I/O (feature-gated with `io-uring`, Linux-only).
///
/// All implementations must be `Send + Sync` so they can be shared across
/// threads via `Arc<dyn PageIO>`.
pub trait PageIO: Send + Sync {
    /// Read a page by its ID, returning the raw page data.
    ///
    /// Returns an error if the page does not exist on disk (i.e., the file
    /// is not large enough to contain the requested page).
    fn read_page(&self, page_id: PageId) -> Result<[u8; PAGE_SIZE]>;

    /// Write raw page data to the given page ID.
    ///
    /// The page must have been previously allocated (or the implementation
    /// must handle extending the backing store).
    fn write_page(&self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()>;

    /// Allocate a new page, returning its ID.
    ///
    /// The backing store is extended by one page (zeroed out). The returned
    /// `PageId` corresponds to the newly allocated page.
    fn allocate_page(&self) -> Result<PageId>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_manager::FileManager;
    use tempfile::NamedTempFile;

    /// Verify that FileManager correctly implements the PageIO trait by
    /// exercising allocate, write, and read through the trait object.
    #[test]
    fn test_page_io_trait() {
        let tmp = NamedTempFile::new().unwrap();
        let fm = FileManager::new(tmp.path()).unwrap();

        // Use the FileManager through the trait object interface.
        let page_io: &dyn PageIO = &fm;

        // Allocate a page.
        let pid = page_io.allocate_page().unwrap();
        assert_eq!(pid, PageId(0));

        // Write identifiable data.
        let mut data = [0u8; PAGE_SIZE];
        data[0] = 0xDE;
        data[1] = 0xAD;
        data[PAGE_SIZE - 1] = 0xFF;
        page_io.write_page(pid, &data).unwrap();

        // Read it back and verify.
        let read_back = page_io.read_page(pid).unwrap();
        assert_eq!(read_back[0], 0xDE);
        assert_eq!(read_back[1], 0xAD);
        assert_eq!(read_back[PAGE_SIZE - 1], 0xFF);

        // Allocate a second page and verify distinct IDs.
        let pid2 = page_io.allocate_page().unwrap();
        assert_eq!(pid2, PageId(1));

        // Write different data to the second page.
        let mut data2 = [0u8; PAGE_SIZE];
        data2[0] = 0xBE;
        data2[1] = 0xEF;
        page_io.write_page(pid2, &data2).unwrap();

        // Verify both pages are independent.
        let r1 = page_io.read_page(pid).unwrap();
        let r2 = page_io.read_page(pid2).unwrap();
        assert_eq!(r1[0], 0xDE);
        assert_eq!(r2[0], 0xBE);
    }

    /// Verify that reading a non-existent page through the trait returns an error.
    #[test]
    fn test_page_io_read_nonexistent() {
        let tmp = NamedTempFile::new().unwrap();
        let fm = FileManager::new(tmp.path()).unwrap();
        let page_io: &dyn PageIO = &fm;

        let result = page_io.read_page(PageId(999));
        assert!(result.is_err());
    }
}

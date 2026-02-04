//! Buffer pool manager — LRU cache of pages.
//!
//! The buffer pool sits between the storage engine and the file manager,
//! caching recently accessed pages in memory to avoid redundant disk I/O.
//! It uses a simple LRU eviction policy for unpinned pages.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::PageId;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::file_manager::FileManager;
use crate::page::PAGE_SIZE;

/// Index into the frame table.
pub type FrameId = usize;

/// A single frame in the buffer pool.
struct Frame {
    /// The page currently loaded into this frame (None if the frame is free).
    page_id: Option<PageId>,
    /// Whether the page data has been modified since it was loaded.
    dirty: bool,
    /// Number of active references to this frame. A page cannot be evicted while pinned.
    pin_count: u32,
    /// The raw page data.
    data: Box<[u8; PAGE_SIZE]>,
}

impl Frame {
    fn new() -> Self {
        Self {
            page_id: None,
            dirty: false,
            pin_count: 0,
            data: Box::new([0u8; PAGE_SIZE]),
        }
    }
}

/// A guard that provides read access to a pinned page's data.
/// When dropped, the page remains pinned — the caller must explicitly unpin.
pub struct PageGuard {
    frame_id: FrameId,
    page_id: PageId,
    pool: Arc<BufferPoolInner>,
}

impl PageGuard {
    /// Get a shared reference to the page data.
    pub fn data(&self) -> PageData {
        let inner = self.pool.frames.read();
        let frame = &inner[self.frame_id];
        let mut buf = [0u8; PAGE_SIZE];
        buf.copy_from_slice(frame.data.as_ref());
        PageData(buf)
    }

    /// Get the page ID of this guard.
    pub fn page_id(&self) -> PageId {
        self.page_id
    }

    /// Write data into the page through this guard, marking it dirty.
    pub fn write_data(&self, data: &[u8; PAGE_SIZE]) {
        let mut inner = self.pool.frames.write();
        let frame = &mut inner[self.frame_id];
        frame.data.copy_from_slice(data);
        frame.dirty = true;
    }
}

/// Owned copy of page data, returned from PageGuard::data().
pub struct PageData(pub [u8; PAGE_SIZE]);

impl std::ops::Deref for PageData {
    type Target = [u8; PAGE_SIZE];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Internal state of the buffer pool, behind a RwLock.
struct BufferPoolInner {
    frames: RwLock<Vec<Frame>>,
    /// Maps page_id -> frame_id for currently loaded pages.
    page_table: RwLock<HashMap<PageId, FrameId>>,
    /// LRU list of unpinned frame IDs (front = least recently used).
    lru: RwLock<VecDeque<FrameId>>,
    /// Maximum number of frames (capacity).
    #[allow(dead_code)]
    capacity: usize,
}

/// The buffer pool manager.
pub struct BufferPool {
    inner: Arc<BufferPoolInner>,
    file_manager: Arc<FileManager>,
}

impl BufferPool {
    /// Create a new buffer pool with the given capacity (number of page frames).
    pub fn new(file_manager: Arc<FileManager>, capacity: usize) -> Self {
        let mut frames = Vec::with_capacity(capacity);
        let mut lru = VecDeque::with_capacity(capacity);
        for i in 0..capacity {
            frames.push(Frame::new());
            lru.push_back(i);
        }

        let inner = Arc::new(BufferPoolInner {
            frames: RwLock::new(frames),
            page_table: RwLock::new(HashMap::new()),
            lru: RwLock::new(lru),
            capacity,
        });

        Self {
            inner,
            file_manager,
        }
    }

    /// Pin a page, loading it from disk if not already cached.
    /// Returns a guard for reading/writing the page data.
    pub fn pin_page(&self, page_id: PageId) -> Result<PageGuard> {
        // Check if already in the pool.
        {
            let page_table = self.inner.page_table.read();
            if let Some(&frame_id) = page_table.get(&page_id) {
                let mut frames = self.inner.frames.write();
                frames[frame_id].pin_count += 1;
                // Remove from LRU if present (it's now pinned).
                let mut lru = self.inner.lru.write();
                lru.retain(|&fid| fid != frame_id);
                return Ok(PageGuard {
                    frame_id,
                    page_id,
                    pool: Arc::clone(&self.inner),
                });
            }
        }

        // Not in pool — need to find a free frame or evict.
        let frame_id = self.find_or_evict_frame()?;

        // Load the page from disk.
        let page_data = self.file_manager.read_page(page_id)?;

        // Install in the frame.
        {
            let mut frames = self.inner.frames.write();
            let frame = &mut frames[frame_id];
            frame.page_id = Some(page_id);
            frame.dirty = false;
            frame.pin_count = 1;
            frame.data.copy_from_slice(&page_data);
        }
        {
            let mut page_table = self.inner.page_table.write();
            page_table.insert(page_id, frame_id);
        }

        Ok(PageGuard {
            frame_id,
            page_id,
            pool: Arc::clone(&self.inner),
        })
    }

    /// Pin a new page (allocate on disk and bring into the pool).
    pub fn pin_new_page(&self, page_data: &[u8; PAGE_SIZE]) -> Result<PageGuard> {
        let page_id = self.file_manager.allocate_page()?;

        // Write the initial data to disk.
        self.file_manager.write_page(page_id, page_data)?;

        let frame_id = self.find_or_evict_frame()?;

        {
            let mut frames = self.inner.frames.write();
            let frame = &mut frames[frame_id];
            frame.page_id = Some(page_id);
            frame.dirty = false;
            frame.pin_count = 1;
            frame.data.copy_from_slice(page_data);
        }
        {
            let mut page_table = self.inner.page_table.write();
            page_table.insert(page_id, frame_id);
        }

        Ok(PageGuard {
            frame_id,
            page_id,
            pool: Arc::clone(&self.inner),
        })
    }

    /// Unpin a page. If dirty is true, mark the page as modified.
    pub fn unpin_page(&self, page_id: PageId, dirty: bool) -> Result<()> {
        let page_table = self.inner.page_table.read();
        let frame_id = match page_table.get(&page_id) {
            Some(&fid) => fid,
            None => return Ok(()), // Page not in pool, nothing to do.
        };
        drop(page_table);

        let mut frames = self.inner.frames.write();
        let frame = &mut frames[frame_id];
        if dirty {
            frame.dirty = true;
        }
        if frame.pin_count > 0 {
            frame.pin_count -= 1;
        }
        if frame.pin_count == 0 {
            // Add back to LRU (most recently used at the back).
            let mut lru = self.inner.lru.write();
            // Avoid duplicates.
            if !lru.contains(&frame_id) {
                lru.push_back(frame_id);
            }
        }
        Ok(())
    }

    /// Flush a specific page to disk if it is dirty.
    pub fn flush_page(&self, page_id: PageId) -> Result<()> {
        let page_table = self.inner.page_table.read();
        let frame_id = match page_table.get(&page_id) {
            Some(&fid) => fid,
            None => return Ok(()),
        };
        drop(page_table);

        let mut frames = self.inner.frames.write();
        let frame = &mut frames[frame_id];
        if frame.dirty {
            let mut data = [0u8; PAGE_SIZE];
            data.copy_from_slice(frame.data.as_ref());
            // Must release the lock before doing I/O, but we need the data.
            // We already copied it above.
            let pid = frame.page_id.unwrap();
            frame.dirty = false;
            drop(frames);
            self.file_manager.write_page(pid, &data)?;
        }
        Ok(())
    }

    /// Flush all dirty pages to disk.
    pub fn flush_all(&self) -> Result<()> {
        // Collect pages to flush while holding the lock briefly.
        let to_flush: Vec<(PageId, [u8; PAGE_SIZE])>;
        {
            let mut frames = self.inner.frames.write();
            to_flush = frames
                .iter_mut()
                .filter(|f| f.dirty && f.page_id.is_some())
                .map(|f| {
                    let pid = f.page_id.unwrap();
                    let mut data = [0u8; PAGE_SIZE];
                    data.copy_from_slice(f.data.as_ref());
                    f.dirty = false;
                    (pid, data)
                })
                .collect();
        }

        for (pid, data) in to_flush {
            self.file_manager.write_page(pid, &data)?;
        }
        Ok(())
    }

    /// Find a free frame or evict the least recently used unpinned frame.
    fn find_or_evict_frame(&self) -> Result<FrameId> {
        let mut lru = self.inner.lru.write();

        // Try to find an unused frame first (one with no page loaded).
        {
            let frames = self.inner.frames.read();
            for (i, fid) in lru.iter().enumerate() {
                if frames[*fid].page_id.is_none() {
                    let frame_id = lru.remove(i).unwrap();
                    return Ok(frame_id);
                }
            }
        }

        // All frames have pages — evict LRU unpinned frame.
        if let Some(frame_id) = lru.pop_front() {
            // Flush if dirty before evicting.
            let (need_flush, old_page_id, data_copy);
            {
                let frames = self.inner.frames.read();
                let frame = &frames[frame_id];
                need_flush = frame.dirty;
                old_page_id = frame.page_id;
                if need_flush {
                    let mut buf = [0u8; PAGE_SIZE];
                    buf.copy_from_slice(frame.data.as_ref());
                    data_copy = Some(buf);
                } else {
                    data_copy = None;
                }
            }

            if need_flush {
                if let (Some(pid), Some(data)) = (old_page_id, data_copy) {
                    drop(lru);
                    self.file_manager.write_page(pid, &data)?;
                    // Re-acquire lru — we don't need it below but we dropped it.
                }
            } else {
                drop(lru);
            }

            // Remove old page from page table.
            if let Some(old_pid) = old_page_id {
                let mut page_table = self.inner.page_table.write();
                page_table.remove(&old_pid);
            }

            // Clear the frame.
            {
                let mut frames = self.inner.frames.write();
                let frame = &mut frames[frame_id];
                frame.page_id = None;
                frame.dirty = false;
                frame.pin_count = 0;
                frame.data.fill(0);
            }

            return Ok(frame_id);
        }

        // No free or unpinned frames available.
        Err(AstraeaError::BufferPoolFull(PageId(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{init_page, PageType};
    use tempfile::NamedTempFile;

    fn make_pool(capacity: usize) -> (BufferPool, Arc<FileManager>) {
        let tmp = NamedTempFile::new().unwrap();
        let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
        let pool = BufferPool::new(Arc::clone(&fm), capacity);
        // Leak the tempfile so it isn't deleted while we still use it.
        let _ = tmp.into_temp_path();
        (pool, fm)
    }

    #[test]
    fn test_pin_new_page_and_read() {
        let (pool, _fm) = make_pool(4);

        let page_buf = init_page(PageId(0), PageType::NodePage);
        let guard = pool.pin_new_page(&page_buf).unwrap();
        let page_id = guard.page_id();

        let data = guard.data();
        assert_eq!(data[0..8], PageId(0).0.to_le_bytes());

        pool.unpin_page(page_id, false).unwrap();
    }

    #[test]
    fn test_dirty_page_flush() {
        let (pool, fm) = make_pool(4);

        // Create a page.
        let page_buf = init_page(PageId(0), PageType::NodePage);
        let guard = pool.pin_new_page(&page_buf).unwrap();
        let page_id = guard.page_id();

        // Write modified data.
        let mut modified = page_buf;
        modified[100] = 0xFF;
        guard.write_data(&modified);

        pool.unpin_page(page_id, true).unwrap();
        pool.flush_all().unwrap();

        // Verify on disk.
        let disk_data = fm.read_page(page_id).unwrap();
        assert_eq!(disk_data[100], 0xFF);
    }

    #[test]
    fn test_eviction() {
        // Pool of size 2, load 3 pages -> forces eviction.
        let (pool, fm) = make_pool(2);

        // Allocate 3 pages on disk.
        let p0 = fm.allocate_page().unwrap();
        let p1 = fm.allocate_page().unwrap();
        let p2 = fm.allocate_page().unwrap();

        // Write identifiable data.
        for (pid, marker) in [(&p0, 0xAAu8), (&p1, 0xBBu8), (&p2, 0xCCu8)] {
            let mut buf = [0u8; PAGE_SIZE];
            buf[0] = marker;
            fm.write_page(*pid, &buf).unwrap();
        }

        // Pin p0 and p1.
        let g0 = pool.pin_page(p0).unwrap();
        assert_eq!(g0.data()[0], 0xAA);
        pool.unpin_page(p0, false).unwrap();

        let g1 = pool.pin_page(p1).unwrap();
        assert_eq!(g1.data()[0], 0xBB);
        pool.unpin_page(p1, false).unwrap();

        // Pin p2 — should evict p0 (LRU).
        let g2 = pool.pin_page(p2).unwrap();
        assert_eq!(g2.data()[0], 0xCC);
        pool.unpin_page(p2, false).unwrap();

        // p0 should have been evicted, but we can still pin it again (reload from disk).
        let g0_again = pool.pin_page(p0).unwrap();
        assert_eq!(g0_again.data()[0], 0xAA);
        pool.unpin_page(p0, false).unwrap();
    }
}

//! Buffer pool manager — LRU cache of pages with pointer swizzling.
//!
//! The buffer pool sits between the storage engine and the file manager,
//! caching recently accessed pages in memory to avoid redundant disk I/O.
//! It uses a simple LRU eviction policy for unpinned pages.
//!
//! ## Pointer Swizzling
//!
//! Hot pages that are accessed frequently (more than `swizzle_threshold` times)
//! are automatically promoted to the "swizzled" hot set. Swizzled pages are
//! permanently pinned in memory and never evicted, eliminating disk I/O and
//! eviction overhead for the hottest subgraphs. This is the foundation of
//! AstraeaDB's Tier 3 (Hot) storage: active subgraphs stay in RAM with
//! nanosecond-level access latency.
//!
//! A page can be explicitly unswizzled via [`BufferPool::unswizzle`] to allow
//! it to be evicted again when it is no longer part of the active working set.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::PageId;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::page::PAGE_SIZE;
use crate::page_io::PageIO;

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
    /// Number of times this page has been pinned. Used to decide when to promote
    /// the page into the swizzled hot set.
    access_count: u64,
    /// Whether this frame has been promoted to the hot set (pointer-swizzled).
    /// Swizzled frames are never evicted from the buffer pool.
    swizzled: bool,
}

impl Frame {
    fn new() -> Self {
        Self {
            page_id: None,
            dirty: false,
            pin_count: 0,
            data: Box::new([0u8; PAGE_SIZE]),
            access_count: 0,
            swizzled: false,
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
    /// Access count threshold before a page is promoted to the swizzled hot set.
    /// Once a page's cumulative pin count exceeds this value, it becomes
    /// permanently resident in memory (until explicitly unswizzled).
    swizzle_threshold: u64,
    /// The set of page IDs currently in the swizzled hot set.
    hot_pages: RwLock<HashSet<PageId>>,
}

/// The buffer pool manager.
pub struct BufferPool {
    inner: Arc<BufferPoolInner>,
    page_io: Arc<dyn PageIO>,
}

impl BufferPool {
    /// Default swizzle threshold: a page must be pinned this many times before
    /// it is promoted to the hot set.
    const DEFAULT_SWIZZLE_THRESHOLD: u64 = 16;

    /// Create a new buffer pool with the given capacity (number of page frames).
    ///
    /// The `page_io` parameter accepts any `Arc<dyn PageIO>` implementation,
    /// allowing the buffer pool to work with different I/O backends (e.g.,
    /// `FileManager` for standard file I/O, or a future `io_uring` backend).
    pub fn new(page_io: Arc<dyn PageIO>, capacity: usize) -> Self {
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
            swizzle_threshold: Self::DEFAULT_SWIZZLE_THRESHOLD,
            hot_pages: RwLock::new(HashSet::new()),
        });

        Self {
            inner,
            page_io,
        }
    }

    /// Pin a page, loading it from disk if not already cached.
    /// Returns a guard for reading/writing the page data.
    ///
    /// Each call increments the page's access counter. When the counter exceeds
    /// the swizzle threshold, the page is promoted to the hot set and will not
    /// be evicted until explicitly unswizzled.
    pub fn pin_page(&self, page_id: PageId) -> Result<PageGuard> {
        // Check if already in the pool.
        {
            let page_table = self.inner.page_table.read();
            if let Some(&frame_id) = page_table.get(&page_id) {
                let mut frames = self.inner.frames.write();
                frames[frame_id].pin_count += 1;
                frames[frame_id].access_count += 1;

                // Check if we should promote to swizzled hot set.
                if !frames[frame_id].swizzled
                    && frames[frame_id].access_count > self.inner.swizzle_threshold
                {
                    frames[frame_id].swizzled = true;
                    let mut hot_pages = self.inner.hot_pages.write();
                    hot_pages.insert(page_id);
                }

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
        let page_data = self.page_io.read_page(page_id)?;

        // Install in the frame.
        {
            let mut frames = self.inner.frames.write();
            let frame = &mut frames[frame_id];
            frame.page_id = Some(page_id);
            frame.dirty = false;
            frame.pin_count = 1;
            frame.data.copy_from_slice(&page_data);
            frame.access_count = 1;
            frame.swizzled = false;
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
        let page_id = self.page_io.allocate_page()?;

        // Write the initial data to disk.
        self.page_io.write_page(page_id, page_data)?;

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
    ///
    /// Swizzled pages are never added back to the LRU on unpin, ensuring they
    /// remain permanently resident and cannot be evicted.
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
        if frame.pin_count == 0 && !frame.swizzled {
            // Add back to LRU (most recently used at the back).
            // Swizzled frames are never returned to the LRU — they stay
            // permanently cached.
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
            self.page_io.write_page(pid, &data)?;
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
            self.page_io.write_page(pid, &data)?;
        }
        Ok(())
    }

    /// Pin a page with access tracking, identical to [`pin_page`] in behaviour.
    ///
    /// This is the primary entry point for the pointer-swizzling path.
    /// Internally it delegates to `pin_page`, which already tracks access
    /// counts and promotes pages to the hot set.
    pub fn pin_page_ref(&self, page_id: PageId) -> Result<PageGuard> {
        self.pin_page(page_id)
    }

    /// Check whether a page is currently in the swizzled hot set.
    ///
    /// Swizzled pages are permanently cached in memory and never evicted,
    /// providing nanosecond-level access latency for hot subgraphs.
    pub fn is_swizzled(&self, page_id: PageId) -> bool {
        let hot_pages = self.inner.hot_pages.read();
        hot_pages.contains(&page_id)
    }

    /// Remove a page from the swizzled hot set, allowing it to be evicted
    /// again under LRU pressure.
    ///
    /// Resets the page's access counter and — if the page is currently
    /// unpinned — adds it back to the LRU queue.
    pub fn unswizzle(&self, page_id: PageId) -> Result<()> {
        // Remove from hot set.
        {
            let mut hot_pages = self.inner.hot_pages.write();
            hot_pages.remove(&page_id);
        }

        // Find the frame and clear the swizzled flag.
        let page_table = self.inner.page_table.read();
        let frame_id = match page_table.get(&page_id) {
            Some(&fid) => fid,
            None => return Ok(()), // Page not in pool, nothing to do.
        };
        drop(page_table);

        let mut frames = self.inner.frames.write();
        let frame = &mut frames[frame_id];
        frame.swizzled = false;
        frame.access_count = 0;

        // If the frame is currently unpinned, it needs to go back into the
        // LRU so that it can be evicted normally.
        if frame.pin_count == 0 {
            let mut lru = self.inner.lru.write();
            if !lru.contains(&frame_id) {
                lru.push_back(frame_id);
            }
        }

        Ok(())
    }

    /// Return the number of pages currently in the swizzled hot set.
    pub fn hot_page_count(&self) -> usize {
        let hot_pages = self.inner.hot_pages.read();
        hot_pages.len()
    }

    /// Find a free frame or evict the least recently used unpinned frame.
    ///
    /// Swizzled frames are never evicted. They should not appear in the LRU,
    /// but a safety check skips them if they are found there.
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
        // Skip any swizzled frames (safety check — they should not be in the
        // LRU, but we guard against it).
        let evict_idx = {
            let frames = self.inner.frames.read();
            let mut found = None;
            for (i, fid) in lru.iter().enumerate() {
                if !frames[*fid].swizzled {
                    found = Some(i);
                    break;
                }
            }
            found
        };

        if let Some(idx) = evict_idx {
            let frame_id = lru.remove(idx).unwrap();

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
                    self.page_io.write_page(pid, &data)?;
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
                frame.access_count = 0;
                frame.swizzled = false;
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
    use crate::file_manager::FileManager;
    use crate::page::{init_page, PageType};
    use tempfile::NamedTempFile;

    fn make_pool(capacity: usize) -> (BufferPool, Arc<FileManager>) {
        let tmp = NamedTempFile::new().unwrap();
        let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
        let pool = BufferPool::new(Arc::clone(&fm) as Arc<dyn PageIO>, capacity);
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

    // ---- Pointer Swizzling Tests ----

    /// Helper: create a pool with a custom swizzle threshold.
    fn make_pool_with_threshold(
        capacity: usize,
        threshold: u64,
    ) -> (BufferPool, Arc<FileManager>) {
        let tmp = NamedTempFile::new().unwrap();
        let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
        let mut pool = BufferPool::new(Arc::clone(&fm) as Arc<dyn PageIO>, capacity);
        // Override the default threshold via the inner Arc. We can do this
        // because we just created the pool and hold the only Arc reference
        // besides pool.inner.
        Arc::get_mut(&mut pool.inner).unwrap().swizzle_threshold = threshold;
        let _ = tmp.into_temp_path();
        (pool, fm)
    }

    #[test]
    fn test_access_counting() {
        // Use a high threshold so the page does not get swizzled during this test.
        let (pool, fm) = make_pool_with_threshold(4, 1000);

        let p0 = fm.allocate_page().unwrap();
        let mut buf = [0u8; PAGE_SIZE];
        buf[0] = 0xAA;
        fm.write_page(p0, &buf).unwrap();

        // Pin the page multiple times, unpinning between each.
        for expected in 1..=5u64 {
            let _guard = pool.pin_page(p0).unwrap();
            // Verify the access count.
            let frames = pool.inner.frames.read();
            let page_table = pool.inner.page_table.read();
            let frame_id = page_table[&p0];
            assert_eq!(
                frames[frame_id].access_count, expected,
                "access_count should be {} after {} pins",
                expected, expected
            );
            drop(page_table);
            drop(frames);
            pool.unpin_page(p0, false).unwrap();
        }
    }

    #[test]
    fn test_swizzle_promotion() {
        // Set threshold to 3 so the page is promoted after 4 pins.
        let (pool, fm) = make_pool_with_threshold(4, 3);

        let p0 = fm.allocate_page().unwrap();
        let mut buf = [0u8; PAGE_SIZE];
        buf[0] = 0xAA;
        fm.write_page(p0, &buf).unwrap();

        // Pin 3 times — should NOT be swizzled yet (access_count == threshold,
        // promotion requires > threshold).
        for _ in 0..3 {
            let _guard = pool.pin_page(p0).unwrap();
            pool.unpin_page(p0, false).unwrap();
        }
        assert!(
            !pool.is_swizzled(p0),
            "page should not be swizzled at threshold"
        );

        // One more pin pushes access_count to 4, which exceeds threshold of 3.
        let _guard = pool.pin_page(p0).unwrap();
        assert!(
            pool.is_swizzled(p0),
            "page should be swizzled after exceeding threshold"
        );
        pool.unpin_page(p0, false).unwrap();

        // Verify the frame's swizzled flag.
        {
            let frames = pool.inner.frames.read();
            let page_table = pool.inner.page_table.read();
            let frame_id = page_table[&p0];
            assert!(frames[frame_id].swizzled);
        }
    }

    #[test]
    fn test_swizzled_not_evicted() {
        // Pool of size 2. Swizzle one page, then load 3 more — the swizzled
        // page must survive all evictions.
        let (pool, fm) = make_pool_with_threshold(2, 1);

        // Allocate 4 pages on disk with identifiable data.
        let mut pages = Vec::new();
        for marker in [0xAAu8, 0xBBu8, 0xCCu8, 0xDDu8] {
            let pid = fm.allocate_page().unwrap();
            let mut buf = [0u8; PAGE_SIZE];
            buf[0] = marker;
            fm.write_page(pid, &buf).unwrap();
            pages.push(pid);
        }

        let p0 = pages[0];
        let p1 = pages[1];
        let p2 = pages[2];
        let p3 = pages[3];

        // Pin p0 twice (threshold=1, so >1 => swizzled on 2nd pin).
        let _g0 = pool.pin_page(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        let _g0 = pool.pin_page(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        assert!(pool.is_swizzled(p0), "p0 should now be swizzled");

        // p0 is swizzled and unpinned — it should NOT be in the LRU.
        {
            let lru = pool.inner.lru.read();
            let page_table = pool.inner.page_table.read();
            let frame_id_p0 = page_table[&p0];
            assert!(
                !lru.contains(&frame_id_p0),
                "swizzled frame should not be in LRU"
            );
        }

        // Now load p1 — uses the remaining free frame.
        let _g1 = pool.pin_page(p1).unwrap();
        assert_eq!(_g1.data()[0], 0xBB);
        pool.unpin_page(p1, false).unwrap();

        // Load p2 — must evict p1 (the only non-swizzled frame in LRU), NOT p0.
        let _g2 = pool.pin_page(p2).unwrap();
        assert_eq!(_g2.data()[0], 0xCC);
        pool.unpin_page(p2, false).unwrap();

        // Load p3 — must evict p2, NOT p0.
        let _g3 = pool.pin_page(p3).unwrap();
        assert_eq!(_g3.data()[0], 0xDD);
        pool.unpin_page(p3, false).unwrap();

        // p0 should STILL be in the pool with its original data.
        let g0_final = pool.pin_page(p0).unwrap();
        assert_eq!(g0_final.data()[0], 0xAA, "swizzled page p0 must not be evicted");
        pool.unpin_page(p0, false).unwrap();
    }

    #[test]
    fn test_unswizzle() {
        // Pool of size 2. Swizzle a page, then unswizzle it and verify it can
        // be evicted.
        let (pool, fm) = make_pool_with_threshold(2, 1);

        let mut pages = Vec::new();
        for marker in [0xAAu8, 0xBBu8, 0xCCu8] {
            let pid = fm.allocate_page().unwrap();
            let mut buf = [0u8; PAGE_SIZE];
            buf[0] = marker;
            fm.write_page(pid, &buf).unwrap();
            pages.push(pid);
        }

        let p0 = pages[0];
        let p1 = pages[1];
        let p2 = pages[2];

        // Swizzle p0.
        let _g = pool.pin_page(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        let _g = pool.pin_page(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        assert!(pool.is_swizzled(p0));

        // Unswizzle p0.
        pool.unswizzle(p0).unwrap();
        assert!(!pool.is_swizzled(p0), "p0 should no longer be swizzled");

        // Verify access_count was reset.
        {
            let frames = pool.inner.frames.read();
            let page_table = pool.inner.page_table.read();
            let fid = page_table[&p0];
            assert_eq!(frames[fid].access_count, 0);
            assert!(!frames[fid].swizzled);
        }

        // Now load p1 and p2 to force eviction — p0 should be evictable.
        let _g1 = pool.pin_page(p1).unwrap();
        pool.unpin_page(p1, false).unwrap();

        let _g2 = pool.pin_page(p2).unwrap();
        pool.unpin_page(p2, false).unwrap();

        // p0 should have been evicted (it was LRU). Verify we can still reload
        // it from disk.
        let g0_again = pool.pin_page(p0).unwrap();
        assert_eq!(
            g0_again.data()[0], 0xAA,
            "p0 should be reloaded from disk after eviction"
        );
        pool.unpin_page(p0, false).unwrap();
    }

    #[test]
    fn test_hot_page_count() {
        let (pool, fm) = make_pool_with_threshold(4, 1);

        assert_eq!(pool.hot_page_count(), 0);

        // Allocate and swizzle two pages.
        let p0 = fm.allocate_page().unwrap();
        let p1 = fm.allocate_page().unwrap();
        let p2 = fm.allocate_page().unwrap();
        for pid in [p0, p1, p2] {
            let buf = [0u8; PAGE_SIZE];
            fm.write_page(pid, &buf).unwrap();
        }

        // Swizzle p0 (2 pins, threshold=1).
        let _g = pool.pin_page(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        let _g = pool.pin_page(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        assert_eq!(pool.hot_page_count(), 1);

        // Swizzle p1.
        let _g = pool.pin_page(p1).unwrap();
        pool.unpin_page(p1, false).unwrap();
        let _g = pool.pin_page(p1).unwrap();
        pool.unpin_page(p1, false).unwrap();
        assert_eq!(pool.hot_page_count(), 2);

        // p2 pinned only once — should NOT be swizzled.
        let _g = pool.pin_page(p2).unwrap();
        pool.unpin_page(p2, false).unwrap();
        assert_eq!(pool.hot_page_count(), 2);

        // Unswizzle p0.
        pool.unswizzle(p0).unwrap();
        assert_eq!(pool.hot_page_count(), 1);

        // Unswizzle p1.
        pool.unswizzle(p1).unwrap();
        assert_eq!(pool.hot_page_count(), 0);
    }

    #[test]
    fn test_pin_page_ref_delegates_to_pin_page() {
        // Verify that pin_page_ref behaves identically to pin_page.
        let (pool, fm) = make_pool_with_threshold(4, 2);

        let p0 = fm.allocate_page().unwrap();
        let mut buf = [0u8; PAGE_SIZE];
        buf[0] = 0xEE;
        fm.write_page(p0, &buf).unwrap();

        // pin_page_ref should load the page and track access.
        let guard = pool.pin_page_ref(p0).unwrap();
        assert_eq!(guard.data()[0], 0xEE);
        assert_eq!(guard.page_id(), p0);
        pool.unpin_page(p0, false).unwrap();

        // Two more pins via pin_page_ref should swizzle (threshold=2, 3>2).
        let _g = pool.pin_page_ref(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        let _g = pool.pin_page_ref(p0).unwrap();
        pool.unpin_page(p0, false).unwrap();
        assert!(pool.is_swizzled(p0));
    }
}

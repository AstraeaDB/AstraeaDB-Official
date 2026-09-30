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
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::page::PAGE_SIZE;
use crate::page_io::PageIO;

/// Index into the frame table.
pub type FrameId = usize;

/// Bookkeeping for a single frame — everything *except* the raw page bytes.
/// Lives inside [`Meta`], behind the single [`BufferPoolInner::meta`] mutex.
struct FrameMeta {
    /// The page currently loaded into this frame (None if the frame is free).
    page_id: Option<PageId>,
    /// Whether the page data has been modified since it was loaded.
    dirty: bool,
    /// Number of active references to this frame. A page cannot be evicted while pinned.
    pin_count: u32,
    /// Number of times this page has been pinned. Used to decide when to promote
    /// the page into the swizzled hot set.
    access_count: u64,
    /// Whether this frame has been promoted to the hot set (pointer-swizzled).
    /// Swizzled frames are never evicted from the buffer pool.
    swizzled: bool,
}

impl FrameMeta {
    fn new() -> Self {
        Self {
            page_id: None,
            dirty: false,
            pin_count: 0,
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
        let data = self.pool.data.read();
        let mut buf = [0u8; PAGE_SIZE];
        buf.copy_from_slice(data[self.frame_id].as_ref());
        PageData(buf)
    }

    /// Get the page ID of this guard.
    pub fn page_id(&self) -> PageId {
        self.page_id
    }

    /// Write data into the page through this guard, marking it dirty.
    pub fn write_data(&self, new_data: &[u8; PAGE_SIZE]) {
        {
            let mut data = self.pool.data.write();
            data[self.frame_id].copy_from_slice(new_data);
        }
        // Sequential, not nested with `data` above — see `BufferPoolInner`
        // docs on why `data` and `meta` are never held at the same time
        // except with `meta` outermost.
        self.pool.meta.lock().frames[self.frame_id].dirty = true;
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

/// All buffer-pool bookkeeping other than the raw page bytes, behind a
/// single [`Mutex`]. See `BufferPoolInner`'s doc comment for why this
/// replaced four separate `RwLock`s (astraeadb-issues.md #36).
struct Meta {
    frames: Vec<FrameMeta>,
    /// Maps page_id -> frame_id for currently loaded pages.
    page_table: HashMap<PageId, FrameId>,
    /// LRU list of unpinned frame IDs (front = least recently used).
    lru: VecDeque<FrameId>,
    /// The set of page IDs currently in the swizzled hot set.
    hot_pages: HashSet<PageId>,
}

impl Meta {
    /// Bump an already-resident frame's pin/access bookkeeping and promote
    /// it to the swizzled hot set if it just crossed `swizzle_threshold`.
    /// Also removes the frame from `lru` — a pinned frame must never be
    /// there.
    fn bump_pin(&mut self, frame_id: FrameId, page_id: PageId, swizzle_threshold: u64) {
        self.lru.retain(|&fid| fid != frame_id);
        let frame = &mut self.frames[frame_id];
        frame.pin_count += 1;
        frame.access_count += 1;
        if !frame.swizzled && frame.access_count > swizzle_threshold {
            frame.swizzled = true;
            self.hot_pages.insert(page_id);
        }
    }

    /// Reserve a frame for a page that is *not* currently resident
    /// (`page_table` has no entry for it): find a free frame, or evict the
    /// LRU unpinned one. The returned frame is immediately marked
    /// `pin_count = 1` and removed from `lru` — fully private to the
    /// caller, unreachable via `page_table` (no entry yet) or `lru` (just
    /// removed) — so it is safe for the caller to load the real page bytes
    /// into it with no lock held and no risk of anyone else touching this
    /// exact frame in the meantime.
    ///
    /// If eviction was required and the victim was dirty, returns its old
    /// page id and bytes so the caller can flush them to disk *outside*
    /// this lock.
    fn reserve_frame(
        &mut self,
        requested_page_id: PageId,
        data: &[Box<[u8; PAGE_SIZE]>],
    ) -> Result<(FrameId, Option<(PageId, [u8; PAGE_SIZE])>)> {
        // Free frame first (no page loaded).
        if let Some(i) = self
            .lru
            .iter()
            .position(|&fid| self.frames[fid].page_id.is_none())
        {
            let frame_id = self.lru.remove(i).unwrap();
            let frame = &mut self.frames[frame_id];
            frame.dirty = false;
            frame.pin_count = 1;
            frame.access_count = 1;
            frame.swizzled = false;
            return Ok((frame_id, None));
        }

        // All frames have pages — evict the LRU unpinned, non-swizzled one.
        let idx = self
            .lru
            .iter()
            .position(|&fid| !self.frames[fid].swizzled)
            .ok_or(AstraeaError::BufferPoolFull(requested_page_id))?;
        let frame_id = self.lru.remove(idx).unwrap();

        let flush_job = {
            let frame = &self.frames[frame_id];
            if frame.dirty {
                frame.page_id.map(|pid| (pid, *data[frame_id]))
            } else {
                None
            }
        };
        let old_page_id = self.frames[frame_id].page_id.take();
        if let Some(old_pid) = old_page_id {
            self.page_table.remove(&old_pid);
        }
        let frame = &mut self.frames[frame_id];
        frame.dirty = false;
        frame.pin_count = 1;
        frame.access_count = 1;
        frame.swizzled = false;

        Ok((frame_id, flush_job))
    }

    /// Publish a frame that was just reserved via [`Self::reserve_frame`]
    /// and had its real bytes loaded, under `page_id` — unless another
    /// thread's concurrent miss on the same `page_id` already published
    /// first, in which case give `frame_id` back to the pool as free and
    /// bump the winner's pin count instead. Returns the frame the caller
    /// should actually use.
    fn publish_or_yield(
        &mut self,
        frame_id: FrameId,
        page_id: PageId,
        swizzle_threshold: u64,
    ) -> FrameId {
        if let Some(&winner) = self.page_table.get(&page_id) {
            self.frames[frame_id] = FrameMeta::new();
            self.lru.push_back(frame_id);
            self.bump_pin(winner, page_id, swizzle_threshold);
            winner
        } else {
            self.frames[frame_id].page_id = Some(page_id);
            self.page_table.insert(page_id, frame_id);
            frame_id
        }
    }

    fn unpin(&mut self, page_id: PageId, dirty: bool) {
        let Some(&frame_id) = self.page_table.get(&page_id) else {
            return; // Page not in pool, nothing to do.
        };
        let frame = &mut self.frames[frame_id];
        if dirty {
            frame.dirty = true;
        }
        if frame.pin_count > 0 {
            frame.pin_count -= 1;
        }
        // Swizzled frames are never returned to the LRU — they stay
        // permanently cached.
        if frame.pin_count == 0 && !frame.swizzled && !self.lru.contains(&frame_id) {
            self.lru.push_back(frame_id);
        }
    }

    fn unswizzle(&mut self, page_id: PageId) {
        self.hot_pages.remove(&page_id);
        let Some(&frame_id) = self.page_table.get(&page_id) else {
            return; // Page not in pool, nothing to do.
        };
        let frame = &mut self.frames[frame_id];
        frame.swizzled = false;
        frame.access_count = 0;
        if frame.pin_count == 0 && !self.lru.contains(&frame_id) {
            self.lru.push_back(frame_id);
        }
    }
}

/// Internal state of the buffer pool.
///
/// ## Lock design (astraeadb-issues.md #36)
///
/// Earlier revisions of this module split bookkeeping across four
/// independent `parking_lot::RwLock`s (`page_table`, `lru`, `frames`,
/// `hot_pages`). `pin_page`'s page-hit path acquired `frames` then, nested
/// inside it, `lru`; `find_or_evict_frame` acquired `lru` then, nested
/// inside it, `frames` — the opposite order. Two threads racing those two
/// paths deadlocked every time, `parking_lot::RwLock` being neither
/// reentrant nor timeout-based. Reproduced by
/// `tests/buffer_pool_stress.rs::concurrent_pin_unpin_does_not_deadlock`.
///
/// Reordering the locks closed that deadlock, but exposed two more subtle,
/// pre-existing correctness races that the deadlock had been masking (a
/// hang never gave them a chance to manifest): a frame could be evicted out
/// as stale-but-not-yet-committed data out from under a concurrent pinner
/// (details on the old `find_or_evict_frame`), and a frame removed from
/// `lru` for eviction could be raced back onto `lru` by an unrelated
/// concurrent pin+unpin cycle, letting a third thread claim it a second
/// time. Each fix required more cross-lock coordination, and each new fix
/// risked introducing yet another such window.
///
/// Given that, this module now takes the alternative the issue suggested:
/// **all bookkeeping lives in one [`Meta`] behind a single [`Mutex`]**
/// (`page_table`, `lru`, `hot_pages`, and every frame's metadata — page id,
/// pin count, dirty flag, access count, swizzled flag). Every state
/// transition (checking for a cache hit, picking an eviction victim,
/// bumping a pin count, promoting to the hot set, publishing a freshly
/// loaded page) is one atomic critical section under that single lock, so
/// none of the TOCTOU windows above can exist: there is no "lock A, then
/// later lock B" to get backwards, and no gap between "decide" and "commit"
/// for another thread to land in.
///
/// The raw page **bytes** are deliberately *not* behind that mutex — they
/// live in `data: RwLock<Vec<Box<[u8; PAGE_SIZE]>>>` instead, so that
/// concurrent reads of already-cached pages (`PageGuard::data`) only
/// contend with each other and with writers, never with unrelated
/// pin/unpin/evict bookkeeping on other pages. Disk I/O
/// (`PageIO::read_page`/`write_page`) is always done with neither lock
/// held. The one fixed ordering rule that remains: **when both are needed
/// together, `meta` is acquired before `data`, never the other way round**
/// (see e.g. `reserve_frame`, called with `meta` held and `data` passed in
/// by reference for a brief nested read) — since this is the only pairing
/// left, and it's used consistently in one direction, it cannot deadlock.
struct BufferPoolInner {
    meta: Mutex<Meta>,
    /// Raw page bytes, indexed by [`FrameId`]. See the struct doc comment
    /// for why this is split out from `meta`.
    data: RwLock<Vec<Box<[u8; PAGE_SIZE]>>>,
    /// Maximum number of frames (capacity).
    capacity: usize,
    /// Access count threshold before a page is promoted to the swizzled hot set.
    /// Once a page's cumulative pin count exceeds this value, it becomes
    /// permanently resident in memory (until explicitly unswizzled).
    swizzle_threshold: u64,
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
        let mut data = Vec::with_capacity(capacity);
        let mut lru = VecDeque::with_capacity(capacity);
        for i in 0..capacity {
            frames.push(FrameMeta::new());
            data.push(Box::new([0u8; PAGE_SIZE]));
            lru.push_back(i);
        }

        let inner = Arc::new(BufferPoolInner {
            meta: Mutex::new(Meta {
                frames,
                page_table: HashMap::new(),
                lru,
                hot_pages: HashSet::new(),
            }),
            data: RwLock::new(data),
            capacity,
            swizzle_threshold: Self::DEFAULT_SWIZZLE_THRESHOLD,
        });

        Self { inner, page_io }
    }

    /// Pin a page, loading it from disk if not already cached.
    /// Returns a guard for reading/writing the page data.
    ///
    /// Each call increments the page's access counter. When the counter exceeds
    /// the swizzle threshold, the page is promoted to the hot set and will not
    /// be evicted until explicitly unswizzled.
    pub fn pin_page(&self, page_id: PageId) -> Result<PageGuard> {
        enum Outcome {
            Hit(FrameId),
            Reserved(FrameId, Option<(PageId, [u8; PAGE_SIZE])>),
        }

        // One atomic decision: either this is a hit (bump pin/access and
        // possibly promote to swizzled), or we reserve a frame for a miss
        // (possibly evicting, in which case we get back the victim's bytes
        // to flush if it was dirty). See `BufferPoolInner`'s doc comment for
        // why this being a single critical section matters.
        let outcome = {
            let mut meta = self.inner.meta.lock();
            if let Some(&frame_id) = meta.page_table.get(&page_id) {
                meta.bump_pin(frame_id, page_id, self.inner.swizzle_threshold);
                Outcome::Hit(frame_id)
            } else {
                let data = self.inner.data.read();
                let (frame_id, flush_job) = meta.reserve_frame(page_id, &data)?;
                Outcome::Reserved(frame_id, flush_job)
            }
        };

        let frame_id = match outcome {
            Outcome::Hit(frame_id) => {
                return Ok(PageGuard {
                    frame_id,
                    page_id,
                    pool: Arc::clone(&self.inner),
                });
            }
            Outcome::Reserved(frame_id, flush_job) => {
                // No locks held across disk I/O.
                if let Some((old_pid, bytes)) = flush_job {
                    self.page_io.write_page(old_pid, &bytes)?;
                }
                frame_id
            }
        };

        // Load the real page data with no lock held, then install it —
        // `frame_id` is unreachable via `page_table` or `lru` until we
        // publish it below, so nobody else can touch it in the meantime.
        let page_data = self.page_io.read_page(page_id)?;
        {
            let mut data = self.inner.data.write();
            data[frame_id].copy_from_slice(&page_data);
        }

        // Publish. Two concurrent misses on the same `page_id` can each
        // reserve a *different* frame before either gets here — whichever
        // publishes first wins, and the loser's `publish_or_yield` hands
        // its frame back to the pool and bumps the winner's pin count
        // instead, so both callers converge on the same frame_id.
        let final_frame_id = {
            let mut meta = self.inner.meta.lock();
            meta.publish_or_yield(frame_id, page_id, self.inner.swizzle_threshold)
        };

        Ok(PageGuard {
            frame_id: final_frame_id,
            page_id,
            pool: Arc::clone(&self.inner),
        })
    }

    /// Pin a new page (allocate on disk and bring into the pool).
    pub fn pin_new_page(&self, page_data: &[u8; PAGE_SIZE]) -> Result<PageGuard> {
        let page_id = self.page_io.allocate_page()?;

        // Write the initial data to disk.
        self.page_io.write_page(page_id, page_data)?;

        let (frame_id, flush_job) = {
            let mut meta = self.inner.meta.lock();
            let data = self.inner.data.read();
            meta.reserve_frame(page_id, &data)?
        };
        if let Some((old_pid, bytes)) = flush_job {
            self.page_io.write_page(old_pid, &bytes)?;
        }

        {
            let mut data = self.inner.data.write();
            data[frame_id].copy_from_slice(page_data);
        }

        // `page_id` was just freshly allocated by `page_io`, so it cannot
        // already be in `page_table` — no publish/yield race to handle here.
        {
            let mut meta = self.inner.meta.lock();
            meta.frames[frame_id].page_id = Some(page_id);
            meta.page_table.insert(page_id, frame_id);
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
        self.inner.meta.lock().unpin(page_id, dirty);
        Ok(())
    }

    /// Flush a specific page to disk if it is dirty.
    pub fn flush_page(&self, page_id: PageId) -> Result<()> {
        let job = {
            let mut meta = self.inner.meta.lock();
            let Some(&frame_id) = meta.page_table.get(&page_id) else {
                return Ok(());
            };
            if !meta.frames[frame_id].dirty {
                return Ok(());
            }
            let data = self.inner.data.read();
            let bytes = *data[frame_id];
            meta.frames[frame_id].dirty = false;
            (page_id, bytes)
        };
        self.page_io.write_page(job.0, &job.1)?;
        Ok(())
    }

    /// Flush all dirty pages to disk.
    pub fn flush_all(&self) -> Result<()> {
        // Collect pages to flush while holding the lock briefly.
        let to_flush: Vec<(PageId, [u8; PAGE_SIZE])> = {
            let mut meta = self.inner.meta.lock();
            let data = self.inner.data.read();
            let mut jobs = Vec::new();
            for (frame_id, frame) in meta.frames.iter_mut().enumerate() {
                if frame.dirty {
                    if let Some(pid) = frame.page_id {
                        jobs.push((pid, *data[frame_id]));
                        frame.dirty = false;
                    }
                }
            }
            jobs
        };

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
        self.inner.meta.lock().hot_pages.contains(&page_id)
    }

    /// Remove a page from the swizzled hot set, allowing it to be evicted
    /// again under LRU pressure.
    ///
    /// Resets the page's access counter and — if the page is currently
    /// unpinned — adds it back to the LRU queue.
    pub fn unswizzle(&self, page_id: PageId) -> Result<()> {
        self.inner.meta.lock().unswizzle(page_id);
        Ok(())
    }

    /// Return the number of pages currently in the swizzled hot set.
    pub fn hot_page_count(&self) -> usize {
        self.inner.meta.lock().hot_pages.len()
    }

    /// Install `page_data` into the pool at an existing `page_id`.
    ///
    /// Unlike [`Self::pin_new_page`], this does NOT call `allocate_page` on
    /// the underlying [`PageIO`] — the caller supplies the page id, typically
    /// one that was previously freed by compaction. Any old data at `page_id`
    /// (in the pool or on disk) is overwritten. Returns a pinned
    /// [`PageGuard`] for subsequent writes.
    ///
    /// Assumes the caller has exclusive access to `page_id` for the duration
    /// of the call (true of the compaction/free-list-reuse callers this
    /// exists for) — it does not itself guard against a concurrent pinner of
    /// the same, about-to-be-recycled `page_id`.
    pub fn pin_recycled_page(
        &self,
        page_id: PageId,
        page_data: &[u8; PAGE_SIZE],
    ) -> Result<PageGuard> {
        // Reuse an existing frame for this page id if the pool already holds
        // one; otherwise find or evict a frame.
        let (frame_id, flush_job) = {
            let mut meta = self.inner.meta.lock();
            if let Some(&frame_id) = meta.page_table.get(&page_id) {
                meta.lru.retain(|&fid| fid != frame_id);
                (frame_id, None)
            } else {
                let data = self.inner.data.read();
                meta.reserve_frame(page_id, &data)?
            }
        };
        if let Some((old_pid, bytes)) = flush_job {
            self.page_io.write_page(old_pid, &bytes)?;
        }

        // Overwrite on-disk content first so any later eviction of this frame
        // cannot race with stale bytes on disk.
        self.page_io.write_page(page_id, page_data)?;

        {
            let mut data = self.inner.data.write();
            data[frame_id].copy_from_slice(page_data);
        }
        {
            let mut meta = self.inner.meta.lock();
            meta.frames[frame_id] = FrameMeta {
                page_id: Some(page_id),
                dirty: false, // in-pool content matches disk
                pin_count: 1,
                access_count: 0,
                swizzled: false,
            };
            meta.page_table.insert(page_id, frame_id);
            // Pinned frames are not in the LRU.
            meta.lru.retain(|&fid| fid != frame_id);
            // A recycled page id should not remain in the hot set across a
            // free/reuse cycle.
            meta.hot_pages.remove(&page_id);
        }

        Ok(PageGuard {
            frame_id,
            page_id,
            pool: Arc::clone(&self.inner),
        })
    }

    /// Discard every currently cached page — dirty or clean — and reset the
    /// pool to its initial empty state, WITHOUT flushing anything to disk.
    ///
    /// This exists for [`DiskStorageEngine::compact`](crate::engine::DiskStorageEngine::compact):
    /// once compaction has already read every live record's bytes into
    /// memory and is about to truncate and rewrite the underlying file from
    /// scratch, any dirty frame still holding pre-compaction content must
    /// NOT be flushed — flushing here would either resurrect stale bytes at
    /// a page id compaction has already repurposed for different content, or
    /// silently re-extend a file that was just truncated (since
    /// `FileManager::write_page` seeks-and-writes at an absolute offset,
    /// which grows the file if that offset is now past EOF).
    ///
    /// Callers must guarantee nothing needs the cached content anymore
    /// before calling this — the buffer pool cannot tell "safe to discard"
    /// apart from "would lose data" on its own.
    pub fn invalidate_all(&self) {
        {
            let mut meta = self.inner.meta.lock();
            for frame in meta.frames.iter_mut() {
                *frame = FrameMeta::new();
            }
            meta.page_table.clear();
            meta.hot_pages.clear();
            meta.lru.clear();
            meta.lru.extend(0..self.inner.capacity);
        }
        let mut data = self.inner.data.write();
        for buf in data.iter_mut() {
            buf.fill(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_manager::FileManager;
    use crate::page::{PageType, init_page};
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
    fn make_pool_with_threshold(capacity: usize, threshold: u64) -> (BufferPool, Arc<FileManager>) {
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
            let meta = pool.inner.meta.lock();
            let frame_id = meta.page_table[&p0];
            assert_eq!(
                meta.frames[frame_id].access_count, expected,
                "access_count should be {} after {} pins",
                expected, expected
            );
            drop(meta);
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
            let meta = pool.inner.meta.lock();
            let frame_id = meta.page_table[&p0];
            assert!(meta.frames[frame_id].swizzled);
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
            let meta = pool.inner.meta.lock();
            let frame_id_p0 = meta.page_table[&p0];
            assert!(
                !meta.lru.contains(&frame_id_p0),
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
        assert_eq!(
            g0_final.data()[0],
            0xAA,
            "swizzled page p0 must not be evicted"
        );
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
            let meta = pool.inner.meta.lock();
            let fid = meta.page_table[&p0];
            assert_eq!(meta.frames[fid].access_count, 0);
            assert!(!meta.frames[fid].swizzled);
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
            g0_again.data()[0],
            0xAA,
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
    fn test_invalidate_all_drops_dirty_frames_without_flushing() {
        let (pool, fm) = make_pool(4);

        // Load and dirty a page in the pool without unpinning-with-flush.
        let page_buf = init_page(PageId(0), PageType::NodePage);
        let guard = pool.pin_new_page(&page_buf).unwrap();
        let page_id = guard.page_id();
        let mut modified = page_buf;
        modified[100] = 0xFF;
        guard.write_data(&modified);
        pool.unpin_page(page_id, true).unwrap();

        // Truncate the underlying file out from under the pool (simulating
        // what DiskStorageEngine::compact does: read what it needs, then
        // shrink the file).
        fm.truncate_to(0).unwrap();

        // invalidate_all must NOT write the dirty frame back — doing so
        // would silently re-extend the just-truncated file.
        pool.invalidate_all();
        assert_eq!(
            fm.page_count().unwrap(),
            0,
            "invalidate_all must not flush and therefore must not re-grow the file"
        );

        // Pool must be back to a clean, empty state — pinning a fresh page
        // now allocates PageId(0) again, exactly like a new pool.
        let page_buf2 = init_page(PageId(0), PageType::NodePage);
        let guard2 = pool.pin_new_page(&page_buf2).unwrap();
        assert_eq!(guard2.page_id(), PageId(0));
        pool.unpin_page(guard2.page_id(), false).unwrap();
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

    #[test]
    fn test_buffer_pool_full_reports_requested_page_id() {
        // Regression test for astraeadb-issues #24: BufferPoolFull used to
        // hardcode PageId(0) instead of naming the page that couldn't be
        // brought in.
        //
        // Pool of capacity 1. Pin page A and hold the guard so the sole
        // frame stays pinned (never returns to the LRU) — the pool is now
        // "full" with no evictable frame. Requesting a distinct page B must
        // fail, and the error must name B (not A, and not a placeholder
        // PageId(0), since B != PageId(0)).
        let (pool, fm) = make_pool(1);

        let page_a = fm.allocate_page().unwrap();
        let mut buf_a = [0u8; PAGE_SIZE];
        buf_a[0] = 0xAA;
        fm.write_page(page_a, &buf_a).unwrap();
        assert_eq!(page_a, PageId(0));

        let page_b = fm.allocate_page().unwrap();
        let mut buf_b = [0u8; PAGE_SIZE];
        buf_b[0] = 0xBB;
        fm.write_page(page_b, &buf_b).unwrap();
        assert_ne!(
            page_b,
            PageId(0),
            "page_b must differ from the old placeholder"
        );

        // Pin page A and keep the guard alive so its frame can't be evicted.
        let _guard_a = pool.pin_page(page_a).unwrap();

        // No free or evictable frame remains — requesting page B must fail.
        // (`PageGuard` doesn't implement `Debug`, so `unwrap_err` isn't
        // available — match instead.)
        let err = match pool.pin_page(page_b) {
            Err(e) => e,
            Ok(_) => panic!("expected pin_page to fail with BufferPoolFull"),
        };
        let message = err.to_string();

        match err {
            AstraeaError::BufferPoolFull(reported_page_id) => {
                assert_eq!(
                    reported_page_id, page_b,
                    "BufferPoolFull must report the page that was actually requested"
                );
            }
            other => panic!("expected BufferPoolFull, got {other:?}"),
        }

        // The Display impl must also surface the real id, not a placeholder.
        assert!(
            message.contains(&page_b.to_string()),
            "error message {message:?} should mention the requested page {page_b}"
        );
        assert!(
            !message.contains(&PageId(0).to_string()),
            "error message {message:?} should not fall back to the PageId(0) placeholder"
        );
    }
}

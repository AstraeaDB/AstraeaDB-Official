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
use parking_lot::{Condvar, Mutex, MutexGuard, RwLock};
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
    /// Bumped every time this frame's bytes are modified via
    /// [`PageGuard::write_data`]. Write-back paths record the epoch at the
    /// moment they snapshot the frame's bytes for a disk write, and only
    /// clear `dirty` afterward if the epoch hasn't moved on since — otherwise
    /// a newer write landed while the old write was in flight, and the page
    /// must stay dirty so a later flush doesn't silently lose it (review
    /// finding B1 on astraeadb-issues.md #36).
    write_epoch: u64,
}

impl FrameMeta {
    fn new() -> Self {
        Self {
            page_id: None,
            dirty: false,
            pin_count: 0,
            access_count: 0,
            swizzled: false,
            write_epoch: 0,
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
        let mut meta = self.pool.meta.lock();
        let frame = &mut meta.frames[self.frame_id];
        frame.dirty = true;
        frame.write_epoch = frame.write_epoch.wrapping_add(1);
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

/// A dirty victim's page id and bytes, captured by [`BufferPool::reserve_frame`]
/// at the moment it evicted them, to be written back to disk by
/// [`BufferPool::complete_writeback`] with no lock held.
struct WritebackJob {
    old_page_id: PageId,
    bytes: [u8; PAGE_SIZE],
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
    /// Page ids whose bytes are currently being (or about to be) written
    /// back to disk — by an eviction (`reserve_frame`/`complete_writeback`),
    /// `flush_page`, `flush_all`, or `pin_recycled_page`'s direct write.
    /// While a page_id is a key here, its value is the *authoritative*
    /// content for that page: a concurrent cache miss on it is served from
    /// here instead of disk (closes review finding B1 — the physical write
    /// can be arbitrarily delayed after the in-memory state has already
    /// moved on, and a disk read racing it would otherwise see stale bytes).
    /// A second attempt to write the SAME page_id while one is already
    /// in-flight waits (via `BufferPoolInner::writeback_done`) for the first
    /// to finish before proceeding, so writes to a single page_id can never
    /// land on disk out of order.
    writeback: HashMap<PageId, [u8; PAGE_SIZE]>,
    /// Bumped every time a page_id is evicted from a frame (`reserve_frame`),
    /// dirty or not. Lets a concurrent miss detect "a full publish -> modify
    /// -> evict cycle happened on this exact page_id while I was reading it"
    /// and retry instead of publishing outdated content.
    ///
    /// Why this is needed on top of `writeback`: `writeback` alone only
    /// protects a reader that's *currently* racing an in-flight write. It
    /// does not protect against a *fast* cycle that starts and fully
    /// completes (publish, modify, evict, write) between the moment a
    /// reader checks `writeback` (sees nothing) and the moment its disk read
    /// actually lands — once that cycle's own eviction has run,
    /// `page_table` is briefly empty again, so the stale reader's eventual
    /// `publish_or_yield` call finds *no* current winner to yield to and
    /// successfully publishes its outdated content as if it were current.
    /// This is exactly the "two readers, no writer-side bug in sight" shape
    /// `tests/buffer_pool_stress.rs`'s disjoint-ownership counter caught
    /// intermittently even after `writeback` and the "check immediately
    /// before use" fix: the culprit publishing stale data was consistently
    /// a reader thread, not the page's own writer.
    page_generation: HashMap<PageId, u64>,
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

    /// Publish a frame that was just reserved via [`BufferPool::reserve_frame`]
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
            frame.write_epoch = frame.write_epoch.wrapping_add(1);
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
/// hang never gave them a chance to manifest): a frame could be evicted and
/// handed to a different page based on a stale pin-count snapshot while a
/// concurrent caller still held a live `PageGuard` to it, and a frame
/// removed from `lru` for eviction could be raced back onto `lru` by an
/// unrelated concurrent pin+unpin cycle, letting a third thread claim it a
/// second time. Each fix required more cross-lock coordination, and each
/// new fix risked introducing yet another such window.
///
/// Given that, this module takes the alternative the issue suggested: **all
/// bookkeeping lives in one [`Meta`] behind a single [`Mutex`]** (`page_table`,
/// `lru`, `hot_pages`, `writeback`, and every frame's metadata — page id,
/// pin count, dirty flag, access count, swizzled flag, write epoch). Every
/// state transition (checking for a cache hit, picking an eviction victim,
/// bumping a pin count, promoting to the hot set, publishing a freshly
/// loaded page) is one atomic critical section under that single lock, so
/// the lock-order class of bug above cannot recur: there is no "lock A,
/// then later lock B" to get backwards.
///
/// That single-mutex redesign does **not**, by itself, make the disk
/// write-back of a dirty evicted page safe — the write still has to happen
/// with no lock held (disk I/O can be arbitrarily slow, and holding `meta`
/// across it would serialize the whole pool). The gap between "evict and
/// decide to write X back" (under the lock) and "the write actually lands"
/// (no lock held) is exactly where a second review pass (astraeadb-issues.md
/// #36 blocker B1) found a real lost-update/stale-read window: a concurrent
/// miss on the just-evicted page_id could race the pending write and see
/// stale on-disk bytes, and two write-backs of the same page_id (e.g. one
/// from eviction, one from a concurrent `flush_page`) could land out of
/// order. `Meta::writeback` closes this: a page_id present there has its
/// authoritative content held in memory (so a concurrent miss is served
/// from it instead of disk), and `BufferPoolInner::writeback_done` lets a
/// second writer of the same page_id wait for the first to finish rather
/// than racing it. See `Meta::writeback`'s doc comment and
/// [`BufferPool::reserve_frame`]/[`BufferPool::complete_writeback`].
///
/// The raw page **bytes** are deliberately *not* behind that mutex — they
/// live in `data: RwLock<Vec<Box<[u8; PAGE_SIZE]>>>` instead, so that
/// concurrent reads of already-cached pages (`PageGuard::data`) only
/// contend with each other and with writers, never with unrelated
/// pin/unpin/evict bookkeeping on other pages. Disk I/O
/// (`PageIO::read_page`/`write_page`) is always done with neither lock
/// held. The one fixed ordering rule that remains: **when both are needed
/// together, `meta` is acquired before `data`, never the other way round**
/// — since this is the only pairing left, and it's used consistently in one
/// direction, it cannot deadlock.
struct BufferPoolInner {
    meta: Mutex<Meta>,
    /// Signalled whenever an entry is removed from `Meta::writeback`, so a
    /// thread waiting to write back the same page_id (or waiting to read it
    /// — though reads are served from the in-flight copy without waiting)
    /// can proceed. Always waited on together with `meta`'s guard.
    writeback_done: Condvar,
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
                writeback: HashMap::new(),
                page_generation: HashMap::new(),
            }),
            writeback_done: Condvar::new(),
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
        // One atomic decision: either this is a hit (bump pin/access and
        // possibly promote to swizzled), or we reserve a frame for a miss
        // (possibly evicting — see `reserve_frame` for how the victim's
        // write-back is registered before the lock is released).
        let reservation = {
            let mut meta = self.inner.meta.lock();
            if let Some(&frame_id) = meta.page_table.get(&page_id) {
                meta.bump_pin(frame_id, page_id, self.inner.swizzle_threshold);
                return Ok(PageGuard {
                    frame_id,
                    page_id,
                    pool: Arc::clone(&self.inner),
                });
            }
            self.reserve_frame(&mut meta, page_id)?
        };
        let (frame_id, flush_job) = reservation;

        // No lock held across disk I/O. On failure, `frame_id` is already
        // restored to represent the victim again (see `complete_writeback`)
        // — nothing further to clean up here.
        if let Some(job) = flush_job {
            self.complete_writeback(frame_id, job)?;
        }

        // Snapshot both: whether `page_id` itself has an in-flight
        // write-back (served from there instead of disk — review finding
        // B1), and its current `page_generation`. `gen_before` is what lets
        // us detect, after loading, whether a *complete* publish -> modify
        // -> evict cycle raced us on this exact page_id (see
        // `Meta::page_generation`'s doc comment) — `writeback` alone only
        // catches a reader racing a write that's *still* in flight, not one
        // that started and fully finished while we were loading.
        let (mut served_from_writeback, mut gen_before) = {
            let meta = self.inner.meta.lock();
            (
                meta.writeback.get(&page_id).copied(),
                meta.page_generation.get(&page_id).copied().unwrap_or(0),
            )
        };

        // Load the real page data with no lock held, then install it —
        // `frame_id` is unreachable via `page_table` or `lru` until we
        // publish it below, so nobody else can touch it in the meantime.
        // Retried if `page_generation` moved on between snapshotting above
        // and verifying just before publish: that means someone else's
        // full cycle landed on this exact page_id while we were loading,
        // and `page_data` may be one generation stale.
        let final_frame_id = loop {
            let page_data = match served_from_writeback {
                Some(bytes) => bytes,
                None => match self.page_io.read_page(page_id) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        // B3: don't leak the reservation on a failed read —
                        // give the frame back to the pool as free.
                        self.release_reservation(frame_id);
                        return Err(e);
                    }
                },
            };
            {
                let mut data = self.inner.data.write();
                data[frame_id].copy_from_slice(&page_data);
            }

            // Publish. Two concurrent misses on the same `page_id` can each
            // reserve a *different* frame before either gets here —
            // whichever publishes first wins, and the loser's
            // `publish_or_yield` hands its frame back to the pool and bumps
            // the winner's pin count instead, so both callers converge on
            // the same frame_id.
            let mut meta = self.inner.meta.lock();
            let current_gen = meta.page_generation.get(&page_id).copied().unwrap_or(0);
            if current_gen != gen_before {
                served_from_writeback = meta.writeback.get(&page_id).copied();
                gen_before = current_gen;
                drop(meta);
                continue;
            }

            let we_won = !meta.page_table.contains_key(&page_id);
            let final_frame_id =
                meta.publish_or_yield(frame_id, page_id, self.inner.swizzle_threshold);
            // Only mark dirty if *we* actually won and published `page_data`
            // (which is what `served_from_writeback` describes) — if we
            // lost the race, `final_frame_id` is some other caller's frame
            // with its own independently-loaded content, and we must not
            // touch its dirty/epoch state here.
            if we_won && served_from_writeback.is_some() {
                // These bytes are only *eventually* guaranteed to be on
                // disk (the write-back we copied them from may still be in
                // flight, or could even fail and be rolled back elsewhere)
                // — mark the page dirty so it gets a chance to be
                // (re)written by a future flush/eviction rather than
                // assuming disk already matches memory.
                meta.frames[final_frame_id].dirty = true;
                meta.frames[final_frame_id].write_epoch =
                    meta.frames[final_frame_id].write_epoch.wrapping_add(1);
            }
            break final_frame_id;
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

        // Write the initial data to disk. `page_id` was just allocated, so
        // it cannot collide with any in-flight write-back.
        self.page_io.write_page(page_id, page_data)?;

        let (frame_id, flush_job) = {
            let mut meta = self.inner.meta.lock();
            self.reserve_frame(&mut meta, page_id)?
        };
        if let Some(job) = flush_job {
            self.complete_writeback(frame_id, job)?;
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
        let (bytes, epoch) = {
            let mut meta = self.inner.meta.lock();
            loop {
                let Some(&frame_id) = meta.page_table.get(&page_id) else {
                    return Ok(());
                };
                if !meta.frames[frame_id].dirty {
                    return Ok(());
                }
                if meta.writeback.contains_key(&page_id) {
                    // Someone else (an eviction, or another flush) is
                    // already writing this exact page_id back — wait for
                    // it so our write can't land out of order with theirs.
                    self.inner.writeback_done.wait(&mut meta);
                    continue;
                }
                let epoch = meta.frames[frame_id].write_epoch;
                let bytes = *self.inner.data.read()[frame_id];
                meta.writeback.insert(page_id, bytes);
                break (bytes, epoch);
            }
        };

        let result = self.page_io.write_page(page_id, &bytes);
        {
            let mut meta = self.inner.meta.lock();
            meta.writeback.remove(&page_id);
            if result.is_ok()
                && let Some(&frame_id) = meta.page_table.get(&page_id)
                && meta.frames[frame_id].write_epoch == epoch
            {
                // Nobody wrote newer data to this frame while our write was
                // in flight — safe to clear dirty.
                meta.frames[frame_id].dirty = false;
            }
        }
        self.inner.writeback_done.notify_all();
        result
    }

    /// Flush all dirty pages to disk.
    ///
    /// Best-effort: a page whose write-back is already in flight from some
    /// other operation (a concurrent eviction, `flush_page`, or another
    /// `flush_all`) is skipped for this round rather than raced — it stays
    /// marked dirty, so a later flush will still catch it. Unlike the
    /// pre-rewrite version, a failed write does not abort the rest of the
    /// batch: every page gets a chance to flush, and the first error (if
    /// any) is returned after the loop, so one bad page can't also strand
    /// `writeback` registrations for every page after it.
    pub fn flush_all(&self) -> Result<()> {
        let to_flush: Vec<(PageId, FrameId, [u8; PAGE_SIZE], u64)> = {
            let mut meta = self.inner.meta.lock();
            let data = self.inner.data.read();
            let mut jobs = Vec::new();
            for frame_id in 0..meta.frames.len() {
                let (dirty, page_id) = {
                    let f = &meta.frames[frame_id];
                    (f.dirty, f.page_id)
                };
                let Some(pid) = page_id else { continue };
                if !dirty {
                    continue;
                }
                if meta.writeback.contains_key(&pid) {
                    continue;
                }
                let bytes = *data[frame_id];
                let epoch = meta.frames[frame_id].write_epoch;
                meta.writeback.insert(pid, bytes);
                jobs.push((pid, frame_id, bytes, epoch));
            }
            jobs
        };

        let mut first_err = None;
        for (pid, frame_id, bytes, epoch) in to_flush {
            let result = self.page_io.write_page(pid, &bytes);
            {
                let mut meta = self.inner.meta.lock();
                meta.writeback.remove(&pid);
                if result.is_ok()
                    && meta.frames[frame_id].page_id == Some(pid)
                    && meta.frames[frame_id].write_epoch == epoch
                {
                    meta.frames[frame_id].dirty = false;
                }
            }
            self.inner.writeback_done.notify_all();
            if let Err(e) = result
                && first_err.is_none()
            {
                first_err = Some(e);
            }
        }

        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
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
    /// This is reachable from ordinary (non-compaction) writes once the
    /// free list is non-empty — `DiskStorageEngine::write_overflow_page`/
    /// `write_record` call it under only `compaction_lock.read()`, the same
    /// guard an ordinary concurrent `get_node` holds — so a recycle of
    /// `page_id` can race a concurrent reader's miss on that same id
    /// (review finding B2). Two things are guaranteed despite that: (1) if
    /// `page_id` is already resident, this bumps its pin count instead of
    /// clobbering it, so a concurrent pinner's reference is never silently
    /// dropped; (2) if a concurrent miss publishes `page_id` to a different
    /// frame first, this takes over that frame (via the same
    /// `publish_or_yield` protocol `pin_page` uses) rather than leaking its
    /// own reservation or leaving two frames both claiming `page_id`. What
    /// is *not* guaranteed is which content a concurrent reader observes
    /// for `page_id` itself during the race — the caller is still expected
    /// to have exclusive application-level ownership of `page_id` for that.
    pub fn pin_recycled_page(
        &self,
        page_id: PageId,
        page_data: &[u8; PAGE_SIZE],
    ) -> Result<PageGuard> {
        let is_fresh;
        let frame_id = {
            let mut meta = self.inner.meta.lock();
            if let Some(&fid) = meta.page_table.get(&page_id) {
                // B2(a): bump pin_count under the lock exactly like the
                // ordinary hit path, instead of clobbering it to a flat `1`
                // further down — a concurrent pinner's own pin must not be
                // silently discarded (that was a path to premature LRU
                // re-admission of a frame a live `PageGuard` still points
                // at, and from there, eviction stealing it out from under
                // that guard).
                meta.bump_pin(fid, page_id, self.inner.swizzle_threshold);
                is_fresh = false;
                fid
            } else {
                let (fid, job) = self.reserve_frame(&mut meta, page_id)?;
                drop(meta);
                if let Some(job) = job {
                    self.complete_writeback(fid, job)?;
                }
                is_fresh = true;
                fid
            }
        };

        // Overwrite on-disk content first so any later eviction of this
        // frame cannot race with stale bytes on disk. Serialized against
        // any other in-flight write of this exact page_id for the same
        // reason `reserve_frame`/`flush_page`/`flush_all` are (review
        // finding B1) — without this, a concurrent flush of the
        // pre-recycle dirty content could land *after* this write and
        // silently revert the recycle on disk.
        if let Err(e) = self.write_page_serialized(page_id, *page_data) {
            if is_fresh {
                self.release_reservation(frame_id);
            } else {
                self.inner.meta.lock().unpin(page_id, false);
            }
            return Err(e);
        }

        if is_fresh {
            // B2(b): route through the same publish-or-yield protocol
            // `pin_page` uses instead of unconditionally inserting into
            // `page_table`. Without this, a concurrent `pin_page(page_id)`
            // miss that publishes first leaves this reservation mapped to
            // nothing (pin 1, page_id set, but absent from `page_table`):
            // the *other* caller's eventual `unpin_page` then drives this
            // frame's count to 0 and back onto `lru`, where a wholly
            // unrelated page's `pin_page` can steal it — corrupting that
            // third page, not just this one.
            {
                let mut data = self.inner.data.write();
                data[frame_id].copy_from_slice(page_data);
            }
            let final_frame_id = {
                let mut meta = self.inner.meta.lock();
                meta.publish_or_yield(frame_id, page_id, self.inner.swizzle_threshold)
            };
            if final_frame_id != frame_id {
                // Lost the race: `publish_or_yield` already freed our
                // reservation and bumped the winner's pin for us. The
                // winner's frame holds pre-recycle content, which is wrong
                // for a recycle — overwrite it too. (This function's
                // contract already assumes the caller has exclusive
                // ownership of `page_id`; this only has to avoid corrupting
                // *other* pages, not provide a consistent view of this one
                // to a caller that's racing that contract.)
                {
                    let mut data = self.inner.data.write();
                    data[final_frame_id].copy_from_slice(page_data);
                }
            }
            let mut meta = self.inner.meta.lock();
            let frame = &mut meta.frames[final_frame_id];
            frame.dirty = false;
            frame.write_epoch = frame.write_epoch.wrapping_add(1);
            if final_frame_id == frame_id {
                frame.access_count = 0;
                frame.swizzled = false;
                meta.hot_pages.remove(&page_id);
            }
            return Ok(PageGuard {
                frame_id: final_frame_id,
                page_id,
                pool: Arc::clone(&self.inner),
            });
        }

        // Existing-frame path: `frame_id` is already published and pinned
        // (bumped above) under `page_id`; just overwrite its content.
        {
            let mut data = self.inner.data.write();
            data[frame_id].copy_from_slice(page_data);
        }
        {
            let mut meta = self.inner.meta.lock();
            let frame = &mut meta.frames[frame_id];
            frame.dirty = false;
            frame.write_epoch = frame.write_epoch.wrapping_add(1);
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
            // Defensive: by the time a caller can safely invalidate
            // everything (see doc comment above), no write-back of ours
            // should still be in flight. Clear anyway rather than leave a
            // stale entry that could wedge a future wait forever.
            meta.writeback.clear();
        }
        let mut data = self.inner.data.write();
        for buf in data.iter_mut() {
            buf.fill(0);
        }
    }

    /// Reserve a frame for a page that is *not* currently resident
    /// (`meta.page_table` has no entry for it): find a free frame, or evict
    /// the LRU unpinned one. The returned frame is immediately marked
    /// `pin_count = 1` and removed from `lru` — fully private to the
    /// caller, unreachable via `page_table` (no entry yet) or `lru` (just
    /// removed) — so it is safe for the caller to load the real page bytes
    /// into it with no lock held and no risk of anyone else touching this
    /// exact frame in the meantime.
    ///
    /// If eviction was required and the victim was dirty, returns a
    /// [`WritebackJob`] for the caller to pass to [`Self::complete_writeback`]
    /// — the actual disk write must happen with no lock held, but this
    /// function has already registered the victim's bytes in
    /// `meta.writeback` (while still holding the lock, so there is no gap)
    /// so a concurrent miss on the evicted page_id is served from there
    /// instead of racing the pending write (review finding B1).
    ///
    /// If the victim's page_id already has a write-back in flight from an
    /// earlier eviction/flush (it was reloaded and re-dirtied before that
    /// earlier write finished), this waits for it to finish before
    /// registering its own — never two write-backs of the same page_id
    /// in flight at once, so they can't land out of order.
    fn reserve_frame(
        &self,
        meta: &mut MutexGuard<'_, Meta>,
        requested_page_id: PageId,
    ) -> Result<(FrameId, Option<WritebackJob>)> {
        loop {
            // Try to find an unused frame first (one with no page loaded).
            if let Some(i) = meta
                .lru
                .iter()
                .position(|&fid| meta.frames[fid].page_id.is_none())
            {
                let frame_id = meta.lru.remove(i).unwrap();
                let frame = &mut meta.frames[frame_id];
                frame.dirty = false;
                frame.pin_count = 1;
                frame.access_count = 1;
                frame.swizzled = false;
                return Ok((frame_id, None));
            }

            // All frames have pages — evict the LRU unpinned, non-swizzled
            // one. Peek before committing: if it needs a write-back whose
            // page_id already has one in flight, wait and re-evaluate
            // rather than racing it.
            let idx = meta
                .lru
                .iter()
                .position(|&fid| !meta.frames[fid].swizzled)
                .ok_or(AstraeaError::BufferPoolFull(requested_page_id))?;
            let frame_id = meta.lru[idx];
            let old_page_id = meta.frames[frame_id].page_id;
            let dirty = meta.frames[frame_id].dirty;

            if dirty
                && let Some(pid) = old_page_id
                && meta.writeback.contains_key(&pid)
            {
                self.inner.writeback_done.wait(meta);
                continue;
            }

            meta.lru.remove(idx);
            let job = if dirty {
                old_page_id.map(|pid| {
                    let bytes = *self.inner.data.read()[frame_id];
                    meta.writeback.insert(pid, bytes);
                    WritebackJob {
                        old_page_id: pid,
                        bytes,
                    }
                })
            } else {
                None
            };
            if let Some(pid) = old_page_id {
                meta.page_table.remove(&pid);
                // See `Meta::page_generation`'s doc comment: bump on every
                // eviction (not just dirty ones) so a concurrent miss that
                // started reading `pid` before this point can detect it and
                // retry instead of publishing stale content.
                *meta.page_generation.entry(pid).or_insert(0) += 1;
            }
            let frame = &mut meta.frames[frame_id];
            frame.page_id = None;
            frame.dirty = false;
            frame.pin_count = 1;
            frame.access_count = 1;
            frame.swizzled = false;

            return Ok((frame_id, job));
        }
    }

    /// Issue the actual disk write for a victim's dirty bytes evicted by
    /// [`Self::reserve_frame`] (which already registered `job.old_page_id`
    /// in `meta.writeback`, removed it from `page_table`, and cleared the
    /// frame it used to occupy). No lock is held during the write.
    ///
    /// On success, removes the `writeback` registration and wakes any
    /// threads waiting to write back the same page_id.
    ///
    /// On failure (review finding B3 — a regression this guards against:
    /// the pre-single-mutex version of this code kept a failed-flush victim
    /// mapped with `dirty = true`; an earlier draft of this rewrite instead
    /// discarded it, silently losing the page): restores `old_page_id` as a
    /// resident, dirty, unpinned page — mapped back into `page_table`,
    /// requeued onto `lru` — rather than leaking the frame. `frame_id`'s
    /// byte buffer is untouched at this point (the caller hasn't written
    /// the new page's data into it yet), so this is exactly restoring the
    /// pre-eviction state. The caller must propagate the error without
    /// touching `frame_id` further — it no longer represents the page the
    /// caller was trying to load.
    fn complete_writeback(&self, frame_id: FrameId, job: WritebackJob) -> Result<()> {
        let result = self.page_io.write_page(job.old_page_id, &job.bytes);
        {
            let mut meta = self.inner.meta.lock();
            meta.writeback.remove(&job.old_page_id);
            if result.is_err() {
                meta.frames[frame_id] = FrameMeta {
                    page_id: Some(job.old_page_id),
                    dirty: true,
                    pin_count: 0,
                    access_count: 0,
                    swizzled: false,
                    write_epoch: 1,
                };
                meta.page_table.insert(job.old_page_id, frame_id);
                if !meta.lru.contains(&frame_id) {
                    meta.lru.push_back(frame_id);
                }
            }
        }
        self.inner.writeback_done.notify_all();
        result
    }

    /// Write `bytes` for `page_id` to disk, serialized against any other
    /// in-flight write-back of the exact same `page_id` via `meta.writeback`
    /// — used by non-evicting direct writers (`pin_recycled_page`) so two
    /// writes to one page_id can never land out of order on disk (review
    /// finding B1). Unlike [`Self::complete_writeback`], this doesn't touch
    /// any frame's bookkeeping; the caller owns that.
    fn write_page_serialized(&self, page_id: PageId, bytes: [u8; PAGE_SIZE]) -> Result<()> {
        {
            let mut meta = self.inner.meta.lock();
            while meta.writeback.contains_key(&page_id) {
                self.inner.writeback_done.wait(&mut meta);
            }
            meta.writeback.insert(page_id, bytes);
        }
        let result = self.page_io.write_page(page_id, &bytes);
        self.inner.meta.lock().writeback.remove(&page_id);
        self.inner.writeback_done.notify_all();
        result
    }

    /// Return a frame that was reserved via [`Self::reserve_frame`] but
    /// never published (e.g. loading the real page bytes failed in
    /// `pin_page`'s miss path — review finding B3) back to the pool as a
    /// free, unpinned frame.
    ///
    /// The frame is guaranteed to hold no live data at this point (its old
    /// occupant, if any, was already safely flushed and unmapped by
    /// `reserve_frame`/`complete_writeback`), so resetting it to
    /// `FrameMeta::new()` cannot lose data.
    fn release_reservation(&self, frame_id: FrameId) {
        let mut meta = self.inner.meta.lock();
        meta.frames[frame_id] = FrameMeta::new();
        if !meta.lru.contains(&frame_id) {
            meta.lru.push_back(frame_id);
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

    #[test]
    fn test_write_failure_restores_dirty_victim_and_preserves_capacity() {
        // Review finding B3, regression guard: on a failed eviction
        // write-back, the victim must stay mapped+dirty (not be discarded)
        // and the pool's usable capacity must not shrink.
        use astraea_core::error::AstraeaError as Err_;
        use parking_lot::Mutex as PMutex;

        struct FlakyIO {
            fm: Arc<FileManager>,
            fail_next_write: PMutex<bool>,
        }
        impl PageIO for FlakyIO {
            fn read_page(&self, p: PageId) -> Result<[u8; PAGE_SIZE]> {
                self.fm.read_page(p)
            }
            fn write_page(&self, p: PageId, d: &[u8; PAGE_SIZE]) -> Result<()> {
                let mut fail = self.fail_next_write.lock();
                if *fail {
                    *fail = false;
                    return Err(Err_::Storage("simulated write failure".into()));
                }
                self.fm.write_page(p, d)
            }
            fn allocate_page(&self) -> Result<PageId> {
                self.fm.allocate_page()
            }
        }

        let tmp = NamedTempFile::new().unwrap();
        let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
        let _ = tmp.into_temp_path();
        let io = Arc::new(FlakyIO {
            fm: fm.clone(),
            fail_next_write: PMutex::new(false),
        });

        let p0 = io.allocate_page().unwrap();
        let p1 = io.allocate_page().unwrap();
        io.write_page(p0, &[0u8; PAGE_SIZE]).unwrap();
        io.write_page(p1, &[0u8; PAGE_SIZE]).unwrap();

        let pool = BufferPool::new(io.clone() as Arc<dyn PageIO>, 1);

        // Load and dirty p0, then unpin so it's evictable.
        let g0 = pool.pin_page(p0).unwrap();
        let mut b = g0.data().0;
        b[42] = 0x99;
        g0.write_data(&b);
        pool.unpin_page(p0, true).unwrap();

        // Arm the flake, then try to bring in p1 — forces eviction of p0,
        // whose write-back will fail.
        *io.fail_next_write.lock() = true;
        let err = match pool.pin_page(p1) {
            Err(e) => e,
            Ok(_) => panic!("expected pin_page(p1) to fail (write-back failure should propagate)"),
        };
        match err {
            Err_::Storage(_) => {}
            other => panic!("expected AstraeaError::Storage, got {other:?}"),
        }

        // p0 must still be resident, dirty, and reloadable with its
        // modified content intact — not discarded.
        {
            let meta = pool.inner.meta.lock();
            let frame_id = *meta
                .page_table
                .get(&p0)
                .expect("p0 must still be mapped after a failed flush");
            assert!(
                meta.frames[frame_id].dirty,
                "p0 must still be marked dirty after a failed flush"
            );
            assert_eq!(meta.frames[frame_id].pin_count, 0);
        }
        let g0_again = pool.pin_page(p0).unwrap();
        assert_eq!(
            g0_again.data()[42],
            0x99,
            "p0's modified content must survive a failed write-back"
        );
        pool.unpin_page(p0, false).unwrap();

        // Pool capacity must not have shrunk: p1 can still be brought in
        // (evicting p0 again, this time successfully).
        let g1 = pool.pin_page(p1).unwrap();
        pool.unpin_page(p1, false).unwrap();
        drop(g1);
        assert!(fm.read_page(p0).is_ok());
    }
}

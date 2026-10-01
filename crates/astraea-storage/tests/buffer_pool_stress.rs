//! Stress / deadlock-repro test for astraeadb-issues.md #36:
//! "`BufferPool` deadlocks under concurrent reads (lock-order inversion)".
//!
//! The reported cause is a lock-order inversion between `BufferPool::pin_page`'s
//! page-hit path (which used to acquire `frames` then, nested inside it,
//! `lru`) and `find_or_evict_frame` (which acquires `lru` then, nested inside
//! it, `frames`). Two threads racing those two paths — one hitting an
//! already-cached page, one missing and needing to evict — can deadlock with
//! each holding the lock the other wants.
//!
//! This test drives a buffer pool much smaller than the working set with
//! several concurrent reader/writer threads pinning random pages in a tight
//! loop, which forces constant eviction traffic and makes the hit path and
//! the miss/evict path race against each other on essentially every
//! iteration. It runs the actual workload on a background thread and waits
//! on a channel with a bounded timeout (`WATCHDOG_TIMEOUT`, currently 20s),
//! so a deadlock shows up as a test **failure** within that window instead
//! of a hung test binary.
//!
//! On top of the deadlock repro, each writer owns a disjoint partition of
//! pages (no two writer threads ever touch the same page) and increments a
//! counter embedded in its pages; after the run, every writer-owned page is
//! re-read straight from disk and checked against the exact expected count.
//! This is the same disjoint-ownership shape as
//! `tests/bufpool_lost_update.rs`, folded into the heavier
//! many-threads/small-pool workload here so a lost update under real
//! eviction *and* read pressure fails this test too, not just the
//! dedicated, lighter-weight one (review finding B1 on astraeadb-issues.md
//! #36: a page-id-header check alone can't catch a lost update, since the
//! header isn't what gets clobbered).

use astraea_core::types::PageId;
use astraea_storage::buffer_pool::BufferPool;
use astraea_storage::file_manager::FileManager;
use astraea_storage::page::{PageType, init_page};
use astraea_storage::page_io::PageIO;
use rand::Rng;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

/// Pool capacity is deliberately much smaller than the number of distinct
/// pages threads pin, so every thread is constantly forcing eviction of
/// frames other threads may be about to touch.
const POOL_CAPACITY: usize = 8;
const NUM_PAGES: usize = 200;
const NUM_READERS: usize = 6;
const NUM_WRITERS: usize = 2;
const ITERS_PER_THREAD: usize = 3000;
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(20);

/// Byte offset of the per-writer counter each writer thread increments.
/// Bytes `0..17` are the page header embedded by `init_page`/
/// `PageHeader::write_to` (page_id, type, record_count, free_space_offset,
/// checksum) — this must not overlap it.
const COUNTER_OFFSET: usize = 100;

fn setup_pool() -> (BufferPool, Vec<PageId>, Arc<FileManager>) {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
    // Keep the backing file alive for the duration of the test process.
    let _ = tmp.into_temp_path();

    let mut page_ids = Vec::with_capacity(NUM_PAGES);
    for i in 0..NUM_PAGES {
        let buf = init_page(PageId(i as u64), PageType::NodePage);
        let pid = fm.allocate_page().unwrap();
        fm.write_page(pid, &buf).unwrap();
        page_ids.push(pid);
    }

    let pool = BufferPool::new(fm.clone() as Arc<dyn PageIO>, POOL_CAPACITY);
    (pool, page_ids, fm)
}

/// Runs the actual concurrent-access workload and verifies no lost updates.
/// Panics (via an assertion) on any failure; returns normally only if every
/// spawned thread completed without hanging *and* every writer-owned page's
/// final on-disk counter matches the number of increments actually applied.
fn run_stress_workload() {
    let (pool, page_ids, fm) = setup_pool();
    let pool = Arc::new(pool);
    let page_ids = Arc::new(page_ids);

    // Per-writer-thread increment counts, keyed by page index, filled in by
    // each writer thread for the pages it exclusively owns (disjoint
    // partitions — page index `i` belongs to writer `i % NUM_WRITERS`, so
    // there is never an application-level read-modify-write race on a
    // counter even though the frames backing these pages are shared pool
    // resources under constant eviction pressure).
    let writer_counts: Vec<Vec<u32>> = std::thread::scope(|scope| {
        let mut handles = Vec::new();

        // Readers: pin a random page, read its data, unpin — the classic
        // "page-hit" path when the page happens to already be cached, or the
        // "miss + evict" path when it isn't. Readers touch the whole page
        // range (including writer-owned pages) purely for read pressure;
        // they never mutate anything, so they can't affect the counters.
        for _t in 0..NUM_READERS {
            let pool = Arc::clone(&pool);
            let page_ids = Arc::clone(&page_ids);
            handles.push(scope.spawn(move || {
                let mut rng = rand::thread_rng();
                for i in 0..ITERS_PER_THREAD {
                    let pid = page_ids[rng.gen_range(0..page_ids.len())];
                    let guard = pool.pin_page(pid).expect("pin_page failed");
                    let data = guard.data();
                    // The page header embeds its own page id in the first 8
                    // bytes (little-endian). If a frame we just pinned for
                    // `pid` was concurrently evicted-and-reused for a
                    // different page (a stale-frame/TOCTOU race distinct
                    // from the lock-order deadlock this test primarily
                    // targets), this catches it as a hard failure instead of
                    // silently returning another page's bytes.
                    let embedded_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
                    assert_eq!(
                        embedded_id, pid.0,
                        "frame for page {pid:?} actually contained page {embedded_id} — \
                         stale/reused frame handed out while pinned"
                    );
                    pool.unpin_page(pid, false).expect("unpin_page failed");
                    if i % 500 == 0 {
                        // Occasionally exercise the swizzle/is_swizzled path too.
                        let _ = pool.is_swizzled(pid);
                    }
                }
                None::<Vec<u32>>
            }));
        }

        // Writers: each owns a disjoint partition of pages
        // (`i % NUM_WRITERS == w`) and does a read-increment-write on a
        // counter embedded in its own pages only — exercises write_data
        // plus the dirty-flush path inside eviction of a frame some other
        // thread may be mid-pin on, while still giving us an exact expected
        // count per page to check after the run.
        for w in 0..NUM_WRITERS {
            let pool = Arc::clone(&pool);
            let page_ids = Arc::clone(&page_ids);
            handles.push(scope.spawn(move || {
                let mut rng = rand::thread_rng();
                let owned: Vec<usize> = (0..page_ids.len())
                    .filter(|i| i % NUM_WRITERS == w)
                    .collect();
                let mut counts = vec![0u32; page_ids.len()];
                for _ in 0..ITERS_PER_THREAD {
                    let i = owned[rng.gen_range(0..owned.len())];
                    let pid = page_ids[i];
                    let guard = pool.pin_page(pid).expect("pin_page failed");
                    let mut buf = guard.data().0;
                    let embedded_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
                    assert_eq!(
                        embedded_id, pid.0,
                        "frame for page {pid:?} actually contained page {embedded_id} — \
                         stale/reused frame handed out while pinned"
                    );
                    let counter_bytes: [u8; 4] =
                        buf[COUNTER_OFFSET..COUNTER_OFFSET + 4].try_into().unwrap();
                    let v = u32::from_le_bytes(counter_bytes).wrapping_add(1);
                    buf[COUNTER_OFFSET..COUNTER_OFFSET + 4].copy_from_slice(&v.to_le_bytes());
                    guard.write_data(&buf);
                    pool.unpin_page(pid, true).expect("unpin_page failed");
                    counts[i] += 1;
                }
                Some(counts)
            }));
        }

        handles
            .into_iter()
            .filter_map(|h| h.join().expect("worker thread panicked"))
            .collect()
    });

    pool.flush_all().expect("flush_all failed");

    // Verify every writer-owned page's on-disk counter matches the exact
    // number of increments that writer applied to it — a lost update (the
    // write-back race this stress shape originally caught, astraeadb-issues
    // .md #36 review finding B1) shows up as a mismatch here.
    let mut lost = 0usize;
    for counts in &writer_counts {
        for (i, &expected) in counts.iter().enumerate() {
            if expected == 0 {
                continue; // this writer doesn't own page i.
            }
            let pid = page_ids[i];
            let disk = fm.read_page(pid).expect("read_page failed during verification");
            let got = u32::from_le_bytes(
                disk[COUNTER_OFFSET..COUNTER_OFFSET + 4].try_into().unwrap(),
            );
            if got != expected {
                lost += 1;
                eprintln!("page {pid:?} (index {i}): expected counter {expected}, got {got}");
            }
        }
    }
    assert_eq!(lost, 0, "{lost} writer-owned pages lost updates under eviction");
}

#[test]
fn concurrent_pin_unpin_does_not_deadlock() {
    // Run the workload on a dedicated background thread so a real deadlock
    // shows up as a bounded-time test *failure* (we time out waiting on the
    // channel) rather than an indefinitely hung test binary. The background
    // thread itself is never joined if it hangs — the test process still
    // exits normally when `main` returns, abandoning it.
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("buffer-pool-stress-workload".into())
        .spawn(move || {
            run_stress_workload();
            // Ignore send errors: if the receiver already timed out and the
            // test thread moved on, there's nobody left to notify.
            let _ = tx.send(());
        })
        .expect("failed to spawn workload thread");

    match rx.recv_timeout(WATCHDOG_TIMEOUT) {
        Ok(()) => {
            // Completed without deadlocking, and (checked inside
            // `run_stress_workload`) without losing any writer updates.
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!(
                "buffer pool stress workload did not complete within {:?} — \
                 suspected deadlock (astraeadb-issues.md #36: lock-order \
                 inversion between BufferPool::pin_page's hit path and \
                 find_or_evict_frame)",
                WATCHDOG_TIMEOUT
            );
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("buffer pool stress workload thread died without completing (see stderr for panic)");
        }
    }
}

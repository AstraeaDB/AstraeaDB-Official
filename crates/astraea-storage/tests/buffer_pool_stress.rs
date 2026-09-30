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
//! on a channel with a bounded timeout, so a deadlock shows up as a test
//! **failure** within ~10s instead of a hung test binary.

use astraea_storage::buffer_pool::BufferPool;
use astraea_storage::file_manager::FileManager;
use astraea_storage::page::{PAGE_SIZE, PageType, init_page};
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

fn setup_pool() -> (BufferPool, Vec<astraea_core::types::PageId>) {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
    // Keep the backing file alive for the duration of the test process.
    let _ = tmp.into_temp_path();

    let mut page_ids = Vec::with_capacity(NUM_PAGES);
    for i in 0..NUM_PAGES {
        let buf = init_page(astraea_core::types::PageId(i as u64), PageType::NodePage);
        let pid = fm.allocate_page().unwrap();
        fm.write_page(pid, &buf).unwrap();
        page_ids.push(pid);
    }

    let pool = BufferPool::new(fm as Arc<dyn PageIO>, POOL_CAPACITY);
    (pool, page_ids)
}

/// Runs the actual concurrent-access workload. Returns normally only if every
/// spawned thread completes without hanging.
fn run_stress_workload() {
    let (pool, page_ids) = setup_pool();
    let pool = Arc::new(pool);
    let page_ids = Arc::new(page_ids);

    std::thread::scope(|scope| {
        let mut handles = Vec::new();

        // Readers: pin a random page, read its data, unpin — the classic
        // "page-hit" path when the page happens to already be cached, or the
        // "miss + evict" path (find_or_evict_frame) when it isn't.
        for t in 0..NUM_READERS {
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
                t
            }));
        }

        // Writers: pin, mutate, unpin dirty — exercises write_data plus the
        // dirty-flush path inside find_or_evict_frame's eviction of a frame
        // some other thread may be mid-pin on.
        for _ in 0..NUM_WRITERS {
            let pool = Arc::clone(&pool);
            let page_ids = Arc::clone(&page_ids);
            handles.push(scope.spawn(move || {
                let mut rng = rand::thread_rng();
                for _ in 0..ITERS_PER_THREAD {
                    let pid = page_ids[rng.gen_range(0..page_ids.len())];
                    let guard = pool.pin_page(pid).expect("pin_page failed");
                    let mut buf = [0u8; PAGE_SIZE];
                    buf.copy_from_slice(guard.data().as_ref());
                    let embedded_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
                    assert_eq!(
                        embedded_id, pid.0,
                        "frame for page {pid:?} actually contained page {embedded_id} — \
                         stale/reused frame handed out while pinned"
                    );
                    buf[PAGE_SIZE - 1] = buf[PAGE_SIZE - 1].wrapping_add(1);
                    guard.write_data(&buf);
                    pool.unpin_page(pid, true).expect("unpin_page failed");
                }
                999
            }));
        }

        for h in handles {
            h.join().expect("worker thread panicked");
        }
    });

    pool.flush_all().expect("flush_all failed");
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
            // Completed without deadlocking.
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

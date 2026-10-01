use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::PageId;
use astraea_storage::buffer_pool::BufferPool;
use astraea_storage::file_manager::FileManager;
use astraea_storage::page::PAGE_SIZE;
use astraea_storage::page_io::PageIO;
use parking_lot::{Condvar, Mutex};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)] struct GS { entered: bool, released: bool }
#[derive(Default)] struct Gate { st: Mutex<GS>, cv: Condvar }
impl Gate {
    fn pass(&self) { let mut s = self.st.lock(); s.entered = true; self.cv.notify_all(); while !s.released { self.cv.wait(&mut s); } }
    fn wait_entered(&self) { let mut s = self.st.lock(); while !s.entered { let r = self.cv.wait_for(&mut s, Duration::from_secs(5)); assert!(!r.timed_out() || s.entered, "gate never entered"); } }
    fn release(&self) { let mut s = self.st.lock(); s.released = true; self.cv.notify_all(); }
}
struct IO { fm: Arc<FileManager>, pre: Mutex<HashMap<(bool,u64), Arc<Gate>>>, post_read: Mutex<HashMap<u64, Arc<Gate>>>, fail_write: Mutex<HashSet<u64>> }
impl IO {
    fn arm(&self, w: bool, p: PageId) -> Arc<Gate> { let g = Arc::new(Gate::default()); self.pre.lock().insert((w, p.0), g.clone()); g }
    fn arm_post_read(&self, p: PageId) -> Arc<Gate> { let g = Arc::new(Gate::default()); self.post_read.lock().insert(p.0, g.clone()); g }
}
impl PageIO for IO {
    fn read_page(&self, p: PageId) -> Result<[u8; PAGE_SIZE]> {
        let pre = self.pre.lock().remove(&(false, p.0)); if let Some(g) = pre { g.pass(); }
        let r = self.fm.read_page(p);
        let post = self.post_read.lock().remove(&p.0); if let Some(g) = post { g.pass(); }
        r }
    fn write_page(&self, p: PageId, d: &[u8; PAGE_SIZE]) -> Result<()> {
        let pre = self.pre.lock().remove(&(true, p.0)); if let Some(g) = pre { g.pass(); }
        if self.fail_write.lock().remove(&p.0) { return Err(AstraeaError::Storage("injected".into())); }
        self.fm.write_page(p, d) }
    fn allocate_page(&self) -> Result<PageId> { self.fm.allocate_page() }
}
fn setup(cap: usize, n: u64) -> (Arc<BufferPool>, Arc<IO>) {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let fm = Arc::new(FileManager::new(tmp.path()).unwrap()); let _ = tmp.into_temp_path();
    for i in 0..n { let p = fm.allocate_page().unwrap(); let mut b = [0u8; PAGE_SIZE]; b[0..8].copy_from_slice(&i.to_le_bytes()); fm.write_page(p, &b).unwrap(); }
    let io = Arc::new(IO { fm, pre: Default::default(), post_read: Default::default(), fail_write: Default::default() });
    (Arc::new(BufferPool::new(io.clone() as Arc<dyn PageIO>, cap)), io)
}

/// Eviction write-back of P fails while a concurrent miss on P was served from
/// the in-flight copy and already published P -> F. complete_writeback's Err
/// path re-maps P to the victim frame, orphaning F (pinned forever) and
/// letting later writes through F's guard be lost.
#[test]
fn failed_writeback_vs_served_miss_double_maps() {
    let (pool, io) = setup(3, 5);
    let (p0, p1, p2, p3, p4) = (PageId(0), PageId(1), PageId(2), PageId(3), PageId(4));
    let g = pool.pin_page(p0).unwrap(); let mut b = g.data().0; b[100] = 0xAB; g.write_data(&b); pool.unpin_page(p0, true).unwrap();
    pool.pin_page(p2).unwrap(); pool.unpin_page(p2, false).unwrap();
    pool.pin_page(p3).unwrap(); pool.unpin_page(p3, false).unwrap();
    let gate = io.arm(true, p0); io.fail_write.lock().insert(p0.0);
    let pa = pool.clone();
    let a = std::thread::spawn(move || pa.pin_page(p1).map(|_| ()).map_err(|e| e.to_string()));
    gate.wait_entered();                       // A is writing back dirty P0; P0 is in `writeback`
    let g0 = pool.pin_page(p0).unwrap();       // served from writeback, published to another frame, marked dirty
    assert_eq!(g0.data()[100], 0xAB);
    gate.release();
    assert!(a.join().unwrap().is_err());       // A's write-back failed -> victim restored + re-mapped
    let mut nb = g0.data().0; nb[100] = 0xCD; g0.write_data(&nb);  // caller updates P0 through its live guard
    pool.unpin_page(p0, true).unwrap();
    pool.flush_all().unwrap();
    // Cycle the pool to force P0 out and back in.
    for p in [p2, p3, p4, p2, p3, p4] { match pool.pin_page(p) { Ok(_) => { pool.unpin_page(p, false).unwrap(); } Err(e) => panic!("pool lost capacity: pin {p:?} -> {e}") } }
    let seen = pool.pin_page(p0).unwrap().data()[100];
    assert_eq!(seen, 0xCD, "update made through a live guard was lost after a failed write-back");
}

/// Deterministic exercise of the page_generation retry: A's disk read of P
/// completes (stale) and then stalls; meanwhile P is published, modified,
/// evicted and written back. A must retry and return the fresh content.
#[test]
fn generation_retry_rejects_stale_read() {
    let (pool, io) = setup(2, 4);
    let (p, q, r) = (PageId(0), PageId(1), PageId(2));
    let gate = io.arm_post_read(p);
    let pa = pool.clone();
    let a = std::thread::spawn(move || { let g = pa.pin_page(p).unwrap(); let v = g.data()[100]; pa.unpin_page(p, false).unwrap(); v });
    gate.wait_entered();                       // A holds stale bytes, not yet published
    let g = pool.pin_page(p).unwrap(); let mut b = g.data().0; b[100] = 0x5A; g.write_data(&b); pool.unpin_page(p, true).unwrap();
    pool.pin_page(q).unwrap(); pool.unpin_page(q, false).unwrap(); // P evicted dirty, written back fully
    gate.release();
    assert_eq!(a.join().unwrap(), 0x5A, "stale disk read was published");
    let _ = r;
}

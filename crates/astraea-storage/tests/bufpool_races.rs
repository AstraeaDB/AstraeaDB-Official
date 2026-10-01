use astraea_core::error::Result;
use astraea_core::types::PageId;
use astraea_storage::buffer_pool::BufferPool;
use astraea_storage::file_manager::FileManager;
use astraea_storage::page::PAGE_SIZE;
use astraea_storage::page_io::PageIO;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct GateState { entered: bool, released: bool }
#[derive(Default)]
struct Gate { st: Mutex<GateState>, cv: Condvar }
impl Gate {
    fn pass(&self) { let mut s = self.st.lock(); s.entered = true; self.cv.notify_all(); while !s.released { self.cv.wait(&mut s); } }
    fn wait_entered(&self) { let mut s = self.st.lock(); while !s.entered { let r = self.cv.wait_for(&mut s, Duration::from_secs(5)); assert!(!r.timed_out() || s.entered, "gate never entered"); } }
    fn release(&self) { let mut s = self.st.lock(); s.released = true; self.cv.notify_all(); }
}

struct GatedIO { fm: Arc<FileManager>, gates: Mutex<HashMap<(bool, u64), Arc<Gate>>> }
impl GatedIO {
    fn arm(&self, write: bool, pid: PageId) -> Arc<Gate> { let g = Arc::new(Gate::default()); self.gates.lock().insert((write, pid.0), g.clone()); g }
    fn take(&self, write: bool, pid: PageId) -> Option<Arc<Gate>> { self.gates.lock().remove(&(write, pid.0)) }
}
impl PageIO for GatedIO {
    fn read_page(&self, p: PageId) -> Result<[u8; PAGE_SIZE]> { if let Some(g) = self.take(false, p) { g.pass(); } self.fm.read_page(p) }
    fn write_page(&self, p: PageId, d: &[u8; PAGE_SIZE]) -> Result<()> { if let Some(g) = self.take(true, p) { g.pass(); } self.fm.write_page(p, d) }
    fn allocate_page(&self) -> Result<PageId> { self.fm.allocate_page() }
}

fn setup(cap: usize, n: u64) -> (Arc<BufferPool>, Arc<GatedIO>) {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
    let _ = tmp.into_temp_path();
    for i in 0..n { let p = fm.allocate_page().unwrap(); let mut b = [0u8; PAGE_SIZE]; b[0..8].copy_from_slice(&i.to_le_bytes()); fm.write_page(p, &b).unwrap(); }
    let io = Arc::new(GatedIO { fm, gates: Mutex::new(HashMap::new()) });
    (Arc::new(BufferPool::new(io.clone() as Arc<dyn PageIO>, cap)), io)
}

/// A dirty page being evicted is dropped from page_table before its flush lands;
/// a concurrent miss on that page reads the stale on-disk copy.
#[test]
fn stale_read_during_dirty_eviction_flush() {
    let (pool, io) = setup(2, 3);
    let (p0, p1, p2) = (PageId(0), PageId(1), PageId(2));
    let g = pool.pin_page(p0).unwrap();
    let mut b = g.data().0; b[100] = 0xAB; g.write_data(&b);
    pool.unpin_page(p0, true).unwrap();
    pool.pin_page(p2).unwrap(); pool.unpin_page(p2, false).unwrap();
    let gate = io.arm(true, p0);
    let pool2 = pool.clone();
    let t = std::thread::spawn(move || { let _g = pool2.pin_page(p1).unwrap(); pool2.unpin_page(p1, false).unwrap(); });
    gate.wait_entered(); // evictor is now flushing P0 with no lock held
    let g0 = pool.pin_page(p0).unwrap();
    let seen = g0.data()[100];
    pool.unpin_page(p0, false).unwrap();
    gate.release(); t.join().unwrap();
    assert_eq!(seen, 0xAB, "re-pinned P0 during its eviction flush and saw stale disk bytes (lost update window)");
}

/// pin_recycled_page racing a concurrent pin_page miss on the same page id
/// (reachable from DiskStorageEngine: put_node frees+recycles a page while a
/// concurrent get_node holds the old PageId).
#[test]
fn recycled_vs_concurrent_miss_corrupts_other_page() {
    let (pool, io) = setup(2, 3);
    let (p, q, r) = (PageId(0), PageId(1), PageId(2));
    let read_gate = io.arm(false, p);
    let pr = pool.clone();
    let reader = std::thread::spawn(move || { let g = pr.pin_page(p).unwrap(); let _ = g.data(); g });
    read_gate.wait_entered(); // reader has reserved a frame, blocked in read_page(P)
    let write_gate = io.arm(true, p);
    let pw = pool.clone();
    let writer = std::thread::spawn(move || { let mut nb = [0u8; PAGE_SIZE]; nb[0..8].copy_from_slice(&0u64.to_le_bytes()); nb[200] = 0xEE; let g = pw.pin_recycled_page(p, &nb).unwrap(); (g, nb) });
    write_gate.wait_entered(); // writer reserved the other frame, blocked writing P to disk
    read_gate.release();
    let _rg = reader.join().unwrap(); // reader published P -> F_r
    write_gate.release();
    let (wg, mut nb) = writer.join().unwrap(); // writer blindly re-inserted P -> F_w
    pool.unpin_page(p, false).unwrap(); // reader's unpin lands on F_w (pin 1 -> 0) while writer still holds it
    // Another page now evicts the writer's still-held frame.
    let gq = pool.pin_page(q).unwrap();
    assert_eq!(u64::from_le_bytes(gq.data()[0..8].try_into().unwrap()), 1);
    nb[300] = 0x77; wg.write_data(&nb); // writer writes "its" page
    let qd = gq.data();
    let r_res = pool.pin_page(r);
    assert_eq!(u64::from_le_bytes(qd[0..8].try_into().unwrap()), 1,
        "writer's pin_recycled_page guard wrote into page Q's frame; also pin(R) = {:?}", r_res.as_ref().map(|_| ()).map_err(|e| e.to_string()));
}

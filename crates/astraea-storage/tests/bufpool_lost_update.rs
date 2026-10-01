use astraea_storage::buffer_pool::BufferPool;
use astraea_storage::file_manager::FileManager;
use astraea_storage::page::PAGE_SIZE;
use astraea_storage::page_io::PageIO;
use rand::Rng;
use std::sync::Arc;

#[test]
fn no_lost_updates_under_eviction() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let fm = Arc::new(FileManager::new(tmp.path()).unwrap());
    let _ = tmp.into_temp_path();
    let n = 64usize; let writers = 4usize; let iters = 5000usize;
    let mut pids = Vec::new();
    for _ in 0..n { let p = fm.allocate_page().unwrap(); fm.write_page(p, &[0u8; PAGE_SIZE]).unwrap(); pids.push(p); }
    let pool = Arc::new(BufferPool::new(fm.clone() as Arc<dyn PageIO>, 8));
    let counts: Vec<Vec<u32>> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..writers).map(|w| { let pool = pool.clone(); let pids = pids.clone(); s.spawn(move || {
            let mut rng = rand::thread_rng(); let mut c = vec![0u32; n];
            for _ in 0..iters {
                // writer w owns pages i with i % writers == w, so no app-level RMW race
                let i = rng.gen_range(0..n / writers) * writers + w;
                let g = pool.pin_page(pids[i]).unwrap();
                let mut b = g.data().0;
                let v = u32::from_le_bytes(b[0..4].try_into().unwrap()) + 1;
                b[0..4].copy_from_slice(&v.to_le_bytes());
                g.write_data(&b); pool.unpin_page(pids[i], true).unwrap(); c[i] += 1;
            }
            c })}).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    pool.flush_all().unwrap();
    let mut lost = 0;
    for i in 0..n { let exp: u32 = counts.iter().map(|c| c[i]).sum(); let got = u32::from_le_bytes(fm.read_page(pids[i]).unwrap()[0..4].try_into().unwrap()); if got != exp { lost += 1; eprintln!("page {i}: expected {exp} got {got}"); } }
    assert_eq!(lost, 0, "{lost} pages lost updates");
}

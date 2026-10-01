//! Ad-hoc measurement for astraeadb-issues.md #36: wall-clock time for five
//! concurrent vector searches vs five serialised ones, on a
//! `DiskStorageEngine`-backed `Graph` with a few thousand 128-dim
//! embeddings.
//!
//! "Vector search" here means what a real consumer's request looks like:
//! an HNSW top-k lookup *plus* fetching each hit's node record from
//! storage (`Graph::get_node`), since the HNSW index itself holds vectors
//! in memory and doesn't touch the buffer pool — the storage-level
//! contention this benchmark is meant to exercise only shows up once you
//! also touch the graph/storage layer for the results, exactly as a
//! real caller (e.g. a-llama) would.
//!
//! The buffer pool is sized deliberately small relative to the working set
//! so concurrent access forces real eviction traffic — the same shape of
//! load `tests/buffer_pool_stress.rs` uses, and the shape under which
//! astraeadb-issues.md #36 deadlocked.
//!
//! Run with: cargo run --release --example bufpool_bench -p astraea-graph

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use astraea_core::traits::GraphOps;
use astraea_core::types::DistanceMetric;
use astraea_graph::graph::Graph;
use astraea_storage::engine::DiskStorageEngine;
use astraea_vector::HnswVectorIndex;
use rand::Rng;

const DIM: usize = 128;
const NUM_NODES: usize = 3000;
// Deliberately much smaller than the ~3000-node working set (same shape as
// tests/buffer_pool_stress.rs) so both the "before" run forces real,
// frequent eviction traffic (where astraeadb-issues.md #36 deadlocked) and
// the "after" run has something real to show a concurrency win on.
const BUFFER_POOL_FRAMES: usize = 8;
const TOP_K: usize = 10;
// Each of the "5 concurrent vector searches" is actually a burst of this
// many searches on that thread, so there's enough volume to reliably
// exercise the eviction path within the watchdog window — one isolated
// search each barely touches the pool.
const SEARCHES_PER_BURST: usize = 200;
const NUM_QUERIES: usize = 5;
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(30);

fn random_vec(rng: &mut impl Rng, dim: usize) -> Vec<f32> {
    (0..dim).map(|_| rng.gen_range(-1.0f32..1.0f32)).collect()
}

fn build_graph(dir: &std::path::Path) -> Arc<Graph> {
    let engine = DiskStorageEngine::with_pool_size(dir, BUFFER_POOL_FRAMES)
        .expect("failed to open DiskStorageEngine");
    let vector_index = Arc::new(HnswVectorIndex::new(DIM, DistanceMetric::Cosine));
    let graph = Graph::with_vector_index(Box::new(engine), vector_index);

    let mut rng = rand::thread_rng();
    for i in 0..NUM_NODES {
        let embedding = random_vec(&mut rng, DIM);
        graph
            .create_node(
                vec!["Product".to_string()],
                serde_json::json!({ "seq": i }),
                Some(embedding),
            )
            .expect("create_node failed");
    }

    Arc::new(graph)
}

/// One "vector search": HNSW top-k, then fetch every hit's node record from
/// storage (through the buffer pool).
fn do_search(graph: &Graph, query: &[f32]) {
    let vi = graph.vector_index().expect("vector index attached");
    let hits = vi.search(query, TOP_K).expect("vector search failed");
    for hit in hits {
        let node = graph.get_node(hit.node_id).expect("get_node failed");
        assert!(
            node.is_some(),
            "vector index returned node {:?} with no backing storage record",
            hit.node_id
        );
    }
}

fn main() {
    let tmp = tempfile::tempdir().expect("tempdir");
    println!(
        "Building graph: {NUM_NODES} nodes, dim={DIM}, buffer pool={BUFFER_POOL_FRAMES} frames, dir={:?}",
        tmp.path()
    );
    let graph = build_graph(tmp.path());
    println!("Graph built. vector index len = {}", graph.vector_index().unwrap().len());

    // Each "query slot" is a burst of `SEARCHES_PER_BURST` searches with
    // fresh random query vectors, so there's real repeated pool pressure
    // per thread/serial-slot, not just one search each.
    let mut rng = rand::thread_rng();
    let bursts: Vec<Vec<Vec<f32>>> = (0..NUM_QUERIES)
        .map(|_| (0..SEARCHES_PER_BURST).map(|_| random_vec(&mut rng, DIM)).collect())
        .collect();

    // --- Serialised: run the 5 bursts one after another on this thread. ---
    let start = Instant::now();
    for burst in &bursts {
        for q in burst {
            do_search(&graph, q);
        }
    }
    let serial_elapsed = start.elapsed();
    println!(
        "Serialised (5 x {SEARCHES_PER_BURST} sequential searches): {serial_elapsed:?}"
    );

    // --- Concurrent: run the 5 bursts on 5 threads at once, with a
    // watchdog so a deadlock (astraeadb-issues.md #36) is reported instead
    // of hanging forever. ---
    let (tx, rx) = mpsc::channel();
    {
        let graph = Arc::clone(&graph);
        let bursts = bursts.clone();
        std::thread::Builder::new()
            .name("bufpool-bench-concurrent".into())
            .spawn(move || {
                let start = Instant::now();
                std::thread::scope(|scope| {
                    let mut handles = Vec::new();
                    for burst in &bursts {
                        let graph = Arc::clone(&graph);
                        handles.push(scope.spawn(move || {
                            for q in burst {
                                do_search(&graph, q);
                            }
                        }));
                    }
                    for h in handles {
                        h.join().expect("search thread panicked");
                    }
                });
                let _ = tx.send(start.elapsed());
            })
            .expect("failed to spawn concurrent-benchmark thread");
    }

    match rx.recv_timeout(WATCHDOG_TIMEOUT) {
        Ok(elapsed) => println!("Concurrent (5 threads x {SEARCHES_PER_BURST} each): {elapsed:?}"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            println!(
                "Concurrent (5 threads x {SEARCHES_PER_BURST} each): TIMED OUT after {WATCHDOG_TIMEOUT:?} — \
                 suspected deadlock (astraeadb-issues.md #36)"
            );
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            println!("Concurrent (5 threads x {SEARCHES_PER_BURST} each): benchmark thread died (see stderr)");
        }
    }
}

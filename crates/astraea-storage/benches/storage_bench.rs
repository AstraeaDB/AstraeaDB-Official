use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use rand::Rng;
use tempfile::TempDir;

use astraea_core::traits::StorageEngine;
use astraea_core::types::*;
use astraea_storage::engine::DiskStorageEngine;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a test node with the given ID.
fn make_node(id: u64) -> Node {
    Node {
        id: NodeId(id),
        labels: vec!["Person".to_string(), "Entity".to_string()],
        properties: serde_json::json!({
            "name": format!("node_{}", id),
            "age": (id % 100) as u32,
            "active": true,
        }),
        embedding: Some(vec![0.1; 16]),
    }
}

/// Create a test edge between two nodes.
fn make_edge(id: u64, src: u64, tgt: u64) -> Edge {
    Edge {
        id: EdgeId(id),
        source: NodeId(src),
        target: NodeId(tgt),
        edge_type: "KNOWS".to_string(),
        properties: serde_json::json!({"since": 2024}),
        weight: 1.0,
        validity: ValidityInterval::always(),
    }
}

/// Create a fresh engine backed by a temp directory.
fn fresh_engine() -> (DiskStorageEngine, TempDir) {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let engine = DiskStorageEngine::with_pool_size(tmp.path(), 256).expect("failed to create engine");
    (engine, tmp)
}

/// Pre-populate an engine with `count` nodes (IDs 1..=count).
fn prepopulate_nodes(engine: &DiskStorageEngine, count: u64) {
    for i in 1..=count {
        engine.put_node(&make_node(i)).expect("put_node failed");
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn storage_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage");

    // -- bench_put_node: write a single node --
    group.bench_function("put_node", |b| {
        let (engine, _tmp) = fresh_engine();
        let mut id_counter = 1u64;
        b.iter(|| {
            let node = make_node(id_counter);
            engine.put_node(black_box(&node)).unwrap();
            id_counter += 1;
        });
    });

    // -- bench_get_node: read a single node by random ID --
    group.bench_function("get_node", |b| {
        let (engine, _tmp) = fresh_engine();
        prepopulate_nodes(&engine, 1000);
        let mut rng = rand::thread_rng();
        b.iter(|| {
            let id = NodeId(rng.gen_range(1..=1000));
            black_box(engine.get_node(id).unwrap());
        });
    });

    // -- bench_sequential_writes: write 1000 nodes sequentially --
    group.bench_function("sequential_writes_1000", |b| {
        b.iter_batched(
            || fresh_engine(),
            |(engine, _tmp)| {
                for i in 1..=1000u64 {
                    engine.put_node(black_box(&make_node(i))).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    // -- bench_random_reads: read random nodes from a 1000-node dataset --
    group.bench_function("random_reads_100", |b| {
        let (engine, _tmp) = fresh_engine();
        prepopulate_nodes(&engine, 1000);
        let mut rng = rand::thread_rng();
        b.iter(|| {
            for _ in 0..100 {
                let id = NodeId(rng.gen_range(1..=1000));
                black_box(engine.get_node(id).unwrap());
            }
        });
    });

    // -- bench_put_edge: write a single edge --
    group.bench_function("put_edge", |b| {
        let (engine, _tmp) = fresh_engine();
        let mut edge_counter = 1u64;
        b.iter(|| {
            let edge = make_edge(edge_counter, 1, 2);
            engine.put_edge(black_box(&edge)).unwrap();
            edge_counter += 1;
        });
    });

    // -- bench_get_edges_by_direction: get outgoing edges for a node with 10 edges --
    group.bench_function("get_edges_outgoing_10", |b| {
        let (engine, _tmp) = fresh_engine();
        // Create source node and 10 target nodes, then 10 outgoing edges.
        let source_id = 1u64;
        for i in source_id..=11 {
            engine.put_node(&make_node(i)).unwrap();
        }
        for i in 0..10u64 {
            let edge = make_edge(i + 1, source_id, source_id + 1 + i);
            engine.put_edge(&edge).unwrap();
        }
        b.iter(|| {
            black_box(
                engine
                    .get_edges(NodeId(source_id), Direction::Outgoing)
                    .unwrap(),
            );
        });
    });

    group.finish();
}

criterion_group!(benches, storage_benchmarks);
criterion_main!(benches);

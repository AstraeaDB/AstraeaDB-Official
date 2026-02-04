use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::Rng;

use astraea_core::types::{DistanceMetric, NodeId};
use astraea_vector::distance::{cosine_distance, euclidean_distance};
use astraea_vector::hnsw::HnswIndex;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const DIM: usize = 128;
const INDEX_SIZE: usize = 1000;

/// Generate a random f32 vector of the given dimension.
fn random_vector(rng: &mut impl Rng, dim: usize) -> Vec<f32> {
    (0..dim).map(|_| rng.r#gen::<f32>()).collect()
}

/// Build an HNSW index pre-populated with `count` random vectors of dimension `dim`.
fn build_index(dim: usize, count: usize) -> HnswIndex {
    let mut rng = rand::thread_rng();
    let mut index = HnswIndex::new(dim, DistanceMetric::Euclidean, 16, 200);
    for i in 0..count {
        let vec = random_vector(&mut rng, dim);
        index.insert(NodeId(i as u64), &vec).unwrap();
    }
    index
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn vector_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector");

    // -- bench_hnsw_insert: insert a single vector into a 1000-vector index --
    group.bench_function("hnsw_insert", |b| {
        let mut rng = rand::thread_rng();
        let mut index = build_index(DIM, INDEX_SIZE);
        let mut next_id = INDEX_SIZE as u64;
        b.iter(|| {
            let vec = random_vector(&mut rng, DIM);
            index.insert(NodeId(next_id), black_box(&vec)).unwrap();
            next_id += 1;
        });
    });

    // -- bench_hnsw_search_k10: k-NN search (k=10) on a 1000-vector index --
    group.bench_function("hnsw_search_k10", |b| {
        let index = build_index(DIM, INDEX_SIZE);
        let mut rng = rand::thread_rng();
        b.iter(|| {
            let query = random_vector(&mut rng, DIM);
            black_box(index.search(black_box(&query), 10, 50).unwrap());
        });
    });

    // -- bench_hnsw_search_k50: k-NN search (k=50) on a 1000-vector index --
    group.bench_function("hnsw_search_k50", |b| {
        let index = build_index(DIM, INDEX_SIZE);
        let mut rng = rand::thread_rng();
        b.iter(|| {
            let query = random_vector(&mut rng, DIM);
            black_box(index.search(black_box(&query), 50, 100).unwrap());
        });
    });

    // -- bench_cosine_distance: raw cosine distance computation (dim=128) --
    group.bench_function("cosine_distance_128", |b| {
        let mut rng = rand::thread_rng();
        let a = random_vector(&mut rng, DIM);
        let b_vec = random_vector(&mut rng, DIM);
        b.iter(|| {
            black_box(cosine_distance(black_box(&a), black_box(&b_vec)).unwrap());
        });
    });

    // -- bench_euclidean_distance: raw euclidean distance computation (dim=128) --
    group.bench_function("euclidean_distance_128", |b| {
        let mut rng = rand::thread_rng();
        let a = random_vector(&mut rng, DIM);
        let b_vec = random_vector(&mut rng, DIM);
        b.iter(|| {
            black_box(euclidean_distance(black_box(&a), black_box(&b_vec)).unwrap());
        });
    });

    group.finish();
}

criterion_group!(benches, vector_benchmarks);
criterion_main!(benches);

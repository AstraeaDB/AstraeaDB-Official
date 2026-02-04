use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::Rng;

use astraea_core::traits::{GraphOps, StorageEngine};
use astraea_core::types::*;
use astraea_graph::test_utils::InMemoryStorage;
use astraea_graph::Graph;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const NODE_COUNT: u64 = 1000;
const AVG_DEGREE: u64 = 5;

/// Build a random graph with `node_count` nodes and roughly `node_count * avg_degree`
/// random directed edges with random weights in [0.1, 10.0].
/// Returns the Graph and a list of all NodeIds.
fn build_random_graph(node_count: u64, avg_degree: u64) -> (Graph, Vec<NodeId>) {
    let storage = InMemoryStorage::new();
    let mut rng = rand::thread_rng();

    // Create nodes (IDs will be assigned by put_node directly).
    let mut node_ids = Vec::with_capacity(node_count as usize);
    for i in 1..=node_count {
        let node = Node {
            id: NodeId(i),
            labels: vec!["TestNode".to_string()],
            properties: serde_json::json!({"idx": i}),
            embedding: None,
        };
        storage.put_node(&node).unwrap();
        node_ids.push(NodeId(i));
    }

    // Create random edges.
    let edge_count = node_count * avg_degree;
    for e in 1..=edge_count {
        let src = rng.gen_range(1..=node_count);
        let mut tgt = rng.gen_range(1..=node_count);
        // Avoid self-loops.
        while tgt == src {
            tgt = rng.gen_range(1..=node_count);
        }
        let weight: f64 = rng.gen_range(0.1..10.0);
        let edge = Edge {
            id: EdgeId(e),
            source: NodeId(src),
            target: NodeId(tgt),
            edge_type: "LINK".to_string(),
            properties: serde_json::json!({}),
            weight,
            validity: ValidityInterval::always(),
        };
        storage.put_edge(&edge).unwrap();
    }

    // Build the Graph with starting IDs beyond what we used.
    let graph = Graph::with_start_ids(
        Box::new(storage),
        node_count + 1,
        edge_count + 1,
    );
    (graph, node_ids)
}

/// Build a graph where a specific node has exactly `degree` outgoing connections.
fn build_graph_with_hub(hub_degree: u64) -> (Graph, NodeId) {
    let storage = InMemoryStorage::new();

    // Hub node.
    let hub = Node {
        id: NodeId(1),
        labels: vec!["Hub".to_string()],
        properties: serde_json::json!({"role": "hub"}),
        embedding: None,
    };
    storage.put_node(&hub).unwrap();

    // Neighbor nodes and edges.
    for i in 1..=hub_degree {
        let neighbor = Node {
            id: NodeId(i + 1),
            labels: vec!["Neighbor".to_string()],
            properties: serde_json::json!({}),
            embedding: None,
        };
        storage.put_node(&neighbor).unwrap();

        let edge = Edge {
            id: EdgeId(i),
            source: NodeId(1),
            target: NodeId(i + 1),
            edge_type: "CONNECTED".to_string(),
            properties: serde_json::json!({}),
            weight: 1.0,
            validity: ValidityInterval::always(),
        };
        storage.put_edge(&edge).unwrap();
    }

    let graph = Graph::with_start_ids(
        Box::new(storage),
        hub_degree + 2,
        hub_degree + 1,
    );
    (graph, NodeId(1))
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn graph_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph");

    // -- bench_bfs: BFS from a node in a 1000-node random graph, depth 3 --
    group.bench_function("bfs_depth3", |b| {
        let (graph, node_ids) = build_random_graph(NODE_COUNT, AVG_DEGREE);
        let start = node_ids[0];
        b.iter(|| {
            black_box(graph.bfs(black_box(start), 3).unwrap());
        });
    });

    // -- bench_shortest_path: unweighted shortest path between two nodes --
    group.bench_function("shortest_path_unweighted", |b| {
        let (graph, node_ids) = build_random_graph(NODE_COUNT, AVG_DEGREE);
        let from = node_ids[0];
        let to = node_ids[node_ids.len() - 1];
        b.iter(|| {
            black_box(graph.shortest_path(black_box(from), black_box(to)).unwrap());
        });
    });

    // -- bench_dijkstra: weighted shortest path in a 1000-node graph --
    group.bench_function("dijkstra", |b| {
        let (graph, node_ids) = build_random_graph(NODE_COUNT, AVG_DEGREE);
        let from = node_ids[0];
        let to = node_ids[node_ids.len() - 1];
        b.iter(|| {
            black_box(
                graph
                    .shortest_path_weighted(black_box(from), black_box(to))
                    .unwrap(),
            );
        });
    });

    // -- bench_neighbors: get neighbors of a node with 20 connections --
    group.bench_function("neighbors_20", |b| {
        let (graph, hub_id) = build_graph_with_hub(20);
        b.iter(|| {
            black_box(
                graph
                    .neighbors(black_box(hub_id), Direction::Outgoing)
                    .unwrap(),
            );
        });
    });

    // -- bench_create_node: create a single node --
    group.bench_function("create_node", |b| {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        b.iter(|| {
            black_box(
                graph
                    .create_node(
                        vec!["Bench".to_string()],
                        serde_json::json!({"key": "value"}),
                        None,
                    )
                    .unwrap(),
            );
        });
    });

    group.finish();
}

criterion_group!(benches, graph_benchmarks);
criterion_main!(benches);

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use astraea_core::error::Result;
use astraea_core::traits::StorageEngine;
use astraea_core::types::*;

/// Breadth-first search from `start` up to `max_depth`.
/// Returns all discovered nodes with their depth from start.
pub fn bfs(
    storage: &dyn StorageEngine,
    start: NodeId,
    max_depth: usize,
) -> Result<Vec<(NodeId, usize)>> {
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut result: Vec<(NodeId, usize)> = Vec::new();
    let mut queue: VecDeque<(NodeId, usize)> = VecDeque::new();

    visited.insert(start);
    result.push((start, 0));
    queue.push_back((start, 0));

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        let edges = storage.get_edges(current, Direction::Outgoing)?;
        for edge in edges {
            let neighbor = edge.target;
            if visited.insert(neighbor) {
                let next_depth = depth + 1;
                result.push((neighbor, next_depth));
                queue.push_back((neighbor, next_depth));
            }
        }
    }

    Ok(result)
}

/// Depth-first search from `start` up to `max_depth`.
/// Returns all discovered nodes in DFS order.
pub fn dfs(
    storage: &dyn StorageEngine,
    start: NodeId,
    max_depth: usize,
) -> Result<Vec<NodeId>> {
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut result: Vec<NodeId> = Vec::new();
    let mut stack: Vec<(NodeId, usize)> = vec![(start, 0)];

    while let Some((current, depth)) = stack.pop() {
        if !visited.insert(current) {
            continue;
        }
        result.push(current);

        if depth >= max_depth {
            continue;
        }

        let edges = storage.get_edges(current, Direction::Outgoing)?;
        // Push in reverse order so first neighbor is visited first.
        for edge in edges.into_iter().rev() {
            if !visited.contains(&edge.target) {
                stack.push((edge.target, depth + 1));
            }
        }
    }

    Ok(result)
}

/// Unweighted shortest path using BFS.
/// Returns None if no path exists.
pub fn shortest_path_unweighted(
    storage: &dyn StorageEngine,
    from: NodeId,
    to: NodeId,
) -> Result<Option<GraphPath>> {
    if from == to {
        return Ok(Some(GraphPath::new(from)));
    }

    let mut visited: HashSet<NodeId> = HashSet::new();
    // parent map: node -> (edge used, parent node)
    let mut parent: HashMap<NodeId, (EdgeId, NodeId)> = HashMap::new();
    let mut queue: VecDeque<NodeId> = VecDeque::new();

    visited.insert(from);
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        let edges = storage.get_edges(current, Direction::Outgoing)?;
        for edge in edges {
            let neighbor = edge.target;
            if visited.insert(neighbor) {
                parent.insert(neighbor, (edge.id, current));
                if neighbor == to {
                    return Ok(Some(reconstruct_path(from, to, &parent)));
                }
                queue.push_back(neighbor);
            }
        }
    }

    Ok(None)
}

/// Weighted shortest path using Dijkstra's algorithm.
/// Returns None if no path exists. Edge weights must be non-negative.
pub fn shortest_path_dijkstra(
    storage: &dyn StorageEngine,
    from: NodeId,
    to: NodeId,
) -> Result<Option<(GraphPath, f64)>> {
    if from == to {
        return Ok(Some((GraphPath::new(from), 0.0)));
    }

    let mut dist: HashMap<NodeId, f64> = HashMap::new();
    let mut parent: HashMap<NodeId, (EdgeId, NodeId)> = HashMap::new();
    let mut heap: BinaryHeap<DijkstraEntry> = BinaryHeap::new();

    dist.insert(from, 0.0);
    heap.push(DijkstraEntry {
        node: from,
        distance: 0.0,
    });

    while let Some(DijkstraEntry { node, distance }) = heap.pop() {
        if node == to {
            let path = reconstruct_path(from, to, &parent);
            return Ok(Some((path, distance)));
        }

        // Skip if we already found a shorter path to this node.
        if let Some(&best) = dist.get(&node) {
            if distance > best {
                continue;
            }
        }

        let edges = storage.get_edges(node, Direction::Outgoing)?;
        for edge in edges {
            let new_dist = distance + edge.weight;
            let neighbor = edge.target;
            let is_shorter = dist.get(&neighbor).is_none_or(|&d| new_dist < d);

            if is_shorter {
                dist.insert(neighbor, new_dist);
                parent.insert(neighbor, (edge.id, node));
                heap.push(DijkstraEntry {
                    node: neighbor,
                    distance: new_dist,
                });
            }
        }
    }

    Ok(None)
}

/// Reconstruct a path from the parent map built during BFS/Dijkstra.
fn reconstruct_path(
    from: NodeId,
    to: NodeId,
    parent: &HashMap<NodeId, (EdgeId, NodeId)>,
) -> GraphPath {
    let mut steps: Vec<(EdgeId, NodeId)> = Vec::new();
    let mut current = to;

    while current != from {
        let (edge_id, prev) = parent[&current];
        steps.push((edge_id, current));
        current = prev;
    }

    steps.reverse();
    let mut path = GraphPath::new(from);
    for (edge_id, node) in steps {
        path.push(edge_id, node);
    }
    path
}

// ---- Temporal traversals ----

/// Breadth-first search from `start` up to `max_depth`, only traversing edges
/// whose validity interval contains `timestamp`.
pub fn bfs_at(
    storage: &dyn StorageEngine,
    start: NodeId,
    max_depth: usize,
    timestamp: i64,
) -> Result<Vec<(NodeId, usize)>> {
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut result: Vec<(NodeId, usize)> = Vec::new();
    let mut queue: VecDeque<(NodeId, usize)> = VecDeque::new();

    visited.insert(start);
    result.push((start, 0));
    queue.push_back((start, 0));

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        let edges = storage.get_edges(current, Direction::Outgoing)?;
        for edge in edges {
            if !edge.validity.contains(timestamp) {
                continue;
            }
            let neighbor = edge.target;
            if visited.insert(neighbor) {
                let next_depth = depth + 1;
                result.push((neighbor, next_depth));
                queue.push_back((neighbor, next_depth));
            }
        }
    }

    Ok(result)
}

/// Unweighted shortest path using BFS, only traversing edges valid at `timestamp`.
pub fn shortest_path_unweighted_at(
    storage: &dyn StorageEngine,
    from: NodeId,
    to: NodeId,
    timestamp: i64,
) -> Result<Option<GraphPath>> {
    if from == to {
        return Ok(Some(GraphPath::new(from)));
    }

    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut parent: HashMap<NodeId, (EdgeId, NodeId)> = HashMap::new();
    let mut queue: VecDeque<NodeId> = VecDeque::new();

    visited.insert(from);
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        let edges = storage.get_edges(current, Direction::Outgoing)?;
        for edge in edges {
            if !edge.validity.contains(timestamp) {
                continue;
            }
            let neighbor = edge.target;
            if visited.insert(neighbor) {
                parent.insert(neighbor, (edge.id, current));
                if neighbor == to {
                    return Ok(Some(reconstruct_path(from, to, &parent)));
                }
                queue.push_back(neighbor);
            }
        }
    }

    Ok(None)
}

/// Weighted shortest path using Dijkstra's algorithm, only traversing edges valid at `timestamp`.
pub fn shortest_path_dijkstra_at(
    storage: &dyn StorageEngine,
    from: NodeId,
    to: NodeId,
    timestamp: i64,
) -> Result<Option<(GraphPath, f64)>> {
    if from == to {
        return Ok(Some((GraphPath::new(from), 0.0)));
    }

    let mut dist: HashMap<NodeId, f64> = HashMap::new();
    let mut parent: HashMap<NodeId, (EdgeId, NodeId)> = HashMap::new();
    let mut heap: BinaryHeap<DijkstraEntry> = BinaryHeap::new();

    dist.insert(from, 0.0);
    heap.push(DijkstraEntry {
        node: from,
        distance: 0.0,
    });

    while let Some(DijkstraEntry { node, distance }) = heap.pop() {
        if node == to {
            let path = reconstruct_path(from, to, &parent);
            return Ok(Some((path, distance)));
        }

        if let Some(&best) = dist.get(&node) {
            if distance > best {
                continue;
            }
        }

        let edges = storage.get_edges(node, Direction::Outgoing)?;
        for edge in edges {
            if !edge.validity.contains(timestamp) {
                continue;
            }
            let new_dist = distance + edge.weight;
            let neighbor = edge.target;
            let is_shorter = dist.get(&neighbor).is_none_or(|&d| new_dist < d);

            if is_shorter {
                dist.insert(neighbor, new_dist);
                parent.insert(neighbor, (edge.id, node));
                heap.push(DijkstraEntry {
                    node: neighbor,
                    distance: new_dist,
                });
            }
        }
    }

    Ok(None)
}

/// Entry for Dijkstra's priority queue. Lower distance = higher priority.
#[derive(Debug)]
struct DijkstraEntry {
    node: NodeId,
    distance: f64,
}

impl PartialEq for DijkstraEntry {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance && self.node == other.node
    }
}

impl Eq for DijkstraEntry {}

impl PartialOrd for DijkstraEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DijkstraEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap behavior.
        other
            .distance
            .partial_cmp(&self.distance)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.node.cmp(&other.node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::InMemoryStorage;

    /// Build a simple graph: 1 -> 2 -> 3 -> 4, 1 -> 3
    fn build_test_graph() -> Box<dyn StorageEngine> {
        let storage = InMemoryStorage::new();
        for i in 1..=4 {
            storage
                .put_node(&Node {
                    id: NodeId(i),
                    labels: vec![],
                    properties: serde_json::json!({}),
                    embedding: None,
                })
                .unwrap();
        }
        let edges = [
            (1, 2, 1.0),
            (2, 3, 2.0),
            (3, 4, 1.0),
            (1, 3, 5.0), // direct but expensive
        ];
        for (i, &(src, tgt, w)) in edges.iter().enumerate() {
            storage
                .put_edge(&Edge {
                    id: EdgeId(i as u64 + 1),
                    source: NodeId(src),
                    target: NodeId(tgt),
                    edge_type: "LINK".into(),
                    properties: serde_json::json!({}),
                    weight: w,
                    validity: ValidityInterval::always(),
                })
                .unwrap();
        }
        Box::new(storage)
    }

    #[test]
    fn bfs_depth_0() {
        let storage = build_test_graph();
        let result = bfs(storage.as_ref(), NodeId(1), 0).unwrap();
        assert_eq!(result, vec![(NodeId(1), 0)]);
    }

    #[test]
    fn bfs_depth_1() {
        let storage = build_test_graph();
        let result = bfs(storage.as_ref(), NodeId(1), 1).unwrap();
        let nodes: Vec<NodeId> = result.iter().map(|(n, _)| *n).collect();
        assert!(nodes.contains(&NodeId(1)));
        assert!(nodes.contains(&NodeId(2)));
        assert!(nodes.contains(&NodeId(3)));
        assert!(!nodes.contains(&NodeId(4)));
    }

    #[test]
    fn bfs_full() {
        let storage = build_test_graph();
        let result = bfs(storage.as_ref(), NodeId(1), 10).unwrap();
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn dfs_all_nodes() {
        let storage = build_test_graph();
        let result = dfs(storage.as_ref(), NodeId(1), 10).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], NodeId(1)); // start node is first
    }

    #[test]
    fn shortest_path_unweighted_direct() {
        let storage = build_test_graph();
        let path = shortest_path_unweighted(storage.as_ref(), NodeId(1), NodeId(3))
            .unwrap()
            .unwrap();
        // BFS finds 1->3 (1 hop) over 1->2->3 (2 hops)
        assert_eq!(path.len(), 1);
        assert_eq!(path.end(), NodeId(3));
    }

    #[test]
    fn shortest_path_unweighted_multi_hop() {
        let storage = build_test_graph();
        let path = shortest_path_unweighted(storage.as_ref(), NodeId(1), NodeId(4))
            .unwrap()
            .unwrap();
        // 1->2->3->4 or 1->3->4 — both are 2 hops via BFS
        assert!(path.len() <= 3);
        assert_eq!(path.end(), NodeId(4));
    }

    #[test]
    fn shortest_path_unweighted_same_node() {
        let storage = build_test_graph();
        let path = shortest_path_unweighted(storage.as_ref(), NodeId(1), NodeId(1))
            .unwrap()
            .unwrap();
        assert_eq!(path.len(), 0);
        assert_eq!(path.end(), NodeId(1));
    }

    #[test]
    fn shortest_path_unweighted_no_path() {
        let storage = build_test_graph();
        // Node 4 has no outgoing edges, so no path from 4 to 1
        let result =
            shortest_path_unweighted(storage.as_ref(), NodeId(4), NodeId(1)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn dijkstra_prefers_cheaper_path() {
        let storage = build_test_graph();
        // 1->2 (w=1) -> 3 (w=2) = total 3.0
        // 1->3 (w=5) = total 5.0
        // Dijkstra should pick 1->2->3
        let (path, cost) = shortest_path_dijkstra(storage.as_ref(), NodeId(1), NodeId(3))
            .unwrap()
            .unwrap();
        assert_eq!(cost, 3.0);
        assert_eq!(path.len(), 2);
    }

    #[test]
    fn dijkstra_to_node_4() {
        let storage = build_test_graph();
        // 1->2 (1) ->3 (2) ->4 (1) = 4.0
        // 1->3 (5) ->4 (1) = 6.0
        let (_, cost) = shortest_path_dijkstra(storage.as_ref(), NodeId(1), NodeId(4))
            .unwrap()
            .unwrap();
        assert_eq!(cost, 4.0);
    }

    #[test]
    fn dijkstra_no_path() {
        let storage = build_test_graph();
        let result =
            shortest_path_dijkstra(storage.as_ref(), NodeId(4), NodeId(1)).unwrap();
        assert!(result.is_none());
    }

    // ---- Temporal traversal tests ----

    /// Build a graph with temporal edges:
    /// Node 1 -> Node 2 (valid 100..200)
    /// Node 2 -> Node 3 (valid 150..300)
    /// Node 1 -> Node 3 (valid 50..120, weight 5.0)
    fn build_temporal_graph() -> Box<dyn StorageEngine> {
        let storage = InMemoryStorage::new();
        for i in 1..=3 {
            storage
                .put_node(&Node {
                    id: NodeId(i),
                    labels: vec![],
                    properties: serde_json::json!({}),
                    embedding: None,
                })
                .unwrap();
        }
        // Edge 1->2: valid from t=100 to t=200
        storage
            .put_edge(&Edge {
                id: EdgeId(1),
                source: NodeId(1),
                target: NodeId(2),
                edge_type: "LINK".into(),
                properties: serde_json::json!({}),
                weight: 1.0,
                validity: ValidityInterval {
                    valid_from: Some(100),
                    valid_to: Some(200),
                },
            })
            .unwrap();
        // Edge 2->3: valid from t=150 to t=300
        storage
            .put_edge(&Edge {
                id: EdgeId(2),
                source: NodeId(2),
                target: NodeId(3),
                edge_type: "LINK".into(),
                properties: serde_json::json!({}),
                weight: 2.0,
                validity: ValidityInterval {
                    valid_from: Some(150),
                    valid_to: Some(300),
                },
            })
            .unwrap();
        // Edge 1->3: valid from t=50 to t=120 (direct but expensive and only early)
        storage
            .put_edge(&Edge {
                id: EdgeId(3),
                source: NodeId(1),
                target: NodeId(3),
                edge_type: "LINK".into(),
                properties: serde_json::json!({}),
                weight: 5.0,
                validity: ValidityInterval {
                    valid_from: Some(50),
                    valid_to: Some(120),
                },
            })
            .unwrap();
        Box::new(storage)
    }

    #[test]
    fn bfs_at_before_any_edges() {
        let storage = build_temporal_graph();
        // At t=10, no edges are valid.
        let result = bfs_at(storage.as_ref(), NodeId(1), 10, 10).unwrap();
        assert_eq!(result, vec![(NodeId(1), 0)]);
    }

    #[test]
    fn bfs_at_early_window() {
        let storage = build_temporal_graph();
        // At t=100: edge 1->2 is valid (100..200), edge 1->3 is valid (50..120)
        let result = bfs_at(storage.as_ref(), NodeId(1), 10, 100).unwrap();
        let nodes: Vec<NodeId> = result.iter().map(|(n, _)| *n).collect();
        assert!(nodes.contains(&NodeId(1)));
        assert!(nodes.contains(&NodeId(2)));
        assert!(nodes.contains(&NodeId(3))); // reachable via 1->3 (direct)
    }

    #[test]
    fn bfs_at_middle_window() {
        let storage = build_temporal_graph();
        // At t=160: edge 1->2 valid, edge 2->3 valid, edge 1->3 NOT valid (expired at 120)
        let result = bfs_at(storage.as_ref(), NodeId(1), 10, 160).unwrap();
        let nodes: Vec<NodeId> = result.iter().map(|(n, _)| *n).collect();
        assert!(nodes.contains(&NodeId(1)));
        assert!(nodes.contains(&NodeId(2)));
        assert!(nodes.contains(&NodeId(3))); // reachable via 1->2->3
    }

    #[test]
    fn bfs_at_late_window() {
        let storage = build_temporal_graph();
        // At t=250: only edge 2->3 is valid (150..300). Edge 1->2 expired.
        let result = bfs_at(storage.as_ref(), NodeId(1), 10, 250).unwrap();
        // Node 1 has no valid outgoing edges at this time
        assert_eq!(result, vec![(NodeId(1), 0)]);
    }

    #[test]
    fn shortest_path_at_uses_temporal_edges() {
        let storage = build_temporal_graph();
        // At t=100: both 1->2 and 1->3 are valid, so direct path 1->3 exists
        let path = shortest_path_unweighted_at(storage.as_ref(), NodeId(1), NodeId(3), 100)
            .unwrap()
            .unwrap();
        assert_eq!(path.len(), 1); // direct hop

        // At t=160: only 1->2 and 2->3 are valid, so path is 1->2->3
        let path = shortest_path_unweighted_at(storage.as_ref(), NodeId(1), NodeId(3), 160)
            .unwrap()
            .unwrap();
        assert_eq!(path.len(), 2); // two hops
    }

    #[test]
    fn shortest_path_at_no_path() {
        let storage = build_temporal_graph();
        // At t=250: no valid outgoing edges from node 1
        let result = shortest_path_unweighted_at(storage.as_ref(), NodeId(1), NodeId(3), 250)
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn dijkstra_at_temporal() {
        let storage = build_temporal_graph();
        // At t=160: edges 1->2 (w=1) and 2->3 (w=2) are valid. Edge 1->3 (w=5) is expired.
        // Only path: 1->2->3 with cost 3.0
        let (path, cost) =
            shortest_path_dijkstra_at(storage.as_ref(), NodeId(1), NodeId(3), 160)
                .unwrap()
                .unwrap();
        assert_eq!(cost, 3.0);
        assert_eq!(path.len(), 2);

        // At t=100: both 1->2->3 (cost 3.0) and 1->3 (cost 5.0) available
        // But 2->3 requires t>=150, so at t=100 only 1->3 (direct) is valid for reaching 3
        let (_, cost) =
            shortest_path_dijkstra_at(storage.as_ref(), NodeId(1), NodeId(3), 100)
                .unwrap()
                .unwrap();
        assert_eq!(cost, 5.0); // only the direct expensive edge is fully valid
    }
}

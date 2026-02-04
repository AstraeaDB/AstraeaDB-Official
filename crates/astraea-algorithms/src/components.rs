use std::collections::{HashMap, HashSet, VecDeque};

use astraea_core::error::Result;
use astraea_core::traits::GraphOps;
use astraea_core::types::*;

/// Find all connected components in the graph, treating edges as undirected.
///
/// Uses BFS from each unvisited node with [`Direction::Both`] to discover
/// every weakly-connected component.
///
/// # Arguments
/// * `graph` - A graph supporting neighbor lookups.
/// * `nodes` - The complete set of node IDs to partition.
///
/// # Returns
/// A vector of components, each being a `Vec<NodeId>`.  Every node in
/// `nodes` appears in exactly one component.
pub fn connected_components(
    graph: &dyn GraphOps,
    nodes: &[NodeId],
) -> Result<Vec<Vec<NodeId>>> {
    let mut visited: HashSet<NodeId> = HashSet::with_capacity(nodes.len());
    let node_set: HashSet<NodeId> = nodes.iter().copied().collect();
    let mut components: Vec<Vec<NodeId>> = Vec::new();

    for &start in nodes {
        if visited.contains(&start) {
            continue;
        }

        let mut component = Vec::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            component.push(current);
            let neighbors = graph.neighbors(current, Direction::Both)?;
            for (_edge_id, neighbor) in neighbors {
                if node_set.contains(&neighbor) && visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }

        components.push(component);
    }

    Ok(components)
}

/// Find all strongly connected components using Tarjan's algorithm.
///
/// A strongly connected component is a maximal set of nodes where every
/// node is reachable from every other node following directed edges.
///
/// # Arguments
/// * `graph` - A graph supporting neighbor lookups.
/// * `nodes` - The complete set of node IDs to consider.
///
/// # Returns
/// A vector of SCCs, each being a `Vec<NodeId>`.
pub fn strongly_connected_components(
    graph: &dyn GraphOps,
    nodes: &[NodeId],
) -> Result<Vec<Vec<NodeId>>> {
    let node_set: HashSet<NodeId> = nodes.iter().copied().collect();

    let mut index_counter: u64 = 0;
    let mut stack: Vec<NodeId> = Vec::new();
    let mut on_stack: HashSet<NodeId> = HashSet::new();
    let mut index_map: HashMap<NodeId, u64> = HashMap::new();
    let mut lowlink: HashMap<NodeId, u64> = HashMap::new();
    let mut result: Vec<Vec<NodeId>> = Vec::new();

    // Iterative Tarjan to avoid deep recursion on large graphs.
    // We simulate the DFS call stack with an explicit work stack.
    #[derive(Debug)]
    enum Frame {
        /// Enter a new node: assign index, push to stack, start iterating neighbors.
        Enter { node: NodeId },
        /// Resume after returning from a child; update lowlink and continue neighbors.
        Resume {
            node: NodeId,
            child: NodeId,
            remaining_neighbors: Vec<NodeId>,
        },
        /// Continue iterating the remaining neighbors of a node.
        Continue {
            node: NodeId,
            remaining_neighbors: Vec<NodeId>,
        },
    }

    for &start in nodes {
        if index_map.contains_key(&start) {
            continue;
        }

        let mut work: Vec<Frame> = vec![Frame::Enter { node: start }];

        while let Some(frame) = work.pop() {
            match frame {
                Frame::Enter { node } => {
                    index_map.insert(node, index_counter);
                    lowlink.insert(node, index_counter);
                    index_counter += 1;
                    stack.push(node);
                    on_stack.insert(node);

                    let neighbors = graph.neighbors(node, Direction::Outgoing)?;
                    let remaining: Vec<NodeId> = neighbors
                        .into_iter()
                        .map(|(_, n)| n)
                        .filter(|n| node_set.contains(n))
                        .collect();

                    work.push(Frame::Continue {
                        node,
                        remaining_neighbors: remaining,
                    });
                }
                Frame::Resume {
                    node,
                    child,
                    remaining_neighbors,
                } => {
                    let child_low = lowlink[&child];
                    let node_low = lowlink[&node];
                    if child_low < node_low {
                        lowlink.insert(node, child_low);
                    }
                    work.push(Frame::Continue {
                        node,
                        remaining_neighbors,
                    });
                }
                Frame::Continue {
                    node,
                    mut remaining_neighbors,
                } => {
                    let mut descended = false;
                    while let Some(neighbor) = remaining_neighbors.pop() {
                        if !index_map.contains_key(&neighbor) {
                            // Neighbor not yet visited. Descend into it.
                            work.push(Frame::Resume {
                                node,
                                child: neighbor,
                                remaining_neighbors,
                            });
                            work.push(Frame::Enter { node: neighbor });
                            descended = true;
                            break;
                        } else if on_stack.contains(&neighbor) {
                            let neighbor_idx = index_map[&neighbor];
                            let node_low = lowlink[&node];
                            if neighbor_idx < node_low {
                                lowlink.insert(node, neighbor_idx);
                            }
                        }
                    }

                    if !descended {
                        // All neighbors processed. Check if this is a root of an SCC.
                        if lowlink[&node] == index_map[&node] {
                            let mut scc = Vec::new();
                            loop {
                                let w = stack.pop().unwrap();
                                on_stack.remove(&w);
                                scc.push(w);
                                if w == node {
                                    break;
                                }
                            }
                            result.push(scc);
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestGraph;

    /// Two disconnected clusters: {1,2,3} and {4,5}
    fn two_clusters() -> TestGraph {
        let g = TestGraph::new();
        for i in 1..=5 {
            g.add_node(i, vec![]);
        }
        // Cluster 1: 1 <-> 2 <-> 3
        g.add_edge(1, 1, 2, 1.0);
        g.add_edge(2, 2, 3, 1.0);
        // Cluster 2: 4 <-> 5
        g.add_edge(3, 4, 5, 1.0);
        g
    }

    #[test]
    fn connected_components_two_clusters() {
        let g = two_clusters();
        let nodes: Vec<NodeId> = (1..=5).map(NodeId).collect();
        let mut components = connected_components(&g, &nodes).unwrap();

        assert_eq!(components.len(), 2, "Expected 2 connected components");

        // Sort each component and then sort components by first element for deterministic comparison.
        for c in &mut components {
            c.sort();
        }
        components.sort_by_key(|c| c[0]);

        assert_eq!(components[0], vec![NodeId(1), NodeId(2), NodeId(3)]);
        assert_eq!(components[1], vec![NodeId(4), NodeId(5)]);
    }

    #[test]
    fn connected_components_single_node() {
        let g = TestGraph::new();
        g.add_node(1, vec![]);
        let components = connected_components(&g, &[NodeId(1)]).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], vec![NodeId(1)]);
    }

    #[test]
    fn connected_components_all_isolated() {
        let g = TestGraph::new();
        for i in 1..=4 {
            g.add_node(i, vec![]);
        }
        let nodes: Vec<NodeId> = (1..=4).map(NodeId).collect();
        let components = connected_components(&g, &nodes).unwrap();
        assert_eq!(components.len(), 4, "Each isolated node is its own component");
    }

    #[test]
    fn scc_simple_cycle() {
        // 1 -> 2 -> 3 -> 1 forms one SCC.  4 has no edges.
        let g = TestGraph::new();
        for i in 1..=4 {
            g.add_node(i, vec![]);
        }
        g.add_edge(1, 1, 2, 1.0);
        g.add_edge(2, 2, 3, 1.0);
        g.add_edge(3, 3, 1, 1.0);

        let nodes: Vec<NodeId> = (1..=4).map(NodeId).collect();
        let mut sccs = strongly_connected_components(&g, &nodes).unwrap();

        // Sort internals for deterministic comparison.
        for scc in &mut sccs {
            scc.sort();
        }
        sccs.sort_by_key(|s| s[0]);

        // There should be 2 SCCs: {1,2,3} and {4}.
        assert_eq!(sccs.len(), 2);

        let cycle_scc = sccs.iter().find(|s| s.len() == 3).expect("Expected a 3-node SCC");
        assert_eq!(*cycle_scc, vec![NodeId(1), NodeId(2), NodeId(3)]);

        let singleton = sccs.iter().find(|s| s.len() == 1).expect("Expected a singleton SCC");
        assert_eq!(*singleton, vec![NodeId(4)]);
    }

    #[test]
    fn scc_dag_all_singletons() {
        // A DAG has no cycles; every node is its own SCC.
        // 1 -> 2 -> 3
        let g = TestGraph::new();
        for i in 1..=3 {
            g.add_node(i, vec![]);
        }
        g.add_edge(1, 1, 2, 1.0);
        g.add_edge(2, 2, 3, 1.0);

        let nodes: Vec<NodeId> = (1..=3).map(NodeId).collect();
        let sccs = strongly_connected_components(&g, &nodes).unwrap();
        assert_eq!(sccs.len(), 3, "A DAG should have all singleton SCCs");
    }
}

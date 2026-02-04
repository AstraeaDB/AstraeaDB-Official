use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::GraphOps;
use astraea_core::types::*;

/// A minimal in-memory graph implementation used exclusively for testing
/// the algorithm crate. This is not intended for production use.
pub struct TestGraph {
    nodes: RwLock<HashMap<NodeId, Node>>,
    edges: RwLock<Vec<Edge>>,
    next_node_id: AtomicU64,
    next_edge_id: AtomicU64,
}

impl TestGraph {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            next_node_id: AtomicU64::new(1),
            next_edge_id: AtomicU64::new(1),
        }
    }

    /// Add a node with the given numeric ID and labels.
    /// Advances the internal counter past this ID.
    pub fn add_node(&self, id: u64, labels: Vec<String>) {
        let node = Node {
            id: NodeId(id),
            labels,
            properties: serde_json::json!({}),
            embedding: None,
        };
        self.nodes.write().unwrap().insert(NodeId(id), node);
        // Ensure the auto-increment counter stays ahead of manually-assigned IDs.
        let _ = self
            .next_node_id
            .fetch_max(id + 1, Ordering::Relaxed);
    }

    /// Add a directed edge between two nodes with the specified weight.
    pub fn add_edge(&self, id: u64, source: u64, target: u64, weight: f64) {
        let edge = Edge {
            id: EdgeId(id),
            source: NodeId(source),
            target: NodeId(target),
            edge_type: "LINK".into(),
            properties: serde_json::json!({}),
            weight,
            validity: ValidityInterval::always(),
        };
        self.edges.write().unwrap().push(edge);
        let _ = self
            .next_edge_id
            .fetch_max(id + 1, Ordering::Relaxed);
    }
}

impl GraphOps for TestGraph {
    fn create_node(
        &self,
        labels: Vec<String>,
        properties: serde_json::Value,
        embedding: Option<Vec<f32>>,
    ) -> Result<NodeId> {
        let id = NodeId(self.next_node_id.fetch_add(1, Ordering::Relaxed));
        let node = Node {
            id,
            labels,
            properties,
            embedding,
        };
        self.nodes.write().unwrap().insert(id, node);
        Ok(id)
    }

    fn create_edge(
        &self,
        source: NodeId,
        target: NodeId,
        edge_type: String,
        properties: serde_json::Value,
        weight: f64,
        valid_from: Option<i64>,
        valid_to: Option<i64>,
    ) -> Result<EdgeId> {
        let id = EdgeId(self.next_edge_id.fetch_add(1, Ordering::Relaxed));
        let edge = Edge {
            id,
            source,
            target,
            edge_type,
            properties,
            weight,
            validity: ValidityInterval {
                valid_from,
                valid_to,
            },
        };
        self.edges.write().unwrap().push(edge);
        Ok(id)
    }

    fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        Ok(self.nodes.read().unwrap().get(&id).cloned())
    }

    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
        Ok(self.edges.read().unwrap().iter().find(|e| e.id == id).cloned())
    }

    fn update_node(&self, id: NodeId, properties: serde_json::Value) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes
            .get_mut(&id)
            .ok_or(AstraeaError::NodeNotFound(id))?;
        if let (serde_json::Value::Object(existing), serde_json::Value::Object(new)) =
            (&mut node.properties, properties)
        {
            for (k, v) in new {
                existing.insert(k, v);
            }
        }
        Ok(())
    }

    fn update_edge(&self, id: EdgeId, properties: serde_json::Value) -> Result<()> {
        let mut edges = self.edges.write().unwrap();
        let edge = edges
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(AstraeaError::EdgeNotFound(id))?;
        if let (serde_json::Value::Object(existing), serde_json::Value::Object(new)) =
            (&mut edge.properties, properties)
        {
            for (k, v) in new {
                existing.insert(k, v);
            }
        }
        Ok(())
    }

    fn delete_node(&self, id: NodeId) -> Result<()> {
        self.nodes.write().unwrap().remove(&id);
        self.edges.write().unwrap().retain(|e| e.source != id && e.target != id);
        Ok(())
    }

    fn delete_edge(&self, id: EdgeId) -> Result<()> {
        self.edges.write().unwrap().retain(|e| e.id != id);
        Ok(())
    }

    fn neighbors(&self, node_id: NodeId, direction: Direction) -> Result<Vec<(EdgeId, NodeId)>> {
        let edges = self.edges.read().unwrap();
        let mut result = Vec::new();
        for e in edges.iter() {
            match direction {
                Direction::Outgoing => {
                    if e.source == node_id {
                        result.push((e.id, e.target));
                    }
                }
                Direction::Incoming => {
                    if e.target == node_id {
                        result.push((e.id, e.source));
                    }
                }
                Direction::Both => {
                    if e.source == node_id {
                        result.push((e.id, e.target));
                    }
                    if e.target == node_id {
                        result.push((e.id, e.source));
                    }
                }
            }
        }
        Ok(result)
    }

    fn neighbors_filtered(
        &self,
        node_id: NodeId,
        direction: Direction,
        edge_type: &str,
    ) -> Result<Vec<(EdgeId, NodeId)>> {
        let edges = self.edges.read().unwrap();
        let mut result = Vec::new();
        for e in edges.iter() {
            if e.edge_type != edge_type {
                continue;
            }
            match direction {
                Direction::Outgoing => {
                    if e.source == node_id {
                        result.push((e.id, e.target));
                    }
                }
                Direction::Incoming => {
                    if e.target == node_id {
                        result.push((e.id, e.source));
                    }
                }
                Direction::Both => {
                    if e.source == node_id {
                        result.push((e.id, e.target));
                    }
                    if e.target == node_id {
                        result.push((e.id, e.source));
                    }
                }
            }
        }
        Ok(result)
    }

    fn bfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<(NodeId, usize)>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        visited.insert(start);
        queue.push_back((start, 0usize));

        while let Some((current, depth)) = queue.pop_front() {
            result.push((current, depth));
            if depth >= max_depth {
                continue;
            }
            let neighbors = self.neighbors(current, Direction::Outgoing)?;
            for (_edge_id, neighbor) in neighbors {
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
        Ok(result)
    }

    fn dfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<NodeId>> {
        let mut visited = HashSet::new();
        let mut stack = vec![(start, 0usize)];
        let mut result = Vec::new();

        while let Some((current, depth)) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            result.push(current);
            if depth >= max_depth {
                continue;
            }
            let neighbors = self.neighbors(current, Direction::Outgoing)?;
            for (_edge_id, neighbor) in neighbors {
                if !visited.contains(&neighbor) {
                    stack.push((neighbor, depth + 1));
                }
            }
        }
        Ok(result)
    }

    fn shortest_path(&self, from: NodeId, to: NodeId) -> Result<Option<GraphPath>> {
        if from == to {
            return Ok(Some(GraphPath::new(from)));
        }
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        // Each entry: (current_node, path_of_(edge, node) pairs)
        let mut parent: HashMap<NodeId, (EdgeId, NodeId)> = HashMap::new();

        visited.insert(from);
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            let neighbors = self.neighbors(current, Direction::Outgoing)?;
            for (edge_id, neighbor) in neighbors {
                if visited.insert(neighbor) {
                    parent.insert(neighbor, (edge_id, current));
                    if neighbor == to {
                        // Reconstruct the path
                        let mut path = GraphPath::new(from);
                        let mut steps = Vec::new();
                        let mut cursor = to;
                        while cursor != from {
                            let (eid, prev) = parent[&cursor];
                            steps.push((eid, cursor));
                            cursor = prev;
                        }
                        steps.reverse();
                        for (eid, nid) in steps {
                            path.push(eid, nid);
                        }
                        return Ok(Some(path));
                    }
                    queue.push_back(neighbor);
                }
            }
        }
        Ok(None)
    }

    fn shortest_path_weighted(
        &self,
        _from: NodeId,
        _to: NodeId,
    ) -> Result<Option<(GraphPath, f64)>> {
        Err(AstraeaError::QueryExecution(
            "shortest_path_weighted not implemented in TestGraph".into(),
        ))
    }

    fn find_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        let nodes = self.nodes.read().unwrap();
        Ok(nodes
            .values()
            .filter(|n| n.labels.iter().any(|l| l == label))
            .map(|n| n.id)
            .collect())
    }
}

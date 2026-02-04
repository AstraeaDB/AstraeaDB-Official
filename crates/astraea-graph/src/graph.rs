use std::sync::atomic::{AtomicU64, Ordering};

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::{GraphOps, StorageEngine};
use astraea_core::types::*;

use crate::traversal;

/// The primary graph database handle.
///
/// Wraps a `StorageEngine` and provides high-level graph operations
/// including CRUD, traversals, and path finding.
pub struct Graph {
    storage: Box<dyn StorageEngine>,
    next_node_id: AtomicU64,
    next_edge_id: AtomicU64,
}

impl Graph {
    /// Create a new graph backed by the given storage engine.
    pub fn new(storage: Box<dyn StorageEngine>) -> Self {
        Self {
            storage,
            next_node_id: AtomicU64::new(1),
            next_edge_id: AtomicU64::new(1),
        }
    }

    /// Create a new graph with explicit starting IDs (for recovery).
    pub fn with_start_ids(
        storage: Box<dyn StorageEngine>,
        next_node_id: u64,
        next_edge_id: u64,
    ) -> Self {
        Self {
            storage,
            next_node_id: AtomicU64::new(next_node_id),
            next_edge_id: AtomicU64::new(next_edge_id),
        }
    }

    fn alloc_node_id(&self) -> NodeId {
        NodeId(self.next_node_id.fetch_add(1, Ordering::Relaxed))
    }

    fn alloc_edge_id(&self) -> EdgeId {
        EdgeId(self.next_edge_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Get a reference to the underlying storage engine.
    pub fn storage(&self) -> &dyn StorageEngine {
        self.storage.as_ref()
    }
}

impl GraphOps for Graph {
    fn create_node(
        &self,
        labels: Vec<String>,
        properties: serde_json::Value,
        embedding: Option<Vec<f32>>,
    ) -> Result<NodeId> {
        let id = self.alloc_node_id();
        let node = Node {
            id,
            labels,
            properties,
            embedding,
        };
        self.storage.put_node(&node)?;
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
        // Verify both endpoints exist.
        if self.storage.get_node(source)?.is_none() {
            return Err(AstraeaError::NodeNotFound(source));
        }
        if self.storage.get_node(target)?.is_none() {
            return Err(AstraeaError::NodeNotFound(target));
        }

        let id = self.alloc_edge_id();
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
        self.storage.put_edge(&edge)?;
        Ok(id)
    }

    fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        self.storage.get_node(id)
    }

    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
        self.storage.get_edge(id)
    }

    fn update_node(&self, id: NodeId, properties: serde_json::Value) -> Result<()> {
        let mut node = self
            .storage
            .get_node(id)?
            .ok_or(AstraeaError::NodeNotFound(id))?;

        merge_json(&mut node.properties, properties);
        self.storage.put_node(&node)
    }

    fn update_edge(&self, id: EdgeId, properties: serde_json::Value) -> Result<()> {
        let mut edge = self
            .storage
            .get_edge(id)?
            .ok_or(AstraeaError::EdgeNotFound(id))?;

        merge_json(&mut edge.properties, properties);
        self.storage.put_edge(&edge)
    }

    fn delete_node(&self, id: NodeId) -> Result<()> {
        // Delete all connected edges first (both directions).
        let outgoing = self.storage.get_edges(id, Direction::Outgoing)?;
        let incoming = self.storage.get_edges(id, Direction::Incoming)?;

        for edge in outgoing.iter().chain(incoming.iter()) {
            self.storage.delete_edge(edge.id)?;
        }

        self.storage.delete_node(id)?;
        Ok(())
    }

    fn delete_edge(&self, id: EdgeId) -> Result<()> {
        self.storage.delete_edge(id)?;
        Ok(())
    }

    fn neighbors(&self, node_id: NodeId, direction: Direction) -> Result<Vec<(EdgeId, NodeId)>> {
        let edges = self.storage.get_edges(node_id, direction)?;
        Ok(edges
            .into_iter()
            .map(|e| {
                let neighbor = if e.source == node_id {
                    e.target
                } else {
                    e.source
                };
                (e.id, neighbor)
            })
            .collect())
    }

    fn neighbors_filtered(
        &self,
        node_id: NodeId,
        direction: Direction,
        edge_type: &str,
    ) -> Result<Vec<(EdgeId, NodeId)>> {
        let edges = self.storage.get_edges(node_id, direction)?;
        Ok(edges
            .into_iter()
            .filter(|e| e.edge_type == edge_type)
            .map(|e| {
                let neighbor = if e.source == node_id {
                    e.target
                } else {
                    e.source
                };
                (e.id, neighbor)
            })
            .collect())
    }

    fn bfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<(NodeId, usize)>> {
        traversal::bfs(self.storage.as_ref(), start, max_depth)
    }

    fn dfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<NodeId>> {
        traversal::dfs(self.storage.as_ref(), start, max_depth)
    }

    fn shortest_path(&self, from: NodeId, to: NodeId) -> Result<Option<GraphPath>> {
        traversal::shortest_path_unweighted(self.storage.as_ref(), from, to)
    }

    fn shortest_path_weighted(
        &self,
        from: NodeId,
        to: NodeId,
    ) -> Result<Option<(GraphPath, f64)>> {
        traversal::shortest_path_dijkstra(self.storage.as_ref(), from, to)
    }

    fn find_by_label(&self, _label: &str) -> Result<Vec<NodeId>> {
        // TODO: implement label index scan. For now, this is a placeholder.
        // A full implementation requires a label -> NodeId index in storage.
        Err(AstraeaError::QueryExecution(
            "label index scan not yet implemented".into(),
        ))
    }
}

/// Merge `patch` into `target` with JSON object merge semantics.
/// - If both are objects, keys from patch are inserted/overwritten.
/// - Otherwise, target is replaced by patch.
fn merge_json(target: &mut serde_json::Value, patch: serde_json::Value) {
    if let (Some(target_map), serde_json::Value::Object(patch_map)) =
        (target.as_object_mut(), &patch)
    {
        for (key, value) in patch_map {
            target_map.insert(key.clone(), value.clone());
        }
    } else {
        *target = patch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::InMemoryStorage;

    #[test]
    fn create_and_get_node() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let id = graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({"name": "Alice"}),
                None,
            )
            .unwrap();

        let node = graph.get_node(id).unwrap().unwrap();
        assert_eq!(node.id, id);
        assert_eq!(node.labels, vec!["Person"]);
        assert_eq!(node.properties["name"], "Alice");
    }

    #[test]
    fn create_and_get_edge() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let e = graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();

        let edge = graph.get_edge(e).unwrap().unwrap();
        assert_eq!(edge.source, a);
        assert_eq!(edge.target, b);
        assert_eq!(edge.edge_type, "KNOWS");
    }

    #[test]
    fn create_edge_missing_node_fails() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();

        let result = graph.create_edge(a, NodeId(999), "KNOWS".into(), serde_json::json!({}), 1.0, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn update_node_properties() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let id = graph
            .create_node(
                vec![],
                serde_json::json!({"name": "Alice", "age": 30}),
                None,
            )
            .unwrap();

        graph
            .update_node(id, serde_json::json!({"age": 31, "city": "NYC"}))
            .unwrap();

        let node = graph.get_node(id).unwrap().unwrap();
        assert_eq!(node.properties["name"], "Alice");
        assert_eq!(node.properties["age"], 31);
        assert_eq!(node.properties["city"], "NYC");
    }

    #[test]
    fn delete_node_cascades_edges() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let e = graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();

        graph.delete_node(a).unwrap();

        assert!(graph.get_node(a).unwrap().is_none());
        assert!(graph.get_edge(e).unwrap().is_none());
        // b should still exist
        assert!(graph.get_node(b).unwrap().is_some());
    }

    #[test]
    fn neighbors_both_directions() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let c = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();

        graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(c, a, "FOLLOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();

        let out = graph.neighbors(a, Direction::Outgoing).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, b);

        let inc = graph.neighbors(a, Direction::Incoming).unwrap();
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0].1, c);

        let both = graph.neighbors(a, Direction::Both).unwrap();
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn neighbors_filtered_by_type() {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));
        let a = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let b = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();
        let c = graph
            .create_node(vec![], serde_json::json!({}), None)
            .unwrap();

        graph
            .create_edge(a, b, "KNOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();
        graph
            .create_edge(a, c, "FOLLOWS".into(), serde_json::json!({}), 1.0, None, None)
            .unwrap();

        let knows = graph
            .neighbors_filtered(a, Direction::Outgoing, "KNOWS")
            .unwrap();
        assert_eq!(knows.len(), 1);
        assert_eq!(knows[0].1, b);
    }

    #[test]
    fn merge_json_objects() {
        let mut target = serde_json::json!({"a": 1, "b": 2});
        merge_json(&mut target, serde_json::json!({"b": 3, "c": 4}));
        assert_eq!(target, serde_json::json!({"a": 1, "b": 3, "c": 4}));
    }

    #[test]
    fn merge_json_non_object_replaces() {
        let mut target = serde_json::json!({"a": 1});
        merge_json(&mut target, serde_json::json!(42));
        assert_eq!(target, serde_json::json!(42));
    }
}

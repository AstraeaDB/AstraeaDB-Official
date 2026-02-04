use std::collections::{HashMap, HashSet};

use parking_lot::RwLock;

use astraea_core::error::Result;
use astraea_core::traits::StorageEngine;
use astraea_core::types::*;

/// Simple in-memory storage engine for testing.
///
/// Stores nodes and edges in HashMaps. No persistence, no pages, no WAL.
/// Maintains a label index for fast label-based lookups.
/// Useful for unit-testing graph operations and traversals.
pub struct InMemoryStorage {
    nodes: RwLock<HashMap<NodeId, Node>>,
    edges: RwLock<HashMap<EdgeId, Edge>>,
    label_index: RwLock<HashMap<String, HashSet<NodeId>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(HashMap::new()),
            label_index: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageEngine for InMemoryStorage {
    fn put_node(&self, node: &Node) -> Result<()> {
        // If the node already exists, remove its old labels from the index.
        if let Some(old_node) = self.nodes.read().get(&node.id) {
            let mut li = self.label_index.write();
            for label in &old_node.labels {
                if let Some(set) = li.get_mut(label) {
                    set.remove(&node.id);
                    if set.is_empty() {
                        li.remove(label);
                    }
                }
            }
        }

        // Add new labels to the index.
        {
            let mut li = self.label_index.write();
            for label in &node.labels {
                li.entry(label.clone()).or_default().insert(node.id);
            }
        }

        self.nodes.write().insert(node.id, node.clone());
        Ok(())
    }

    fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        Ok(self.nodes.read().get(&id).cloned())
    }

    fn delete_node(&self, id: NodeId) -> Result<bool> {
        let removed = self.nodes.write().remove(&id);
        if let Some(node) = &removed {
            let mut li = self.label_index.write();
            for label in &node.labels {
                if let Some(set) = li.get_mut(label) {
                    set.remove(&id);
                    if set.is_empty() {
                        li.remove(label);
                    }
                }
            }
        }
        Ok(removed.is_some())
    }

    fn put_edge(&self, edge: &Edge) -> Result<()> {
        self.edges.write().insert(edge.id, edge.clone());
        Ok(())
    }

    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
        Ok(self.edges.read().get(&id).cloned())
    }

    fn delete_edge(&self, id: EdgeId) -> Result<bool> {
        Ok(self.edges.write().remove(&id).is_some())
    }

    fn get_edges(&self, node_id: NodeId, direction: Direction) -> Result<Vec<Edge>> {
        let edges = self.edges.read();
        let result = edges
            .values()
            .filter(|e| match direction {
                Direction::Outgoing => e.source == node_id,
                Direction::Incoming => e.target == node_id,
                Direction::Both => e.source == node_id || e.target == node_id,
            })
            .cloned()
            .collect();
        Ok(result)
    }

    fn flush(&self) -> Result<()> {
        Ok(()) // no-op for in-memory storage
    }

    fn find_nodes_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        let li = self.label_index.read();
        Ok(li
            .get(label)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_get_node() {
        let store = InMemoryStorage::new();
        let node = Node {
            id: NodeId(1),
            labels: vec!["Test".into()],
            properties: serde_json::json!({"x": 1}),
            embedding: None,
        };
        store.put_node(&node).unwrap();

        let retrieved = store.get_node(NodeId(1)).unwrap().unwrap();
        assert_eq!(retrieved.id, NodeId(1));
        assert_eq!(retrieved.properties["x"], 1);
    }

    #[test]
    fn get_missing_node_returns_none() {
        let store = InMemoryStorage::new();
        assert!(store.get_node(NodeId(999)).unwrap().is_none());
    }

    #[test]
    fn delete_node() {
        let store = InMemoryStorage::new();
        let node = Node {
            id: NodeId(1),
            labels: vec![],
            properties: serde_json::json!({}),
            embedding: None,
        };
        store.put_node(&node).unwrap();
        assert!(store.delete_node(NodeId(1)).unwrap());
        assert!(store.get_node(NodeId(1)).unwrap().is_none());
        assert!(!store.delete_node(NodeId(1)).unwrap()); // already deleted
    }

    #[test]
    fn get_edges_by_direction() {
        let store = InMemoryStorage::new();
        let edge = Edge {
            id: EdgeId(1),
            source: NodeId(10),
            target: NodeId(20),
            edge_type: "LINK".into(),
            properties: serde_json::json!({}),
            weight: 1.0,
            validity: ValidityInterval::always(),
        };
        store.put_edge(&edge).unwrap();

        assert_eq!(
            store
                .get_edges(NodeId(10), Direction::Outgoing)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .get_edges(NodeId(10), Direction::Incoming)
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            store
                .get_edges(NodeId(20), Direction::Incoming)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .get_edges(NodeId(10), Direction::Both)
                .unwrap()
                .len(),
            1
        );
    }
}

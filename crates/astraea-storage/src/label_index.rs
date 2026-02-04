//! In-memory label index mapping labels to sets of node IDs.
//!
//! Provides O(1) hash-based lookup for finding all nodes that carry a given
//! label, replacing the previous linear scan in `find_by_label()`.

use astraea_core::types::NodeId;
use std::collections::{HashMap, HashSet};

/// In-memory index mapping labels to the set of node IDs carrying that label.
pub struct LabelIndex {
    index: HashMap<String, HashSet<NodeId>>,
}

impl LabelIndex {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
        }
    }

    /// Add all labels for a node.
    pub fn add_node(&mut self, node_id: NodeId, labels: &[String]) {
        for label in labels {
            self.index.entry(label.clone()).or_default().insert(node_id);
        }
    }

    /// Remove all labels for a node.
    pub fn remove_node(&mut self, node_id: NodeId, labels: &[String]) {
        for label in labels {
            if let Some(set) = self.index.get_mut(label) {
                set.remove(&node_id);
                if set.is_empty() {
                    self.index.remove(label);
                }
            }
        }
    }

    /// Get all node IDs with a given label.
    pub fn get(&self, label: &str) -> Vec<NodeId> {
        self.index
            .get(label)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Get node IDs that have ALL of the given labels (intersection).
    pub fn get_intersection(&self, labels: &[String]) -> Vec<NodeId> {
        if labels.is_empty() {
            return Vec::new();
        }
        let mut sets: Vec<&HashSet<NodeId>> = labels
            .iter()
            .filter_map(|l| self.index.get(l.as_str()))
            .collect();
        if sets.len() != labels.len() {
            return Vec::new(); // some label has no nodes
        }
        sets.sort_by_key(|s| s.len()); // start with smallest set
        let first = sets[0];
        first
            .iter()
            .filter(|id| sets[1..].iter().all(|s| s.contains(id)))
            .copied()
            .collect()
    }

    /// Get all distinct labels in the index.
    pub fn all_labels(&self) -> Vec<String> {
        self.index.keys().cloned().collect()
    }

    /// Total number of indexed entries (for diagnostics).
    pub fn len(&self) -> usize {
        self.index.values().map(|s| s.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

impl Default for LabelIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get() {
        let mut idx = LabelIndex::new();
        idx.add_node(NodeId(1), &["Person".to_string()]);
        idx.add_node(NodeId(2), &["Person".to_string()]);
        idx.add_node(NodeId(3), &["Company".to_string()]);

        let mut persons = idx.get("Person");
        persons.sort_by_key(|n| n.0);
        assert_eq!(persons, vec![NodeId(1), NodeId(2)]);

        let companies = idx.get("Company");
        assert_eq!(companies, vec![NodeId(3)]);
    }

    #[test]
    fn test_remove() {
        let mut idx = LabelIndex::new();
        idx.add_node(NodeId(1), &["Person".to_string()]);
        idx.add_node(NodeId(2), &["Person".to_string()]);

        idx.remove_node(NodeId(1), &["Person".to_string()]);

        let persons = idx.get("Person");
        assert_eq!(persons, vec![NodeId(2)]);

        // Remove the last one; the label entry should be cleaned up.
        idx.remove_node(NodeId(2), &["Person".to_string()]);
        let persons = idx.get("Person");
        assert!(persons.is_empty());
        assert!(idx.is_empty());
    }

    #[test]
    fn test_multi_label() {
        let mut idx = LabelIndex::new();
        idx.add_node(
            NodeId(1),
            &["Person".to_string(), "Employee".to_string()],
        );

        let persons = idx.get("Person");
        assert_eq!(persons, vec![NodeId(1)]);

        let employees = idx.get("Employee");
        assert_eq!(employees, vec![NodeId(1)]);
    }

    #[test]
    fn test_intersection() {
        let mut idx = LabelIndex::new();
        idx.add_node(
            NodeId(1),
            &["Person".to_string(), "Employee".to_string()],
        );
        idx.add_node(NodeId(2), &["Person".to_string()]);
        idx.add_node(NodeId(3), &["Employee".to_string()]);

        let both = idx.get_intersection(&[
            "Person".to_string(),
            "Employee".to_string(),
        ]);
        assert_eq!(both, vec![NodeId(1)]);

        // If one label doesn't exist, intersection is empty.
        let none = idx.get_intersection(&[
            "Person".to_string(),
            "Nonexistent".to_string(),
        ]);
        assert!(none.is_empty());

        // Empty labels list returns empty.
        let empty = idx.get_intersection(&[]);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_empty_label() {
        let idx = LabelIndex::new();
        let result = idx.get("Nonexistent");
        assert!(result.is_empty());
    }
}

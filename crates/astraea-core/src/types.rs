use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a node in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct NodeId(pub u64);

/// Unique identifier for an edge in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct EdgeId(pub u64);

/// Unique identifier for a page in the storage engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageId(pub u64);

/// Unique identifier for a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TransactionId(pub u64);

/// Log sequence number for WAL entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lsn(pub u64);

/// Direction for edge traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Both,
}

/// A node in the Vector-Property Graph.
///
/// Contains JSON properties, labels, and an optional dense embedding vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub labels: Vec<String>,
    pub properties: serde_json::Value,
    /// Fixed-size float32 embedding vector for semantic search.
    pub embedding: Option<Vec<f32>>,
}

/// Temporal validity interval for edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidityInterval {
    /// Start of validity (epoch milliseconds), inclusive. None = unbounded start.
    pub valid_from: Option<i64>,
    /// End of validity (epoch milliseconds), exclusive. None = still valid.
    pub valid_to: Option<i64>,
}

impl ValidityInterval {
    /// An interval that is always valid (no bounds).
    pub fn always() -> Self {
        Self {
            valid_from: None,
            valid_to: None,
        }
    }

    /// Check if the interval contains the given timestamp.
    pub fn contains(&self, timestamp: i64) -> bool {
        let after_start = self.valid_from.is_none_or(|start| timestamp >= start);
        let before_end = self.valid_to.is_none_or(|end| timestamp < end);
        after_start && before_end
    }
}

/// An edge in the Vector-Property Graph.
///
/// Connects two nodes with a typed relationship. Supports temporal validity
/// and a learnable weight for GNN integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub edge_type: String,
    pub properties: serde_json::Value,
    /// Learnable weight for GNN / differentiable traversal.
    pub weight: f64,
    /// Temporal validity interval.
    pub validity: ValidityInterval,
}

/// An ordered path through the graph: alternating nodes and edges.
#[derive(Debug, Clone)]
pub struct GraphPath {
    /// Sequence of (edge taken, node arrived at). The starting node is implicit.
    pub start: NodeId,
    pub steps: Vec<(EdgeId, NodeId)>,
}

impl GraphPath {
    pub fn new(start: NodeId) -> Self {
        Self {
            start,
            steps: Vec::new(),
        }
    }

    pub fn push(&mut self, edge: EdgeId, node: NodeId) {
        self.steps.push((edge, node));
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The final node in the path.
    pub fn end(&self) -> NodeId {
        self.steps.last().map(|(_, n)| *n).unwrap_or(self.start)
    }

    /// All node IDs in the path, including start.
    pub fn nodes(&self) -> Vec<NodeId> {
        let mut nodes = vec![self.start];
        for (_, n) in &self.steps {
            nodes.push(*n);
        }
        nodes
    }
}

/// Distance metric for vector similarity search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistanceMetric {
    Cosine,
    Euclidean,
    DotProduct,
}

/// Result of a vector similarity search.
#[derive(Debug, Clone)]
pub struct SimilarityResult {
    pub node_id: NodeId,
    pub distance: f32,
}

// --- Display impls ---

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "e{}", self.0)
    }
}

impl fmt::Display for PageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.0)
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tx{}", self.0)
    }
}

impl fmt::Display for Lsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lsn{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity_interval_always() {
        let iv = ValidityInterval::always();
        assert!(iv.contains(0));
        assert!(iv.contains(i64::MAX));
        assert!(iv.contains(i64::MIN));
    }

    #[test]
    fn validity_interval_bounded() {
        let iv = ValidityInterval {
            valid_from: Some(100),
            valid_to: Some(200),
        };
        assert!(!iv.contains(99));
        assert!(iv.contains(100));
        assert!(iv.contains(150));
        assert!(!iv.contains(200)); // exclusive end
    }

    #[test]
    fn validity_interval_half_open() {
        let from_only = ValidityInterval {
            valid_from: Some(100),
            valid_to: None,
        };
        assert!(!from_only.contains(99));
        assert!(from_only.contains(100));
        assert!(from_only.contains(i64::MAX));

        let to_only = ValidityInterval {
            valid_from: None,
            valid_to: Some(200),
        };
        assert!(to_only.contains(i64::MIN));
        assert!(to_only.contains(199));
        assert!(!to_only.contains(200));
    }

    #[test]
    fn graph_path_basic() {
        let mut path = GraphPath::new(NodeId(1));
        assert_eq!(path.end(), NodeId(1));
        assert!(path.is_empty());

        path.push(EdgeId(10), NodeId(2));
        path.push(EdgeId(20), NodeId(3));
        assert_eq!(path.len(), 2);
        assert_eq!(path.end(), NodeId(3));
        assert_eq!(path.nodes(), vec![NodeId(1), NodeId(2), NodeId(3)]);
    }
}

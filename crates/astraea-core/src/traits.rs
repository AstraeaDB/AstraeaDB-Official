use crate::error::Result;
use crate::types::*;

/// Low-level storage engine trait for persisting and retrieving nodes and edges.
///
/// Implementations handle the page-based storage, buffer pool, and disk I/O.
/// This trait intentionally does NOT handle transactions — that is layered on top.
pub trait StorageEngine: Send + Sync {
    /// Store a node. Overwrites if the node ID already exists.
    fn put_node(&self, node: &Node) -> Result<()>;

    /// Retrieve a node by ID.
    fn get_node(&self, id: NodeId) -> Result<Option<Node>>;

    /// Delete a node by ID. Returns true if the node existed.
    fn delete_node(&self, id: NodeId) -> Result<bool>;

    /// Store an edge. Overwrites if the edge ID already exists.
    fn put_edge(&self, edge: &Edge) -> Result<()>;

    /// Retrieve an edge by ID.
    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>>;

    /// Delete an edge by ID. Returns true if the edge existed.
    fn delete_edge(&self, id: EdgeId) -> Result<bool>;

    /// Get all edges connected to a node in the given direction.
    fn get_edges(&self, node_id: NodeId, direction: Direction) -> Result<Vec<Edge>>;

    /// Flush all dirty data to disk.
    fn flush(&self) -> Result<()>;

    /// Find all node IDs that carry the given label.
    ///
    /// The default implementation returns an empty vector. Storage engines
    /// that maintain a label index should override this for O(1) lookups.
    fn find_nodes_by_label(&self, _label: &str) -> Result<Vec<NodeId>> {
        Ok(Vec::new())
    }
}

/// Extension trait for transactional storage operations.
///
/// Provides MVCC-based transactional access to the storage engine.
/// Writes are buffered in the transaction and applied atomically on commit.
pub trait TransactionalEngine: StorageEngine {
    /// Begin a new transaction. Returns the assigned transaction ID.
    fn begin_transaction(&self) -> Result<TransactionId>;

    /// Commit a transaction, atomically applying all buffered writes.
    fn commit_transaction(&self, txn_id: TransactionId) -> Result<()>;

    /// Abort a transaction, discarding all buffered writes.
    fn abort_transaction(&self, txn_id: TransactionId) -> Result<()>;

    /// Buffer a node write within the given transaction.
    fn put_node_tx(&self, node: &Node, txn_id: TransactionId) -> Result<()>;

    /// Buffer a node deletion within the given transaction.
    /// Returns false if the node was not found (but does not error).
    fn delete_node_tx(&self, id: NodeId, txn_id: TransactionId) -> Result<bool>;

    /// Buffer an edge write within the given transaction.
    fn put_edge_tx(&self, edge: &Edge, txn_id: TransactionId) -> Result<()>;

    /// Buffer an edge deletion within the given transaction.
    /// Returns false if the edge was not found (but does not error).
    fn delete_edge_tx(&self, id: EdgeId, txn_id: TransactionId) -> Result<bool>;
}

/// Graph-level operations: CRUD and traversals over the property graph.
pub trait GraphOps: Send + Sync {
    /// Create a new node with the given labels and properties.
    /// Returns the assigned NodeId.
    fn create_node(
        &self,
        labels: Vec<String>,
        properties: serde_json::Value,
        embedding: Option<Vec<f32>>,
    ) -> Result<NodeId>;

    /// Create a new edge between two nodes.
    /// Returns the assigned EdgeId.
    /// `valid_from` and `valid_to` are optional epoch-millisecond bounds for temporal validity.
    fn create_edge(
        &self,
        source: NodeId,
        target: NodeId,
        edge_type: String,
        properties: serde_json::Value,
        weight: f64,
        valid_from: Option<i64>,
        valid_to: Option<i64>,
    ) -> Result<EdgeId>;

    /// Get a node by ID.
    fn get_node(&self, id: NodeId) -> Result<Option<Node>>;

    /// Get an edge by ID.
    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>>;

    /// Update a node's properties (merge semantics).
    fn update_node(&self, id: NodeId, properties: serde_json::Value) -> Result<()>;

    /// Update an edge's properties (merge semantics).
    fn update_edge(&self, id: EdgeId, properties: serde_json::Value) -> Result<()>;

    /// Delete a node and all its connected edges.
    fn delete_node(&self, id: NodeId) -> Result<()>;

    /// Delete an edge.
    fn delete_edge(&self, id: EdgeId) -> Result<()>;

    /// Get neighbor node IDs reachable from the given node in the given direction.
    fn neighbors(&self, node_id: NodeId, direction: Direction) -> Result<Vec<(EdgeId, NodeId)>>;

    /// Get neighbor node IDs filtered by edge type.
    fn neighbors_filtered(
        &self,
        node_id: NodeId,
        direction: Direction,
        edge_type: &str,
    ) -> Result<Vec<(EdgeId, NodeId)>>;

    /// Breadth-first search from a starting node up to a maximum depth.
    /// Returns all discovered nodes with their depth.
    fn bfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<(NodeId, usize)>>;

    /// Depth-first search from a starting node up to a maximum depth.
    /// Returns all discovered nodes.
    fn dfs(&self, start: NodeId, max_depth: usize) -> Result<Vec<NodeId>>;

    /// Find the shortest path between two nodes (unweighted).
    fn shortest_path(&self, from: NodeId, to: NodeId) -> Result<Option<GraphPath>>;

    /// Find the shortest path between two nodes using edge weights (Dijkstra).
    fn shortest_path_weighted(&self, from: NodeId, to: NodeId) -> Result<Option<(GraphPath, f64)>>;

    /// Find all nodes matching a label.
    fn find_by_label(&self, label: &str) -> Result<Vec<NodeId>>;
}

/// Vector index trait for approximate nearest neighbor search.
pub trait VectorIndex: Send + Sync {
    /// Insert a vector for a node. Dimension must match the index's configured dimension.
    fn insert(&self, node_id: NodeId, embedding: &[f32]) -> Result<()>;

    /// Remove a vector for a node.
    fn remove(&self, node_id: NodeId) -> Result<bool>;

    /// Search for the k nearest neighbors of the query vector.
    fn search(&self, query: &[f32], k: usize) -> Result<Vec<SimilarityResult>>;

    /// The dimensionality of vectors in this index.
    fn dimension(&self) -> usize;

    /// The distance metric used by this index.
    fn metric(&self) -> DistanceMetric;

    /// Number of vectors currently in the index.
    fn len(&self) -> usize;

    /// Whether the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

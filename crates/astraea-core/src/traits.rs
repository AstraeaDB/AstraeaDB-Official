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

    /// Find all edges whose `edge_type` matches the given string.
    ///
    /// Returns a list of `(EdgeId, source NodeId, target NodeId)` triples.
    /// The default implementation returns an empty vector. Storage engines
    /// that maintain an edge index should override this.
    fn find_edges_by_type(&self, _edge_type: &str) -> Result<Vec<(EdgeId, NodeId, NodeId)>> {
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

    /// Create a node at a caller-supplied id. Used by import paths
    /// (Flight `do_put`, JSON `import`) that need to preserve client-side
    /// identifiers across an export/import roundtrip.
    /// astraeadb-issues.md #14.
    ///
    /// Implementations must (a) fail with [`AstraeaError::DuplicateNode`]
    /// if `id` is already in use, and (b) advance the id allocator past
    /// `id` so subsequent [`create_node`] calls don't collide.
    ///
    /// The default falls back to auto-assignment via [`create_node`] —
    /// override if your implementation can actually honor the id.
    fn create_node_with_id(
        &self,
        _id: NodeId,
        labels: Vec<String>,
        properties: serde_json::Value,
        embedding: Option<Vec<f32>>,
    ) -> Result<NodeId> {
        self.create_node(labels, properties, embedding)
    }

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

    /// Find all edges whose `edge_type` matches the given string.
    ///
    /// Returns `(EdgeId, source NodeId, target NodeId)` triples.
    /// The default implementation returns an empty vector.
    /// astraeadb-issues.md #3.
    fn find_edges_by_type(&self, _edge_type: &str) -> Result<Vec<(EdgeId, NodeId, NodeId)>> {
        Ok(Vec::new())
    }

    /// Hybrid search combining graph proximity and vector similarity.
    ///
    /// 1. BFS from `anchor` up to `max_hops` to collect candidate nodes
    /// 2. For each candidate with an embedding, compute vector distance to `query_embedding`
    /// 3. Blend: `final_score = alpha * vector_score + (1 - alpha) * graph_score`
    /// 4. Sort ascending (lower = better), return top-k
    ///
    /// `alpha`: 0.0 = pure graph proximity, 1.0 = pure vector similarity.
    fn hybrid_search(
        &self,
        _anchor: NodeId,
        _query_embedding: &[f32],
        _max_hops: usize,
        _k: usize,
        _alpha: f32,
    ) -> Result<Vec<(NodeId, f32)>> {
        Err(crate::error::AstraeaError::QueryExecution(
            "hybrid search not supported by this implementation".into(),
        ))
    }

    /// Rank neighbors of a node by semantic similarity to a concept embedding.
    ///
    /// Returns up to `k` neighbors sorted by ascending distance (most similar first).
    /// Neighbors without embeddings are excluded.
    fn semantic_neighbors(
        &self,
        _node_id: NodeId,
        _concept_embedding: &[f32],
        _direction: Direction,
        _k: usize,
    ) -> Result<Vec<(NodeId, f32)>> {
        Err(crate::error::AstraeaError::QueryExecution(
            "semantic neighbors not supported by this implementation".into(),
        ))
    }

    /// Greedy multi-hop walk toward a semantic concept.
    ///
    /// At each hop, moves to the unvisited neighbor most similar to `concept_embedding`.
    /// Returns the full path of (NodeId, distance) pairs including the start node.
    /// Stops when `max_hops` is reached or no unvisited neighbors with embeddings exist.
    fn semantic_walk(
        &self,
        _start: NodeId,
        _concept_embedding: &[f32],
        _max_hops: usize,
    ) -> Result<Vec<(NodeId, f32)>> {
        Err(crate::error::AstraeaError::QueryExecution(
            "semantic walk not supported by this implementation".into(),
        ))
    }

    // ---- Temporal query methods ----

    /// Get neighbors of a node, only including edges valid at the given timestamp.
    fn neighbors_at(
        &self,
        _node_id: NodeId,
        _direction: Direction,
        _timestamp: i64,
    ) -> Result<Vec<(EdgeId, NodeId)>> {
        Err(crate::error::AstraeaError::QueryExecution(
            "temporal neighbors not supported by this implementation".into(),
        ))
    }

    /// Breadth-first search from a starting node, only traversing edges valid at the given timestamp.
    fn bfs_at(
        &self,
        _start: NodeId,
        _max_depth: usize,
        _timestamp: i64,
    ) -> Result<Vec<(NodeId, usize)>> {
        Err(crate::error::AstraeaError::QueryExecution(
            "temporal BFS not supported by this implementation".into(),
        ))
    }

    /// Find the shortest path between two nodes, only traversing edges valid at the given timestamp.
    fn shortest_path_at(
        &self,
        _from: NodeId,
        _to: NodeId,
        _timestamp: i64,
    ) -> Result<Option<GraphPath>> {
        Err(crate::error::AstraeaError::QueryExecution(
            "temporal shortest path not supported by this implementation".into(),
        ))
    }

    /// Find the weighted shortest path between two nodes at a specific timestamp.
    fn shortest_path_weighted_at(
        &self,
        _from: NodeId,
        _to: NodeId,
        _timestamp: i64,
    ) -> Result<Option<(GraphPath, f64)>> {
        Err(crate::error::AstraeaError::QueryExecution(
            "temporal weighted shortest path not supported by this implementation".into(),
        ))
    }
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

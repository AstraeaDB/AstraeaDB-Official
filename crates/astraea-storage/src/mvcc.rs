//! MVCC (Multi-Version Concurrency Control) transaction manager.
//!
//! Provides snapshot isolation with write-write conflict detection.
//! Transactions buffer their writes and apply them atomically on commit.
//! A first-writer-wins policy is used for conflict resolution.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::*;

/// State of a single in-flight transaction.
pub struct TransactionState {
    /// Unique identifier for this transaction.
    pub id: TransactionId,
    /// The LSN at the time the transaction began (snapshot point).
    pub snapshot_lsn: Lsn,
    /// Current status of the transaction.
    pub status: TxnStatus,
    /// Buffered writes -- applied atomically on commit.
    pub write_set: Vec<WriteOp>,
    /// Tracks which entity IDs were read, for conflict detection.
    pub read_set: HashSet<u64>,
}

/// Status of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnStatus {
    Active,
    Committed,
    Aborted,
}

/// A buffered write operation within a transaction.
#[derive(Debug, Clone)]
pub enum WriteOp {
    PutNode(Node),
    DeleteNode(NodeId),
    PutEdge(Edge),
    DeleteEdge(EdgeId),
}

/// The MVCC transaction manager.
///
/// Tracks active transactions, manages snapshot isolation, and
/// detects write-write conflicts on commit using a first-writer-wins policy.
pub struct TransactionManager {
    /// Monotonically increasing transaction ID counter.
    next_txn_id: AtomicU64,
    /// All tracked transactions (active, committed, or aborted until GC).
    active_txns: RwLock<HashMap<TransactionId, TransactionState>>,
    /// Set of entity IDs currently locked for write by active transactions.
    /// Maps entity_id -> TransactionId of the lock holder.
    write_locks: RwLock<HashMap<u64, TransactionId>>,
}

impl TransactionManager {
    /// Create a new transaction manager with no active transactions.
    pub fn new() -> Self {
        Self {
            next_txn_id: AtomicU64::new(1),
            active_txns: RwLock::new(HashMap::new()),
            write_locks: RwLock::new(HashMap::new()),
        }
    }

    /// Begin a new transaction. Returns the assigned transaction ID.
    ///
    /// The `current_lsn` parameter captures the WAL position at the time of
    /// the snapshot, enabling snapshot isolation reads.
    pub fn begin(&self, current_lsn: Lsn) -> TransactionId {
        let id = TransactionId(self.next_txn_id.fetch_add(1, Ordering::SeqCst));
        let state = TransactionState {
            id,
            snapshot_lsn: current_lsn,
            status: TxnStatus::Active,
            write_set: Vec::new(),
            read_set: HashSet::new(),
        };
        self.active_txns.write().insert(id, state);
        id
    }

    /// Buffer a write operation in the transaction.
    ///
    /// Returns an error if another active transaction already holds a write
    /// lock on the same entity (first-writer-wins conflict detection).
    pub fn buffer_write(&self, txn_id: TransactionId, entity_id: u64, op: WriteOp) -> Result<()> {
        // Check for write-write conflicts (first-writer-wins).
        let mut locks = self.write_locks.write();
        if let Some(&owner) = locks.get(&entity_id) {
            if owner != txn_id {
                return Err(AstraeaError::WriteConflict(entity_id));
            }
        }
        locks.insert(entity_id, txn_id);
        drop(locks);

        let mut txns = self.active_txns.write();
        if let Some(state) = txns.get_mut(&txn_id) {
            if state.status != TxnStatus::Active {
                return Err(AstraeaError::TransactionNotActive);
            }
            state.write_set.push(op);
        } else {
            return Err(AstraeaError::TransactionNotActive);
        }
        Ok(())
    }

    /// Record a read for conflict detection (for serializable isolation).
    pub fn record_read(&self, txn_id: TransactionId, entity_id: u64) {
        let mut txns = self.active_txns.write();
        if let Some(state) = txns.get_mut(&txn_id) {
            state.read_set.insert(entity_id);
        }
    }

    /// Commit a transaction. Returns the write set for the caller to apply.
    ///
    /// Releases all write locks held by this transaction and marks the
    /// transaction as committed.
    pub fn commit(&self, txn_id: TransactionId) -> Result<Vec<WriteOp>> {
        let mut txns = self.active_txns.write();
        let state = txns
            .get_mut(&txn_id)
            .ok_or(AstraeaError::TransactionNotActive)?;
        if state.status != TxnStatus::Active {
            return Err(AstraeaError::TransactionNotActive);
        }
        state.status = TxnStatus::Committed;
        let write_set = std::mem::take(&mut state.write_set);
        drop(txns);

        // Release write locks held by this transaction.
        let mut locks = self.write_locks.write();
        locks.retain(|_, owner| *owner != txn_id);

        Ok(write_set)
    }

    /// Abort a transaction. Discards all buffered writes and releases locks.
    pub fn abort(&self, txn_id: TransactionId) -> Result<()> {
        let mut txns = self.active_txns.write();
        if let Some(state) = txns.get_mut(&txn_id) {
            state.status = TxnStatus::Aborted;
            state.write_set.clear();
        }
        drop(txns);

        let mut locks = self.write_locks.write();
        locks.retain(|_, owner| *owner != txn_id);

        Ok(())
    }

    /// Check if a transaction is still active.
    pub fn is_active(&self, txn_id: TransactionId) -> bool {
        self.active_txns
            .read()
            .get(&txn_id)
            .map(|s| s.status == TxnStatus::Active)
            .unwrap_or(false)
    }

    /// Number of currently active transactions.
    pub fn active_count(&self) -> usize {
        self.active_txns
            .read()
            .values()
            .filter(|s| s.status == TxnStatus::Active)
            .count()
    }

    /// Clean up completed (committed/aborted) transactions that are no longer needed.
    pub fn gc(&self) {
        let mut txns = self.active_txns.write();
        txns.retain(|_, state| state.status == TxnStatus::Active);
    }
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_node(id: u64) -> Node {
        Node {
            id: NodeId(id),
            labels: vec!["Person".to_string()],
            properties: serde_json::json!({"name": "Test", "id": id}),
            embedding: None,
        }
    }

    fn make_test_edge(id: u64, src: u64, tgt: u64) -> Edge {
        Edge {
            id: EdgeId(id),
            source: NodeId(src),
            target: NodeId(tgt),
            edge_type: "KNOWS".to_string(),
            properties: serde_json::json!({}),
            weight: 1.0,
            validity: ValidityInterval::always(),
        }
    }

    #[test]
    fn test_begin_and_commit() {
        let mgr = TransactionManager::new();

        // Begin a transaction.
        let txn = mgr.begin(Lsn(0));
        assert!(mgr.is_active(txn));

        // Buffer some writes.
        let node = make_test_node(1);
        let edge = make_test_edge(100, 1, 2);
        mgr.buffer_write(txn, node.id.0, WriteOp::PutNode(node))
            .unwrap();
        mgr.buffer_write(txn, edge.id.0 + 1_000_000, WriteOp::PutEdge(edge))
            .unwrap();

        // Commit and verify the write set is returned.
        let write_set = mgr.commit(txn).unwrap();
        assert_eq!(write_set.len(), 2);
        assert!(matches!(write_set[0], WriteOp::PutNode(_)));
        assert!(matches!(write_set[1], WriteOp::PutEdge(_)));

        // Transaction should no longer be active.
        assert!(!mgr.is_active(txn));
    }

    #[test]
    fn test_begin_and_abort() {
        let mgr = TransactionManager::new();

        let txn = mgr.begin(Lsn(0));
        let node = make_test_node(1);
        mgr.buffer_write(txn, node.id.0, WriteOp::PutNode(node))
            .unwrap();

        // Abort discards all writes.
        mgr.abort(txn).unwrap();
        assert!(!mgr.is_active(txn));

        // The write set should have been cleared.
        // Attempting to commit should fail because the txn is aborted.
        let result = mgr.commit(txn);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_conflict() {
        let mgr = TransactionManager::new();

        let txn1 = mgr.begin(Lsn(0));
        let txn2 = mgr.begin(Lsn(0));

        // txn1 writes entity 42.
        let node = make_test_node(42);
        mgr.buffer_write(txn1, 42, WriteOp::PutNode(node.clone()))
            .unwrap();

        // txn2 tries to write the same entity -- should get a conflict error.
        let result = mgr.buffer_write(txn2, 42, WriteOp::PutNode(node));
        assert!(result.is_err());
        match result.unwrap_err() {
            AstraeaError::WriteConflict(eid) => assert_eq!(eid, 42),
            other => panic!("expected WriteConflict, got {:?}", other),
        }
    }

    #[test]
    fn test_active_count() {
        let mgr = TransactionManager::new();
        assert_eq!(mgr.active_count(), 0);

        let txn1 = mgr.begin(Lsn(0));
        assert_eq!(mgr.active_count(), 1);

        let txn2 = mgr.begin(Lsn(0));
        assert_eq!(mgr.active_count(), 2);

        mgr.commit(txn1).unwrap();
        assert_eq!(mgr.active_count(), 1);

        mgr.abort(txn2).unwrap();
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn test_gc() {
        let mgr = TransactionManager::new();

        let txn1 = mgr.begin(Lsn(0));
        let txn2 = mgr.begin(Lsn(0));
        let txn3 = mgr.begin(Lsn(0));

        mgr.commit(txn1).unwrap();
        mgr.abort(txn2).unwrap();
        // txn3 is still active.

        // Before GC, all three transactions are tracked.
        assert_eq!(mgr.active_txns.read().len(), 3);

        mgr.gc();

        // After GC, only the active transaction remains.
        assert_eq!(mgr.active_txns.read().len(), 1);
        assert!(mgr.is_active(txn3));
    }

    #[test]
    fn test_commit_inactive() {
        let mgr = TransactionManager::new();
        let txn = mgr.begin(Lsn(0));

        // Commit once -- should succeed.
        mgr.commit(txn).unwrap();

        // Commit again -- transaction is no longer active.
        let result = mgr.commit(txn);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AstraeaError::TransactionNotActive
        ));
    }

    #[test]
    fn test_record_read() {
        let mgr = TransactionManager::new();
        let txn = mgr.begin(Lsn(0));

        mgr.record_read(txn, 10);
        mgr.record_read(txn, 20);
        mgr.record_read(txn, 10); // duplicate, should be idempotent

        let txns = mgr.active_txns.read();
        let state = txns.get(&txn).unwrap();
        assert_eq!(state.read_set.len(), 2);
        assert!(state.read_set.contains(&10));
        assert!(state.read_set.contains(&20));
    }

    #[test]
    fn test_write_lock_released_after_commit() {
        let mgr = TransactionManager::new();

        let txn1 = mgr.begin(Lsn(0));
        let node = make_test_node(1);
        mgr.buffer_write(txn1, 1, WriteOp::PutNode(node.clone()))
            .unwrap();

        // Commit txn1, releasing the lock.
        mgr.commit(txn1).unwrap();

        // Now txn2 can write the same entity.
        let txn2 = mgr.begin(Lsn(0));
        mgr.buffer_write(txn2, 1, WriteOp::PutNode(node)).unwrap();
        mgr.commit(txn2).unwrap();
    }

    #[test]
    fn test_write_lock_released_after_abort() {
        let mgr = TransactionManager::new();

        let txn1 = mgr.begin(Lsn(0));
        let node = make_test_node(1);
        mgr.buffer_write(txn1, 1, WriteOp::PutNode(node.clone()))
            .unwrap();

        // Abort txn1, releasing the lock.
        mgr.abort(txn1).unwrap();

        // Now txn2 can write the same entity.
        let txn2 = mgr.begin(Lsn(0));
        mgr.buffer_write(txn2, 1, WriteOp::PutNode(node)).unwrap();
        mgr.commit(txn2).unwrap();
    }

    #[test]
    fn test_same_txn_can_write_same_entity_twice() {
        let mgr = TransactionManager::new();
        let txn = mgr.begin(Lsn(0));

        let node = make_test_node(1);
        mgr.buffer_write(txn, 1, WriteOp::PutNode(node.clone()))
            .unwrap();
        // Same transaction writing same entity again should succeed.
        mgr.buffer_write(txn, 1, WriteOp::PutNode(node)).unwrap();

        let write_set = mgr.commit(txn).unwrap();
        assert_eq!(write_set.len(), 2);
    }

    #[test]
    fn test_delete_operations() {
        let mgr = TransactionManager::new();
        let txn = mgr.begin(Lsn(0));

        mgr.buffer_write(txn, 1, WriteOp::DeleteNode(NodeId(1)))
            .unwrap();
        mgr.buffer_write(txn, 1_000_100, WriteOp::DeleteEdge(EdgeId(100)))
            .unwrap();

        let write_set = mgr.commit(txn).unwrap();
        assert_eq!(write_set.len(), 2);
        assert!(matches!(write_set[0], WriteOp::DeleteNode(NodeId(1))));
        assert!(matches!(write_set[1], WriteOp::DeleteEdge(EdgeId(100))));
    }
}

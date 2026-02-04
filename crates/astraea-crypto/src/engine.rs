use std::collections::HashMap;

use astraea_core::types::NodeId;
use crate::encrypted::{EncryptedLabel, EncryptedNode};

/// An engine that can perform basic operations on encrypted data.
///
/// The server-side engine NEVER sees plaintext. All comparisons are
/// performed on deterministic tags or encrypted values. This demonstrates
/// the core principle of homomorphic / searchable encryption: the server
/// can execute queries (e.g., "find nodes with label X") without ever
/// learning what X actually is.
///
/// In a full deployment, this would be backed by persistent storage
/// (the AstraeaDB tiered storage engine) rather than an in-memory HashMap.
pub struct EncryptedQueryEngine {
    encrypted_nodes: HashMap<NodeId, EncryptedNode>,
}

impl EncryptedQueryEngine {
    /// Create a new empty encrypted query engine.
    pub fn new() -> Self {
        EncryptedQueryEngine {
            encrypted_nodes: HashMap::new(),
        }
    }

    /// Store an encrypted node. The server never sees plaintext.
    ///
    /// If a node with the same ID already exists, it is overwritten.
    pub fn insert(&mut self, node: EncryptedNode) {
        self.encrypted_nodes.insert(node.id, node);
    }

    /// Find nodes whose encrypted labels match the query label.
    ///
    /// The server compares deterministic tags without decryption.
    /// Returns the IDs of all matching nodes.
    pub fn find_by_encrypted_label(&self, query: &EncryptedLabel) -> Vec<NodeId> {
        self.encrypted_nodes
            .values()
            .filter(|node| node.has_encrypted_label(query))
            .map(|node| node.id)
            .collect()
    }

    /// Remove an encrypted node by ID, returning it if it existed.
    pub fn remove(&mut self, id: NodeId) -> Option<EncryptedNode> {
        self.encrypted_nodes.remove(&id)
    }

    /// Get a reference to an encrypted node by ID.
    pub fn get(&self, id: NodeId) -> Option<&EncryptedNode> {
        self.encrypted_nodes.get(&id)
    }

    /// Number of stored encrypted nodes.
    pub fn len(&self) -> usize {
        self.encrypted_nodes.len()
    }

    /// Whether the engine contains no encrypted nodes.
    pub fn is_empty(&self) -> bool {
        self.encrypted_nodes.is_empty()
    }

    /// Return all stored node IDs.
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.encrypted_nodes.keys().copied().collect()
    }
}

impl Default for EncryptedQueryEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::types::Node;
    use crate::encrypted::{EncryptedLabel, EncryptedNode};
    use crate::keys::KeyPair;

    fn make_node(id: u64, labels: &[&str], name: &str) -> Node {
        Node {
            id: NodeId(id),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            properties: serde_json::json!({ "name": name }),
            embedding: None,
        }
    }

    #[test]
    fn test_engine_new_is_empty() {
        let engine = EncryptedQueryEngine::new();
        assert!(engine.is_empty());
        assert_eq!(engine.len(), 0);
    }

    #[test]
    fn test_engine_insert_and_get() {
        let kp = KeyPair::generate();
        let mut engine = EncryptedQueryEngine::new();

        let node = make_node(1, &["Person"], "Alice");
        let encrypted = EncryptedNode::from_node(&node, &kp.secret_key);
        engine.insert(encrypted);

        assert_eq!(engine.len(), 1);
        assert!(!engine.is_empty());

        let retrieved = engine.get(NodeId(1));
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, NodeId(1));
    }

    #[test]
    fn test_engine_get_nonexistent() {
        let engine = EncryptedQueryEngine::new();
        assert!(engine.get(NodeId(999)).is_none());
    }

    #[test]
    fn test_engine_find_by_encrypted_label() {
        let kp = KeyPair::generate();
        let mut engine = EncryptedQueryEngine::new();

        let alice = make_node(1, &["Person", "Employee"], "Alice");
        let bob = make_node(2, &["Person"], "Bob");
        let acme = make_node(3, &["Company"], "ACME Corp");

        engine.insert(EncryptedNode::from_node(&alice, &kp.secret_key));
        engine.insert(EncryptedNode::from_node(&bob, &kp.secret_key));
        engine.insert(EncryptedNode::from_node(&acme, &kp.secret_key));

        assert_eq!(engine.len(), 3);

        // Search for "Person" label.
        let query_person = EncryptedLabel::encrypt("Person", &kp.secret_key);
        let mut person_results = engine.find_by_encrypted_label(&query_person);
        person_results.sort();
        assert_eq!(person_results, vec![NodeId(1), NodeId(2)]);

        // Search for "Company" label.
        let query_company = EncryptedLabel::encrypt("Company", &kp.secret_key);
        let company_results = engine.find_by_encrypted_label(&query_company);
        assert_eq!(company_results, vec![NodeId(3)]);

        // Search for "Employee" label.
        let query_employee = EncryptedLabel::encrypt("Employee", &kp.secret_key);
        let employee_results = engine.find_by_encrypted_label(&query_employee);
        assert_eq!(employee_results, vec![NodeId(1)]);
    }

    #[test]
    fn test_engine_find_by_label_no_match() {
        let kp = KeyPair::generate();
        let mut engine = EncryptedQueryEngine::new();

        let node = make_node(1, &["Person"], "Alice");
        engine.insert(EncryptedNode::from_node(&node, &kp.secret_key));

        let query = EncryptedLabel::encrypt("NonExistent", &kp.secret_key);
        let results = engine.find_by_encrypted_label(&query);
        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_find_different_key_no_match() {
        let kp1 = KeyPair::generate();
        let kp2 = KeyPair::generate();
        let mut engine = EncryptedQueryEngine::new();

        // Insert with key 1.
        let node = make_node(1, &["Person"], "Alice");
        engine.insert(EncryptedNode::from_node(&node, &kp1.secret_key));

        // Search with key 2 -- should NOT match even for same label text.
        let query = EncryptedLabel::encrypt("Person", &kp2.secret_key);
        let results = engine.find_by_encrypted_label(&query);
        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_remove() {
        let kp = KeyPair::generate();
        let mut engine = EncryptedQueryEngine::new();

        let node = make_node(1, &["Person"], "Alice");
        engine.insert(EncryptedNode::from_node(&node, &kp.secret_key));
        assert_eq!(engine.len(), 1);

        let removed = engine.remove(NodeId(1));
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, NodeId(1));
        assert!(engine.is_empty());

        // Removing again should return None.
        assert!(engine.remove(NodeId(1)).is_none());
    }

    #[test]
    fn test_engine_overwrite() {
        let kp = KeyPair::generate();
        let mut engine = EncryptedQueryEngine::new();

        let node_v1 = make_node(1, &["Person"], "Alice");
        engine.insert(EncryptedNode::from_node(&node_v1, &kp.secret_key));

        let node_v2 = make_node(1, &["Company"], "ACME");
        engine.insert(EncryptedNode::from_node(&node_v2, &kp.secret_key));

        // Should still have only one node.
        assert_eq!(engine.len(), 1);

        // Should match "Company" now, not "Person".
        let query_company = EncryptedLabel::encrypt("Company", &kp.secret_key);
        let query_person = EncryptedLabel::encrypt("Person", &kp.secret_key);
        assert_eq!(
            engine.find_by_encrypted_label(&query_company),
            vec![NodeId(1)]
        );
        assert!(engine.find_by_encrypted_label(&query_person).is_empty());
    }

    #[test]
    fn test_engine_node_ids() {
        let kp = KeyPair::generate();
        let mut engine = EncryptedQueryEngine::new();

        engine.insert(EncryptedNode::from_node(
            &make_node(10, &["A"], "a"),
            &kp.secret_key,
        ));
        engine.insert(EncryptedNode::from_node(
            &make_node(20, &["B"], "b"),
            &kp.secret_key,
        ));
        engine.insert(EncryptedNode::from_node(
            &make_node(30, &["C"], "c"),
            &kp.secret_key,
        ));

        let mut ids = engine.node_ids();
        ids.sort();
        assert_eq!(ids, vec![NodeId(10), NodeId(20), NodeId(30)]);
    }

    #[test]
    fn test_engine_default() {
        let engine = EncryptedQueryEngine::default();
        assert!(engine.is_empty());
    }

    #[test]
    fn test_full_workflow_encrypt_store_search_decrypt() {
        // Simulates the full client-server workflow:
        // 1. Client encrypts data and sends to server.
        // 2. Server stores encrypted data (never sees plaintext).
        // 3. Client creates encrypted query and sends to server.
        // 4. Server finds matching nodes and returns encrypted results.
        // 5. Client decrypts results.

        let client_keys = KeyPair::generate();

        // --- Client side: encrypt nodes ---
        let alice = Node {
            id: NodeId(1),
            labels: vec!["Person".to_string(), "Developer".to_string()],
            properties: serde_json::json!({"name": "Alice", "clearance": "top-secret"}),
            embedding: None,
        };
        let bob = Node {
            id: NodeId(2),
            labels: vec!["Person".to_string(), "Manager".to_string()],
            properties: serde_json::json!({"name": "Bob", "clearance": "secret"}),
            embedding: None,
        };
        let acme = Node {
            id: NodeId(3),
            labels: vec!["Organization".to_string()],
            properties: serde_json::json!({"name": "ACME Corp"}),
            embedding: None,
        };

        let enc_alice = EncryptedNode::from_node(&alice, &client_keys.secret_key);
        let enc_bob = EncryptedNode::from_node(&bob, &client_keys.secret_key);
        let enc_acme = EncryptedNode::from_node(&acme, &client_keys.secret_key);

        // --- Server side: store and query ---
        let mut server_engine = EncryptedQueryEngine::new();
        server_engine.insert(enc_alice);
        server_engine.insert(enc_bob);
        server_engine.insert(enc_acme);

        // Client creates an encrypted query for "Person" nodes.
        let query = EncryptedLabel::encrypt("Person", &client_keys.secret_key);

        // Server executes the query without seeing "Person".
        let mut matching_ids = server_engine.find_by_encrypted_label(&query);
        matching_ids.sort();
        assert_eq!(matching_ids, vec![NodeId(1), NodeId(2)]);

        // --- Client side: decrypt results ---
        for id in &matching_ids {
            let enc_node = server_engine.get(*id).unwrap();
            let decrypted = enc_node.to_node(&client_keys.secret_key);
            assert!(decrypted.labels.contains(&"Person".to_string()));
            assert!(decrypted.properties["name"].is_string());
        }
    }
}

use rand::Rng;
use serde::{Deserialize, Serialize};

use astraea_core::types::{Node, NodeId};
use crate::keys::SecretKey;

/// An encrypted string value (label or property value).
///
/// Contains both the ciphertext and a random nonce that ensures
/// different encryptions of the same plaintext produce different
/// ciphertexts (semantic security).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedValue {
    pub ciphertext: Vec<u8>,
    /// Random nonce mixed in before encryption for uniqueness,
    /// even when the same plaintext is encrypted multiple times.
    pub nonce: Vec<u8>,
}

/// An encrypted label that supports equality comparison under encryption.
///
/// Uses a dual-encryption approach:
/// - A deterministic tag allows the server to check equality without decryption.
/// - A randomized encrypted value protects confidentiality and allows decryption
///   by the key holder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedLabel {
    /// Deterministic encryption of the label (allows equality checks).
    /// The server can compare these tags without knowing the plaintext.
    pub deterministic_tag: Vec<u8>,
    /// Randomized encryption of the label (for security and decryption).
    pub encrypted_value: EncryptedValue,
}

/// An encrypted node where labels and property values are encrypted.
///
/// The node ID remains in plaintext for indexing, but all semantic
/// content (labels, properties) is encrypted. The server can still
/// perform structural graph operations (traversals, adjacency) while
/// the actual data remains opaque.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedNode {
    /// Node ID remains unencrypted for graph structure operations.
    pub id: NodeId,
    /// Each label is individually encrypted, supporting per-label matching.
    pub encrypted_labels: Vec<EncryptedLabel>,
    /// All properties are encrypted as a single JSON blob.
    pub encrypted_properties: EncryptedValue,
}

impl EncryptedValue {
    /// Encrypt a plaintext byte slice with a random nonce for semantic security.
    pub fn encrypt(plaintext: &[u8], key: &SecretKey) -> Self {
        let mut rng = rand::thread_rng();
        let mut nonce = vec![0u8; 16];
        rng.fill(&mut nonce[..]);

        // Prepend the nonce to the plaintext before encryption.
        // This ensures that even identical plaintexts produce different ciphertexts.
        let mut combined = Vec::with_capacity(nonce.len() + plaintext.len());
        combined.extend_from_slice(&nonce);
        combined.extend_from_slice(plaintext);

        let ciphertext = key.encrypt_bytes(&combined);

        EncryptedValue { ciphertext, nonce }
    }

    /// Decrypt the value back to plaintext bytes.
    pub fn decrypt(&self, key: &SecretKey) -> Vec<u8> {
        let combined = key.decrypt_bytes(&self.ciphertext);
        // Strip the prepended nonce (first 16 bytes) to recover original plaintext.
        if combined.len() > self.nonce.len() {
            combined[self.nonce.len()..].to_vec()
        } else {
            Vec::new()
        }
    }
}

impl EncryptedLabel {
    /// Create an encrypted label from a plaintext label string.
    ///
    /// The deterministic tag is computed so that the server can compare
    /// encrypted labels for equality without decryption. The encrypted
    /// value uses randomized encryption for confidentiality.
    pub fn encrypt(label: &str, key: &SecretKey) -> Self {
        let deterministic_tag = key.deterministic_tag(label.as_bytes());
        let encrypted_value = EncryptedValue::encrypt(label.as_bytes(), key);

        EncryptedLabel {
            deterministic_tag,
            encrypted_value,
        }
    }

    /// Check if two encrypted labels represent the same plaintext.
    ///
    /// This can be performed by the server without knowing the plaintext,
    /// by comparing the deterministic tags.
    pub fn matches(&self, other: &EncryptedLabel) -> bool {
        self.deterministic_tag == other.deterministic_tag
    }

    /// Decrypt to recover the original label string.
    pub fn decrypt(&self, key: &SecretKey) -> String {
        let bytes = self.encrypted_value.decrypt(key);
        String::from_utf8(bytes).unwrap_or_default()
    }
}

impl EncryptedNode {
    /// Encrypt a node. Labels and properties are encrypted; the ID is preserved.
    ///
    /// Note: The embedding vector is NOT encrypted in this demonstration.
    /// Encrypting floating-point vectors for use with similarity search
    /// requires specialized techniques (e.g., secure multi-party computation)
    /// that are beyond the scope of this foundation crate.
    pub fn from_node(node: &Node, key: &SecretKey) -> Self {
        let encrypted_labels = node
            .labels
            .iter()
            .map(|label| EncryptedLabel::encrypt(label, key))
            .collect();

        let properties_json = serde_json::to_string(&node.properties)
            .unwrap_or_else(|_| "{}".to_string());
        let encrypted_properties =
            EncryptedValue::encrypt(properties_json.as_bytes(), key);

        EncryptedNode {
            id: node.id,
            encrypted_labels,
            encrypted_properties,
        }
    }

    /// Decrypt back to a Node.
    ///
    /// The embedding is set to `None` since it is not stored in the
    /// encrypted representation.
    pub fn to_node(&self, key: &SecretKey) -> Node {
        let labels: Vec<String> = self
            .encrypted_labels
            .iter()
            .map(|el| el.decrypt(key))
            .collect();

        let properties_bytes = self.encrypted_properties.decrypt(key);
        let properties_str =
            String::from_utf8(properties_bytes).unwrap_or_else(|_| "{}".to_string());
        let properties: serde_json::Value =
            serde_json::from_str(&properties_str).unwrap_or(serde_json::Value::Null);

        Node {
            id: self.id,
            labels,
            properties,
            embedding: None,
        }
    }

    /// Check if this encrypted node has a label matching the given encrypted label.
    ///
    /// This comparison is performed entirely on encrypted data -- the server
    /// never sees the plaintext labels.
    pub fn has_encrypted_label(&self, query: &EncryptedLabel) -> bool {
        self.encrypted_labels.iter().any(|el| el.matches(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyPair;

    #[test]
    fn test_encrypted_value_roundtrip() {
        let kp = KeyPair::generate();
        let plaintext = b"test data for encryption";
        let ev = EncryptedValue::encrypt(plaintext, &kp.secret_key);
        // Ciphertext should not be empty.
        assert!(!ev.ciphertext.is_empty());
        // Nonce should be 16 bytes.
        assert_eq!(ev.nonce.len(), 16);
        // Decryption should recover the original.
        let decrypted = ev.decrypt(&kp.secret_key);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypted_value_randomized() {
        let kp = KeyPair::generate();
        let plaintext = b"same plaintext";
        let ev1 = EncryptedValue::encrypt(plaintext, &kp.secret_key);
        let ev2 = EncryptedValue::encrypt(plaintext, &kp.secret_key);
        // Two encryptions of the same plaintext should produce different ciphertexts
        // (due to different random nonces).
        assert_ne!(ev1.ciphertext, ev2.ciphertext);
        // But both should decrypt to the same value.
        assert_eq!(ev1.decrypt(&kp.secret_key), ev2.decrypt(&kp.secret_key));
    }

    #[test]
    fn test_encrypted_label_roundtrip() {
        let kp = KeyPair::generate();
        let label = "Person";
        let el = EncryptedLabel::encrypt(label, &kp.secret_key);
        let decrypted = el.decrypt(&kp.secret_key);
        assert_eq!(decrypted, label);
    }

    #[test]
    fn test_encrypted_label_same_plaintext_matches() {
        let kp = KeyPair::generate();
        let el1 = EncryptedLabel::encrypt("Person", &kp.secret_key);
        let el2 = EncryptedLabel::encrypt("Person", &kp.secret_key);
        // Same plaintext with same key should produce matching tags.
        assert!(el1.matches(&el2));
        // Even though the encrypted values differ (randomized).
        assert_ne!(
            el1.encrypted_value.ciphertext,
            el2.encrypted_value.ciphertext
        );
    }

    #[test]
    fn test_encrypted_label_different_plaintext_no_match() {
        let kp = KeyPair::generate();
        let el1 = EncryptedLabel::encrypt("Person", &kp.secret_key);
        let el2 = EncryptedLabel::encrypt("Company", &kp.secret_key);
        assert!(!el1.matches(&el2));
    }

    #[test]
    fn test_encrypted_label_different_keys_no_match() {
        let kp1 = KeyPair::generate();
        let kp2 = KeyPair::generate();
        let el1 = EncryptedLabel::encrypt("Person", &kp1.secret_key);
        let el2 = EncryptedLabel::encrypt("Person", &kp2.secret_key);
        // Same plaintext, different keys: should NOT match.
        assert!(!el1.matches(&el2));
    }

    #[test]
    fn test_encrypted_node_roundtrip() {
        let kp = KeyPair::generate();
        let node = Node {
            id: NodeId(42),
            labels: vec!["Person".to_string(), "Employee".to_string()],
            properties: serde_json::json!({
                "name": "Alice",
                "age": 30,
                "active": true
            }),
            embedding: Some(vec![1.0, 2.0, 3.0]),
        };

        let encrypted = EncryptedNode::from_node(&node, &kp.secret_key);

        // ID should be preserved in plaintext.
        assert_eq!(encrypted.id, NodeId(42));
        // Should have 2 encrypted labels.
        assert_eq!(encrypted.encrypted_labels.len(), 2);

        let decrypted = encrypted.to_node(&kp.secret_key);
        assert_eq!(decrypted.id, NodeId(42));
        assert_eq!(decrypted.labels, vec!["Person", "Employee"]);
        assert_eq!(decrypted.properties["name"], "Alice");
        assert_eq!(decrypted.properties["age"], 30);
        assert_eq!(decrypted.properties["active"], true);
        // Embedding is not preserved through encryption.
        assert!(decrypted.embedding.is_none());
    }

    #[test]
    fn test_encrypted_node_has_label() {
        let kp = KeyPair::generate();
        let node = Node {
            id: NodeId(1),
            labels: vec!["Person".to_string(), "Employee".to_string()],
            properties: serde_json::json!({}),
            embedding: None,
        };

        let encrypted = EncryptedNode::from_node(&node, &kp.secret_key);
        let query_person = EncryptedLabel::encrypt("Person", &kp.secret_key);
        let query_company = EncryptedLabel::encrypt("Company", &kp.secret_key);
        let query_employee = EncryptedLabel::encrypt("Employee", &kp.secret_key);

        assert!(encrypted.has_encrypted_label(&query_person));
        assert!(encrypted.has_encrypted_label(&query_employee));
        assert!(!encrypted.has_encrypted_label(&query_company));
    }

    #[test]
    fn test_encrypted_node_empty_properties() {
        let kp = KeyPair::generate();
        let node = Node {
            id: NodeId(99),
            labels: vec![],
            properties: serde_json::json!({}),
            embedding: None,
        };

        let encrypted = EncryptedNode::from_node(&node, &kp.secret_key);
        let decrypted = encrypted.to_node(&kp.secret_key);

        assert_eq!(decrypted.id, NodeId(99));
        assert!(decrypted.labels.is_empty());
        assert_eq!(decrypted.properties, serde_json::json!({}));
    }

    #[test]
    fn test_encrypted_node_complex_properties() {
        let kp = KeyPair::generate();
        let node = Node {
            id: NodeId(7),
            labels: vec!["Document".to_string()],
            properties: serde_json::json!({
                "title": "Secret Report",
                "tags": ["classified", "internal"],
                "metadata": {
                    "pages": 42,
                    "author": "Bob"
                }
            }),
            embedding: None,
        };

        let encrypted = EncryptedNode::from_node(&node, &kp.secret_key);
        let decrypted = encrypted.to_node(&kp.secret_key);

        assert_eq!(decrypted.properties["title"], "Secret Report");
        assert_eq!(decrypted.properties["tags"][0], "classified");
        assert_eq!(decrypted.properties["metadata"]["pages"], 42);
        assert_eq!(decrypted.properties["metadata"]["author"], "Bob");
    }
}

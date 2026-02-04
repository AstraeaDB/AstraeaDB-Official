//! Authentication and Role-Based Access Control (RBAC) for AstraeaDB.
//!
//! Supports API key authentication with three roles:
//! - `Admin`: full access to all operations
//! - `Writer`: read + write operations (no admin)
//! - `Reader`: read-only operations

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// User roles with increasing privilege levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    /// Read-only access: get, query, search, traverse.
    Reader,
    /// Read + write: create, update, delete nodes/edges.
    Writer,
    /// Full access: all operations including server management.
    Admin,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Reader => write!(f, "reader"),
            Role::Writer => write!(f, "writer"),
            Role::Admin => write!(f, "admin"),
        }
    }
}

/// An API key entry with associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    /// The API key string.
    pub key: String,
    /// The role assigned to this key.
    pub role: Role,
    /// Human-readable description (e.g., "CI pipeline key").
    pub description: String,
    /// Whether this key is currently active.
    pub active: bool,
}

/// An entry in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp of the operation (epoch seconds).
    pub timestamp: u64,
    /// The API key used (truncated for security).
    pub api_key_prefix: String,
    /// The role of the authenticated user.
    pub role: Role,
    /// The operation type (e.g., "CreateNode", "Query").
    pub operation: String,
    /// Whether the operation was allowed.
    pub allowed: bool,
}

/// Authentication and authorization manager.
pub struct AuthManager {
    /// Map from API key string to entry.
    keys: RwLock<HashMap<String, ApiKeyEntry>>,
    /// Whether authentication is enabled. If false, all requests are allowed.
    enabled: bool,
    /// Audit log (bounded circular buffer).
    audit_log: RwLock<Vec<AuditEntry>>,
    /// Maximum audit log entries before truncation.
    max_audit_entries: usize,
}

impl AuthManager {
    /// Create a new auth manager with authentication disabled.
    pub fn disabled() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            enabled: false,
            audit_log: RwLock::new(Vec::new()),
            max_audit_entries: 10000,
        }
    }

    /// Create a new auth manager with authentication enabled.
    pub fn new(keys: Vec<ApiKeyEntry>) -> Self {
        let key_map: HashMap<String, ApiKeyEntry> =
            keys.into_iter().map(|k| (k.key.clone(), k)).collect();
        Self {
            keys: RwLock::new(key_map),
            enabled: true,
            audit_log: RwLock::new(Vec::new()),
            max_audit_entries: 10000,
        }
    }

    /// Check if authentication is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Authenticate an API key. Returns the role if valid.
    pub fn authenticate(&self, api_key: &str) -> Option<Role> {
        if !self.enabled {
            return Some(Role::Admin); // no auth = full access
        }

        let keys = self.keys.read().unwrap();
        keys.get(api_key)
            .filter(|entry| entry.active)
            .map(|entry| entry.role)
    }

    /// Check if a role is authorized for a given operation.
    pub fn authorize(role: Role, operation: &str) -> bool {
        match role {
            Role::Admin => true,
            Role::Writer => !Self::is_admin_operation(operation),
            Role::Reader => Self::is_read_operation(operation),
        }
    }

    /// Check if an operation is read-only.
    fn is_read_operation(operation: &str) -> bool {
        matches!(
            operation,
            "GetNode"
                | "GetEdge"
                | "Neighbors"
                | "NeighborsAt"
                | "Bfs"
                | "BfsAt"
                | "ShortestPath"
                | "ShortestPathAt"
                | "VectorSearch"
                | "HybridSearch"
                | "SemanticNeighbors"
                | "SemanticWalk"
                | "Query"
                | "ExtractSubgraph"
                | "GraphRag"
                | "Ping"
        )
    }

    /// Check if an operation requires admin role.
    fn is_admin_operation(_operation: &str) -> bool {
        // Currently no admin-only operations beyond normal CRUD.
        // This is a hook for future server management commands.
        false
    }

    /// Record an operation in the audit log.
    pub fn audit(&self, api_key: &str, role: Role, operation: &str, allowed: bool) {
        let entry = AuditEntry {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            api_key_prefix: if api_key.len() >= 8 {
                format!("{}...", &api_key[..8])
            } else {
                api_key.to_string()
            },
            role,
            operation: operation.to_string(),
            allowed,
        };

        let mut log = self.audit_log.write().unwrap();
        log.push(entry);
        if log.len() > self.max_audit_entries {
            // Remove oldest 10% to avoid constant shifting.
            let drain_count = self.max_audit_entries / 10;
            log.drain(..drain_count);
        }
    }

    /// Get recent audit log entries.
    pub fn recent_audit(&self, count: usize) -> Vec<AuditEntry> {
        let log = self.audit_log.read().unwrap();
        log.iter().rev().take(count).cloned().collect()
    }

    /// Add a new API key.
    pub fn add_key(&self, entry: ApiKeyEntry) {
        let mut keys = self.keys.write().unwrap();
        keys.insert(entry.key.clone(), entry);
    }

    /// Revoke (deactivate) an API key.
    pub fn revoke_key(&self, api_key: &str) -> bool {
        let mut keys = self.keys.write().unwrap();
        if let Some(entry) = keys.get_mut(api_key) {
            entry.active = false;
            true
        } else {
            false
        }
    }

    /// Get the operation name from a request type string.
    pub fn operation_name(request_json: &str) -> &str {
        // Quick extraction of the "type" field from JSON without full parsing.
        if let Some(start) = request_json.find("\"type\":\"") {
            let rest = &request_json[start + 8..];
            if let Some(end) = rest.find('"') {
                return &rest[..end];
            }
        }
        "Unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keys() -> Vec<ApiKeyEntry> {
        vec![
            ApiKeyEntry {
                key: "admin-key-12345678".into(),
                role: Role::Admin,
                description: "Admin key".into(),
                active: true,
            },
            ApiKeyEntry {
                key: "writer-key-12345678".into(),
                role: Role::Writer,
                description: "Writer key".into(),
                active: true,
            },
            ApiKeyEntry {
                key: "reader-key-12345678".into(),
                role: Role::Reader,
                description: "Reader key".into(),
                active: true,
            },
            ApiKeyEntry {
                key: "inactive-key-12345678".into(),
                role: Role::Admin,
                description: "Inactive key".into(),
                active: false,
            },
        ]
    }

    #[test]
    fn disabled_auth_allows_all() {
        let auth = AuthManager::disabled();
        assert!(!auth.is_enabled());
        assert_eq!(auth.authenticate("anything"), Some(Role::Admin));
    }

    #[test]
    fn valid_key_returns_role() {
        let auth = AuthManager::new(make_keys());
        assert_eq!(auth.authenticate("admin-key-12345678"), Some(Role::Admin));
        assert_eq!(auth.authenticate("writer-key-12345678"), Some(Role::Writer));
        assert_eq!(auth.authenticate("reader-key-12345678"), Some(Role::Reader));
    }

    #[test]
    fn invalid_key_returns_none() {
        let auth = AuthManager::new(make_keys());
        assert_eq!(auth.authenticate("bad-key"), None);
    }

    #[test]
    fn inactive_key_returns_none() {
        let auth = AuthManager::new(make_keys());
        assert_eq!(auth.authenticate("inactive-key-12345678"), None);
    }

    #[test]
    fn admin_can_do_everything() {
        assert!(AuthManager::authorize(Role::Admin, "CreateNode"));
        assert!(AuthManager::authorize(Role::Admin, "DeleteNode"));
        assert!(AuthManager::authorize(Role::Admin, "GetNode"));
        assert!(AuthManager::authorize(Role::Admin, "Ping"));
    }

    #[test]
    fn writer_can_read_and_write() {
        assert!(AuthManager::authorize(Role::Writer, "CreateNode"));
        assert!(AuthManager::authorize(Role::Writer, "DeleteNode"));
        assert!(AuthManager::authorize(Role::Writer, "GetNode"));
        assert!(AuthManager::authorize(Role::Writer, "Query"));
    }

    #[test]
    fn reader_cannot_write() {
        assert!(!AuthManager::authorize(Role::Reader, "CreateNode"));
        assert!(!AuthManager::authorize(Role::Reader, "DeleteNode"));
        assert!(!AuthManager::authorize(Role::Reader, "UpdateNode"));
        assert!(AuthManager::authorize(Role::Reader, "GetNode"));
        assert!(AuthManager::authorize(Role::Reader, "Query"));
        assert!(AuthManager::authorize(Role::Reader, "VectorSearch"));
        assert!(AuthManager::authorize(Role::Reader, "Ping"));
    }

    #[test]
    fn audit_log_records_entries() {
        let auth = AuthManager::new(make_keys());
        auth.audit("admin-key-12345678", Role::Admin, "CreateNode", true);
        auth.audit("reader-key-12345678", Role::Reader, "GetNode", true);
        auth.audit("reader-key-12345678", Role::Reader, "CreateNode", false);

        let recent = auth.recent_audit(10);
        assert_eq!(recent.len(), 3);
        assert!(!recent[0].allowed); // most recent first
        assert!(recent[1].allowed);
    }

    #[test]
    fn revoke_key_prevents_auth() {
        let auth = AuthManager::new(make_keys());
        assert_eq!(auth.authenticate("writer-key-12345678"), Some(Role::Writer));
        assert!(auth.revoke_key("writer-key-12345678"));
        assert_eq!(auth.authenticate("writer-key-12345678"), None);
    }

    #[test]
    fn add_key_works() {
        let auth = AuthManager::new(vec![]);
        assert_eq!(auth.authenticate("new-key"), None);
        auth.add_key(ApiKeyEntry {
            key: "new-key".into(),
            role: Role::Writer,
            description: "test".into(),
            active: true,
        });
        assert_eq!(auth.authenticate("new-key"), Some(Role::Writer));
    }

    #[test]
    fn role_ordering() {
        assert!(Role::Reader < Role::Writer);
        assert!(Role::Writer < Role::Admin);
    }
}

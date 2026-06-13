//! Access control types — Role and UserAssignment are the JSON content
//! of vault:role:<name> and vault:user:<id> frames respectively.
//!
//! T14 will add the `authorize_tag_access` helper for read-path checks.
//! T13 uses these types only for the bootstrap admin role written at
//! vault init time.

use serde::{Deserialize, Serialize};

/// Canonical list of vault operations the access-control system recognises.
/// Roles must list each operation they're allowed to perform; the bootstrap
/// admin role gets all of them.
pub const VAULT_OPERATIONS: &[&str] = &[
    "read", "rebuild", "restore", "compare", "ingest", "grant", "revoke",
];

/// A named role with allow/deny tag patterns and the list of operations
/// it permits. The bootstrap admin role uses `allows = ["*"]` to match
/// everything; tag-specific roles list explicit patterns.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Role {
    pub role: String,
    pub allows: Vec<String>,
    pub denies: Vec<String>,
    pub operations: Vec<String>,
}

/// A user identity assignment. The user inherits the union of their
/// assigned roles' permissions at authorization time.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UserAssignment {
    pub user: String,
    pub roles: Vec<String>,
    pub granted_at: String,
    pub granted_by: String,
}

/// Returns true iff at least one role in `roles` permits `operation`
/// against a manifest with `tags`. Deny patterns override allow patterns
/// when both apply to the same doc.
pub fn authorize_tag_access(roles: &[Role], tags: &[String], operation: &str) -> bool {
    for role in roles {
        if !role.operations.iter().any(|op| op == operation) {
            continue;
        }
        let mut allowed = false;
        for allow in &role.allows {
            // "*" = match-all wildcard (used by the bootstrap admin role)
            if allow == "*" || tags.iter().any(|t| tag_matches(t, allow)) {
                allowed = true;
                break;
            }
        }
        if !allowed {
            continue;
        }
        let mut denied = false;
        for deny in &role.denies {
            if tags.iter().any(|t| tag_matches(t, deny)) {
                denied = true;
                break;
            }
        }
        if !denied {
            return true;
        }
    }
    false
}

/// Exact match for v1. Future v1.5: glob-pattern (`dept:*`) matching.
fn tag_matches(tag: &str, pattern: &str) -> bool {
    tag == pattern
}

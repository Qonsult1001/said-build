//! Access control — role grant/revoke + authorization checks.

use said_vault::access::{Role, authorize_tag_access};

#[test]
fn admin_role_with_star_allows_everything() {
    let role = Role {
        role: "admin".into(),
        allows: vec!["*".into()],
        denies: vec![],
        operations: vec!["read".into(), "rebuild".into()],
    };
    assert!(authorize_tag_access(&[role], &["dept:legal".into()], "read"));
    assert!(authorize_tag_access(&[Role {
        role: "admin".into(), allows: vec!["*".into()], denies: vec![],
        operations: vec!["restore".into()],
    }], &["classification:restricted".into()], "restore"));
}

#[test]
fn role_allows_only_listed_tag_patterns() {
    let role = Role {
        role: "legal-reader".into(),
        allows: vec!["dept:legal".into()],
        denies: vec![],
        operations: vec!["read".into(), "rebuild".into()],
    };
    assert!(authorize_tag_access(&[role.clone()], &["dept:legal".into()], "read"));
    assert!(!authorize_tag_access(&[role.clone()], &["dept:hr".into()], "read"));
}

#[test]
fn deny_overrides_allow() {
    let role = Role {
        role: "limited".into(),
        allows: vec!["dept:legal".into()],
        denies: vec!["classification:restricted".into()],
        operations: vec!["read".into()],
    };
    // Doc with both dept:legal AND classification:restricted → denied
    let tags = vec!["dept:legal".into(), "classification:restricted".into()];
    assert!(!authorize_tag_access(&[role], &tags, "read"));
}

#[test]
fn operation_must_be_in_role_operations() {
    let role = Role {
        role: "read-only".into(),
        allows: vec!["*".into()],
        denies: vec![],
        operations: vec!["read".into()], // no rebuild, no restore
    };
    assert!(authorize_tag_access(&[role.clone()], &["dept:legal".into()], "read"));
    assert!(!authorize_tag_access(&[role], &["dept:legal".into()], "rebuild"));
}

#[test]
fn no_matching_role_means_denied() {
    let role = Role {
        role: "wrong-role".into(),
        allows: vec!["dept:hr".into()],
        denies: vec![],
        operations: vec!["read".into()],
    };
    assert!(!authorize_tag_access(&[role], &["dept:legal".into()], "read"));
}

#[test]
fn vault_operations_constant_contains_all_known_ops() {
    // Sanity check — if the operation list shrinks, callers checking against
    // it should fail at compile time, not silently mis-authorize at runtime.
    use said_vault::access::VAULT_OPERATIONS;
    for op in &["read", "rebuild", "restore", "compare", "ingest", "grant", "revoke"] {
        assert!(VAULT_OPERATIONS.contains(op), "missing operation: {}", op);
    }
}

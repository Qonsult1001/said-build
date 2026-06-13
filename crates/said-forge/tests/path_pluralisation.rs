//! Tests for the path pluralisation rewriter.
//!
//! Rule (collection_plural_singular_id):
//!   - GET /<collection>          → plural   (list all)
//!   - POST /<collection>         → singular (create one)
//!   - <verb> /<root>/{rootId>...  → root segment singular
//!   - nested .../<sub>/{id}       → sub stays plural (C2)
//!   - bare nested collection      → plural

use said_forge::OpenApiStandard;
use said_forge::dev_spec::parser::pluralise_path;

fn std_default() -> OpenApiStandard {
    OpenApiStandard::defaults()
}

#[test]
fn get_collection_stays_plural() {
    let std = std_default();
    let (out, _) = pluralise_path("GET", "/accounts", &std);
    assert_eq!(out, "/accounts");
}

#[test]
fn get_collection_cardholders_stays_plural() {
    let std = std_default();
    let (out, _) = pluralise_path("GET", "/cardholders", &std);
    assert_eq!(out, "/cardholders");
}

#[test]
fn post_collection_becomes_singular() {
    let std = std_default();
    let (out, log) = pluralise_path("POST", "/accounts", &std);
    assert_eq!(out, "/account");
    assert!(log.is_some(), "rewrite must be logged");
}

#[test]
fn post_cardholders_becomes_singular() {
    let std = std_default();
    let (out, _) = pluralise_path("POST", "/cardholders", &std);
    assert_eq!(out, "/cardholder");
}

#[test]
fn get_by_id_root_becomes_singular() {
    let std = std_default();
    let (out, log) = pluralise_path("GET", "/accounts/{accountId}", &std);
    assert_eq!(out, "/account/{accountId}");
    assert!(log.is_some());
}

#[test]
fn put_by_id_root_becomes_singular() {
    let std = std_default();
    let (out, _) = pluralise_path("PUT", "/accounts/{accountId}", &std);
    assert_eq!(out, "/account/{accountId}");
}

#[test]
fn delete_by_id_root_becomes_singular() {
    let std = std_default();
    let (out, _) = pluralise_path("DELETE", "/accounts/{accountId}", &std);
    assert_eq!(out, "/account/{accountId}");
}

#[test]
fn sub_resource_root_singular() {
    let std = std_default();
    let (out, _) = pluralise_path("GET", "/accounts/{accountId}/balance", &std);
    assert_eq!(out, "/account/{accountId}/balance");
}

#[test]
fn put_sub_resource_root_singular() {
    let std = std_default();
    let (out, _) = pluralise_path("PUT", "/accounts/{accountId}/balance", &std);
    assert_eq!(out, "/account/{accountId}/balance");
}

#[test]
fn nested_collection_get_keeps_child_plural() {
    let std = std_default();
    let (out, _) = pluralise_path("GET", "/accounts/{accountId}/transitions", &std);
    assert_eq!(out, "/account/{accountId}/transitions");
}

#[test]
fn nested_post_creates_singular_child() {
    let std = std_default();
    let (out, _) = pluralise_path("POST", "/accounts/{accountId}/transitions", &std);
    assert_eq!(out, "/account/{accountId}/transition");
}

#[test]
fn nested_collection_with_id_keeps_plural_c2() {
    let std = std_default();
    let (out, _) = pluralise_path(
        "GET",
        "/accounts/{accountId}/transitions/{transitionId}",
        &std,
    );
    assert_eq!(out, "/account/{accountId}/transitions/{transitionId}");
}

#[test]
fn nested_collection_under_other_parent_stays_plural() {
    let std = std_default();
    let (out, _) = pluralise_path(
        "GET",
        "/cardholders/{cardholderId}/accounts",
        &std,
    );
    assert_eq!(out, "/cardholder/{cardholderId}/accounts");
}

#[test]
fn already_singular_paths_are_unchanged() {
    let std = std_default();
    let (out, log) = pluralise_path("PUT", "/account/{accountId}", &std);
    assert_eq!(out, "/account/{accountId}");
    assert!(log.is_none(), "no rewrite, no log entry");
}

#[test]
fn already_singular_collection_unchanged_for_post() {
    let std = std_default();
    let (out, log) = pluralise_path("POST", "/account", &std);
    assert_eq!(out, "/account");
    assert!(log.is_none());
}

#[test]
fn rule_disabled_passes_through() {
    let mut std = std_default();
    std.paths.collection_pluralisation = said_forge::openapi_standard::CollectionPluralisation::Preserve;
    let (out, log) = pluralise_path("POST", "/accounts", &std);
    assert_eq!(out, "/accounts");
    assert!(log.is_none());
}

#[test]
fn known_singular_words_left_alone() {
    let std = std_default();
    // `pins` is a domain noun where plural feels natural even for create.
    // The rule still applies — POST /pins → /pin. If a user wants to keep
    // a specific path plural they use a per-client override (not yet
    // implemented as an exception list, but the toggle still works at
    // the standard level).
    let (out, _) = pluralise_path("POST", "/pins", &std);
    assert_eq!(out, "/pin");
}

#[test]
fn segments_with_digits_are_not_singularised() {
    // `3ds` is 3-D Secure (an acronym), not a plural. The singulariser
    // must leave any segment containing a digit alone.
    let std = std_default();
    let (out, _) = pluralise_path("POST", "/3ds/bulk/enroll", &std);
    assert_eq!(out, "/3ds/bulk/enroll");

    let (out2, _) = pluralise_path("POST", "/3ds/events/{authenticationId}", &std);
    assert_eq!(out2, "/3ds/events/{authenticationId}");
}

#[test]
fn singular_with_irregular_plural_simulated() {
    let std = std_default();
    // /businesses → /business. Smoke test for ‑es plural drop.
    let (out, _) = pluralise_path("POST", "/businesses", &std);
    assert_eq!(out, "/business");
    let (out2, _) = pluralise_path("PUT", "/businesses/{businessId}", &std);
    assert_eq!(out2, "/business/{businessId}");
}

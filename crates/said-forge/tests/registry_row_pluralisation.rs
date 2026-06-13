//! Tests that registry rows go through the same pluralisation +
//! param-casing rules as Dev Spec endpoints. Without this, the seed
//! data in `ars_Api_Rule_Settings` (singular vs plural drift) leaks
//! into the emitted OpenAPI spec.

#![cfg(feature = "forge-sql-verify")]

use said_forge::fitter::{add_op_to_spec, RegistryEntry, StandardNormalisation};
use said_forge::OpenApiStandard;
use uuid::Uuid;

fn empty_spec() -> serde_yaml::Value {
    let mut doc = serde_yaml::Mapping::new();
    doc.insert(
        serde_yaml::Value::String("paths".into()),
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
    );
    serde_yaml::Value::Mapping(doc)
}

fn entry(path: &str, method: &str) -> RegistryEntry {
    RegistryEntry {
        api_id: Uuid::nil(),
        has_credential: false,
        path: path.into(),
        original_path: path.into(),
        method: Some(method.into()),
        enabled: true,
        source_table: "ars_Api_Rule_Settings".into(),
    }
}

fn paths_in(spec: &serde_yaml::Value) -> Vec<String> {
    spec.get("paths")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.keys()
                .filter_map(|k| k.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn registry_plural_by_id_becomes_singular() {
    // TXN seed has `/businesses/{businessId}/transitions` (plural root,
    // wrong per the rule). After normalisation it must be
    // `/business/{businessId}/transitions` (singular root, plural child).
    let std_def = OpenApiStandard::defaults();
    let mut spec = empty_spec();
    let mut norms: Vec<StandardNormalisation> = Vec::new();
    add_op_to_spec(
        &mut spec,
        &entry("/businesses/{businessId}/transitions", "GET"),
        "txn.p_txn_Get_Business_Transitions",
        &std_def,
        &mut norms,
    );
    let paths = paths_in(&spec);
    assert!(
        paths.contains(&"/business/{businessId}/transitions".to_string()),
        "expected singular root, got: {paths:?}"
    );
    assert!(
        norms.iter().any(|n| n.reason.starts_with("paths.collection_pluralisation")),
        "expected pluralisation log entry, got: {:?}",
        norms.iter().map(|n| n.reason.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn registry_snake_case_path_param_becomes_camel() {
    // `/cards/{card_id}` from the registry must surface as
    // `/cards/{cardId}` and log paths.param_casing.
    let std_def = OpenApiStandard::defaults();
    let mut spec = empty_spec();
    let mut norms: Vec<StandardNormalisation> = Vec::new();
    add_op_to_spec(
        &mut spec,
        &entry("/cards/{card_id}", "GET"),
        "txn.p_txn_Get_Card",
        &std_def,
        &mut norms,
    );
    let paths = paths_in(&spec);
    // Note: `/cards` is a bare collection (no id directly after) — stays
    // plural; the param itself becomes camelCase.
    assert!(
        paths.iter().any(|p| p == "/cards/{cardId}" || p == "/card/{cardId}"),
        "expected camelCased path-param, got: {paths:?}"
    );
    assert!(
        norms.iter().any(|n| n.reason.starts_with("paths.param_casing")),
        "expected param-casing log entry"
    );
}

#[test]
fn registry_post_collection_becomes_singular() {
    // POST /accounts is creating one resource. Rule: singular.
    let std_def = OpenApiStandard::defaults();
    let mut spec = empty_spec();
    let mut norms: Vec<StandardNormalisation> = Vec::new();
    add_op_to_spec(
        &mut spec,
        &entry("/accounts", "POST"),
        "txn.p_txn_Create_Account",
        &std_def,
        &mut norms,
    );
    let paths = paths_in(&spec);
    assert_eq!(paths, vec!["/account".to_string()]);
}

#[test]
fn registry_get_collection_stays_plural() {
    let std_def = OpenApiStandard::defaults();
    let mut spec = empty_spec();
    let mut norms: Vec<StandardNormalisation> = Vec::new();
    add_op_to_spec(
        &mut spec,
        &entry("/accounts", "GET"),
        "txn.p_txn_Get_Accounts",
        &std_def,
        &mut norms,
    );
    let paths = paths_in(&spec);
    assert_eq!(paths, vec!["/accounts".to_string()]);
}

#[test]
fn registry_nested_id_keeps_plural_c2() {
    // C2: nested {id}-bearing collection segments keep plural.
    let std_def = OpenApiStandard::defaults();
    let mut spec = empty_spec();
    let mut norms: Vec<StandardNormalisation> = Vec::new();
    add_op_to_spec(
        &mut spec,
        &entry(
            "/cardholders/transitions/{transition_id}",
            "GET",
        ),
        "txn.p_txn_Get_Cardholder_Transition",
        &std_def,
        &mut norms,
    );
    let paths = paths_in(&spec);
    // Root `cardholders` is NOT followed by id directly — stays plural.
    // Nested `transitions` keeps plural per C2.
    // Path-param `transition_id` becomes `transitionId`.
    assert!(
        paths.contains(&"/cardholders/transitions/{transitionId}".to_string()),
        "got: {paths:?}"
    );
}

#[test]
fn preserve_mode_skips_normalisation() {
    let mut std_def = OpenApiStandard::defaults();
    std_def.paths.collection_pluralisation =
        said_forge::openapi_standard::CollectionPluralisation::Preserve;
    std_def.paths.param_casing = said_forge::openapi_standard::ParamCasing::Preserve;

    let mut spec = empty_spec();
    let mut norms: Vec<StandardNormalisation> = Vec::new();
    add_op_to_spec(
        &mut spec,
        &entry("/businesses/{business_id}/transitions", "GET"),
        "txn.p_txn_Get_Business_Transitions",
        &std_def,
        &mut norms,
    );
    let paths = paths_in(&spec);
    assert_eq!(paths, vec!["/businesses/{business_id}/transitions".to_string()]);
    assert!(norms.is_empty(), "no logs in preserve mode");
}

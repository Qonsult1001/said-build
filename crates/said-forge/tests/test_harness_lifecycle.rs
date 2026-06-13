//! Integration tests for the rewritten test-harness lifecycle. The
//! `lifecycle` module is gated behind `forge-sql-verify` (it depends
//! on tiberius/axum), so these tests only build when that feature is
//! on. The bucket classifier and spec-paths matcher don't actually
//! touch the sandbox — they're pure functions over fixture URLs +
//! parsed spec — so the tests run anywhere.

#![cfg(feature = "forge-sql-verify")]

use said_forge::test_harness::lifecycle::{
    classify_bucket, spec_paths_from_yaml, Bucket, SpecPaths,
};
use serde_yaml::Value;

#[test]
fn classify_handles_card_canonical_lifecycle() {
    // Walks each fixture name from the dtcard Card folder and asserts
    // the bucket assignment matches the design rules. This is the
    // headline case the rewrite was motivated by.
    let cases: &[(&str, &str, Bucket)] = &[
        ("POST", "{{localBaseURL}}/card", Bucket::RootPost),
        ("GET",  "{{localBaseURL}}/card/{cardId}", Bucket::GetById),
        ("PUT",  "{{localBaseURL}}/card/{cardId}", Bucket::UpdateById),
        ("POST", "{{localBaseURL}}/card/{cardId}/transition", Bucket::SubResource),
        ("GET",  "{{localBaseURL}}/card/{cardId}/transitions", Bucket::SubResource),
        ("GET",  "{{localBaseURL}}/cardholder/{cardHolderId}/cards?page=1&limit=10",
            Bucket::SubResource), // cross-resource — same bucket, label distinguishes
        ("GET",  "{{localBaseURL}}/cards", Bucket::CollectionList),
    ];
    for (method, url, expected) in cases {
        let actual = classify_bucket(method, url, "Card");
        assert_eq!(actual, *expected,
            "method={method} url={url} expected={expected:?} got={actual:?}");
    }
}

#[test]
fn spec_paths_from_yaml_harvests_methods_and_paths() {
    // Tiny spec sample — enough to verify the harvester collects each
    // (method, path) pair and uppercases the method.
    let yaml = r#"
paths:
  /card:
    post:
      description: Create a card
  /card/{cardId}:
    get:
      description: Read a card
    put:
      description: Update a card
  /cards:
    get:
      description: List cards
"#;
    let spec: Value = serde_yaml::from_str(yaml).expect("parse spec");
    let paths: SpecPaths = spec_paths_from_yaml(&spec);

    assert!(paths.contains(&("POST".to_string(), "/card".to_string())),
        "missing POST /card");
    assert!(paths.contains(&("GET".to_string(), "/card/{cardId}".to_string())),
        "missing GET /card/{{cardId}}");
    assert!(paths.contains(&("PUT".to_string(), "/card/{cardId}".to_string())),
        "missing PUT /card/{{cardId}}");
    assert!(paths.contains(&("GET".to_string(), "/cards".to_string())),
        "missing GET /cards");
    assert_eq!(paths.len(), 4);
}

#[test]
fn classify_query_string_does_not_break_bucket() {
    let b = classify_bucket(
        "GET",
        "{{localBaseURL}}/cards?page=1&limit=10",
        "Card",
    );
    assert_eq!(b, Bucket::CollectionList);
}

#[test]
fn classify_works_with_pre_rendered_url() {
    // After templating runs, the URL might already be the full HTTP form.
    let b = classify_bucket(
        "POST",
        "http://127.0.0.1:5050/binsponsor",
        "BinSponsor",
    );
    assert_eq!(b, Bucket::RootPost);
}

#[test]
fn classify_pluralised_folder_entity_still_singular_root() {
    // Build-order names are PascalCase singular (e.g. "Bin"). Bruno
    // fixtures may live in plural folders ("Bins"). The classifier
    // takes the entity name straight from build_order, so it sees
    // "Bin" — but the fixture URL `/bin` is still the singular root.
    let b = classify_bucket(
        "POST",
        "{{localBaseURL}}/bin",
        "Bin",
    );
    assert_eq!(b, Bucket::RootPost);
}

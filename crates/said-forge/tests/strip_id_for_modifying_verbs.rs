//! Tests for R4: `parameters.strip_id_for_modifying_verbs`.
//!
//! Rule: strip `id` from request body when verb ∈ {PUT, PATCH, DELETE}.
//! POST keeps `id` (POST creates the resource and the body's `id` IS
//! the new id; needed for client-generated UUID idempotency).
//!
//! Response bodies are untouched.

use said_forge::OpenApiStandard;
use said_forge::dev_spec::parser::strip_id_for_modifying_verbs;
use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecSchema};
use std::collections::BTreeMap;

fn scalar(ty: &str) -> DevSpecSchema {
    DevSpecSchema {
        ty: ty.into(), format: None, description: None,
            example: None,
        properties: BTreeMap::new(), items: None,
    }
}

fn body_with_keys(keys: &[&str]) -> DevSpecSchema {
    let mut props = BTreeMap::new();
    for k in keys {
        props.insert((*k).into(), scalar("string"));
    }
    DevSpecSchema {
        ty: "object".into(), format: None, description: None,
            example: None,
        properties: props, items: None,
    }
}

fn ep_with_request(method: &str, body: DevSpecSchema) -> DevSpecEndpoint {
    DevSpecEndpoint {
        method: method.into(),
        path: "/test".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![],
        request_body: Some(body),
        response_body: None,
    }
}

#[test]
fn put_strips_id_from_request_body() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request("PUT", body_with_keys(&["id", "name", "amount"]));
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(!body.properties.contains_key("id"),
        "PUT must strip id from body");
    assert!(body.properties.contains_key("name"));
    assert!(body.properties.contains_key("amount"));
}

#[test]
fn patch_strips_id_from_request_body() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request("PATCH", body_with_keys(&["id", "status"]));
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    assert!(!ep.request_body.unwrap().properties.contains_key("id"));
}

#[test]
fn delete_strips_id_from_request_body() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request("DELETE", body_with_keys(&["id", "reason"]));
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    assert!(!ep.request_body.unwrap().properties.contains_key("id"));
}

#[test]
fn post_keeps_id_in_request_body() {
    // POST creates the resource. Client-generated UUID idempotency
    // means the body MUST carry the new id.
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request("POST", body_with_keys(&["id", "name"]));
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(body.properties.contains_key("id"),
        "POST must KEEP id — it's the new resource's id");
    assert!(body.properties.contains_key("name"));
}

#[test]
fn get_keeps_id_in_request_body() {
    // GET rarely has a body, but if it does, the rule doesn't apply.
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request("GET", body_with_keys(&["id", "filter"]));
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    assert!(ep.request_body.unwrap().properties.contains_key("id"));
}

#[test]
fn preserve_mode_keeps_id_for_put() {
    let mut std = OpenApiStandard::defaults();
    std.parameters.strip_id_for_modifying_verbs = false;
    let mut ep = ep_with_request("PUT", body_with_keys(&["id", "name"]));
    strip_id_for_modifying_verbs(&mut ep, &std);
    assert!(ep.request_body.unwrap().properties.contains_key("id"),
        "preserve mode must keep id");
}

#[test]
fn lowercase_verb_still_matches() {
    // Defensive: verbs may arrive in any case; rule must be
    // case-insensitive.
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request("put", body_with_keys(&["id", "name"]));
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    assert!(!ep.request_body.unwrap().properties.contains_key("id"));
}

#[test]
fn no_request_body_is_safe() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = DevSpecEndpoint {
        method: "DELETE".into(),
        path: "/test/{id}".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![],
        request_body: None,
        response_body: None,
    };
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    assert!(ep.request_body.is_none());
}

#[test]
fn response_body_id_untouched_for_put() {
    // Response bodies must keep id even for PUT — that's the resource
    // we operated on.
    let std_def = OpenApiStandard::defaults();
    let mut ep = DevSpecEndpoint {
        method: "PUT".into(),
        path: "/test/{id}".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![],
        request_body: None,
        response_body: Some(body_with_keys(&["id", "name"])),
    };
    strip_id_for_modifying_verbs(&mut ep, &std_def);
    assert!(ep.response_body.unwrap().properties.contains_key("id"),
        "response bodies must keep id even for PUT");
}

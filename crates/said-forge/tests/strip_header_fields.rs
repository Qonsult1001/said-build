//! Tests for `parameters.strip_header_fields_from_request_body`.
//!
//! Rule:
//!   - Strip top-level request-body keys that match a name declared in
//!     `parameters.standard_headers.{required,optional}` (these travel
//!     as headers, not body fields).
//!   - Always strip `responseId` from request bodies (response-only).
//!   - Leave `id` alone — it's a domain field for POST creates.
//!   - Response bodies are untouched.
//!   - Toggle off via `strip_header_fields_from_request_body = false`.

use said_forge::OpenApiStandard;
use said_forge::dev_spec::parser::strip_header_fields_from_request_body;
use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecSchema};
use std::collections::BTreeMap;

fn scalar(ty: &str) -> DevSpecSchema {
    DevSpecSchema {
        ty: ty.into(), format: None, description: None, example: None,
        properties: BTreeMap::new(), items: None,
    }
}

fn body_with_keys(keys: &[&str]) -> DevSpecSchema {
    let mut props = BTreeMap::new();
    for k in keys {
        props.insert((*k).into(), scalar("string"));
    }
    DevSpecSchema {
        ty: "object".into(), format: None, description: None, example: None,
        properties: props, items: None,
    }
}

fn ep_with_request(body: DevSpecSchema) -> DevSpecEndpoint {
    DevSpecEndpoint {
        method: "POST".into(),
        path: "/test".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![],
        request_body: Some(body),
        response_body: None,
    }
}

fn ep_with_response(body: DevSpecSchema) -> DevSpecEndpoint {
    DevSpecEndpoint {
        method: "GET".into(),
        path: "/test".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![],
        request_body: None,
        response_body: Some(body),
    }
}

#[test]
fn strips_request_id_from_request_body() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request(body_with_keys(&["requestId", "name"]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(!body.properties.contains_key("requestId"),
        "requestId must be stripped; got keys: {:?}",
        body.properties.keys().collect::<Vec<_>>());
    assert!(body.properties.contains_key("name"));
}

#[test]
fn strips_correlation_id_from_request_body() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request(body_with_keys(&["correlationId", "amount"]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(!body.properties.contains_key("correlationId"));
    assert!(body.properties.contains_key("amount"));
}

#[test]
fn strips_response_id_from_request_body() {
    // responseId is response-only; stripping is unconditional, regardless
    // of whether it appears in standard_headers.
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request(body_with_keys(&["responseId", "currency"]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(!body.properties.contains_key("responseId"));
    assert!(body.properties.contains_key("currency"));
}

#[test]
fn keeps_id_in_request_body() {
    // `id` is a domain field, not a tracing field. POST creates use it.
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request(body_with_keys(&["id", "name", "requestId"]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(body.properties.contains_key("id"),
        "id must NOT be stripped — it's a domain field");
    assert!(body.properties.contains_key("name"));
    assert!(!body.properties.contains_key("requestId"));
}

#[test]
fn does_not_touch_response_body() {
    // Response bodies must keep requestId/correlationId/responseId —
    // they're the envelope tracking fields.
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_response(body_with_keys(&[
        "requestId", "correlationId", "responseId", "result",
    ]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.response_body.unwrap();
    assert!(body.properties.contains_key("requestId"));
    assert!(body.properties.contains_key("correlationId"));
    assert!(body.properties.contains_key("responseId"));
    assert!(body.properties.contains_key("result"));
}

#[test]
fn ocp_apim_subscription_key_is_stripped_too() {
    // It's listed in standard_headers.required, so the rule strips it
    // from request bodies. (Subscription keys must travel as headers.)
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request(body_with_keys(&[
        "Ocp-Apim-Subscription-Key", "amount",
    ]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(!body.properties.contains_key("Ocp-Apim-Subscription-Key"));
    assert!(body.properties.contains_key("amount"));
}

#[test]
fn x_api_version_is_stripped_too() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request(body_with_keys(&["x-api-version", "amount"]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(!body.properties.contains_key("x-api-version"));
    assert!(body.properties.contains_key("amount"));
}

#[test]
fn nested_request_id_is_left_alone() {
    // Strip only at top level — a nested object field named `requestId`
    // probably means something different (e.g. a logged inner request).
    let std_def = OpenApiStandard::defaults();
    let mut nested_props = BTreeMap::new();
    nested_props.insert("requestId".into(), scalar("string"));
    let nested = DevSpecSchema {
        ty: "object".into(), format: None, description: None,
            example: None,
        properties: nested_props, items: None,
    };
    let mut top_props = BTreeMap::new();
    top_props.insert("auditTrail".into(), nested);
    top_props.insert("amount".into(), scalar("string"));
    top_props.insert("requestId".into(), scalar("string"));
    let body = DevSpecSchema {
        ty: "object".into(), format: None, description: None,
            example: None,
        properties: top_props, items: None,
    };
    let mut ep = ep_with_request(body);
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let out = ep.request_body.unwrap();
    // Top-level requestId stripped
    assert!(!out.properties.contains_key("requestId"));
    // Nested requestId preserved
    let audit = out.properties.get("auditTrail").unwrap();
    assert!(audit.properties.contains_key("requestId"),
        "nested requestId must be preserved");
}

#[test]
fn preserve_mode_keeps_header_fields_in_request_body() {
    let mut std = OpenApiStandard::defaults();
    std.parameters.strip_header_fields_from_request_body = false;
    let mut ep = ep_with_request(body_with_keys(&["requestId", "responseId", "amount"]));
    strip_header_fields_from_request_body(&mut ep, &std);
    let body = ep.request_body.unwrap();
    assert!(body.properties.contains_key("requestId"));
    assert!(body.properties.contains_key("responseId"));
    assert!(body.properties.contains_key("amount"));
}

#[test]
fn strips_response_envelope_fields_result_error_datetime() {
    // `result`, `error`, `dateTime` are response-envelope concerns —
    // they never belong in a request body. Real Dev Spec authoring
    // mistake bled them into PutAccountRequest; the rule defends
    // against it regardless of upstream parser hygiene.
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_request(body_with_keys(&[
        "id", "name", "result", "error", "dateTime",
    ]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.request_body.unwrap();
    assert!(body.properties.contains_key("id"));
    assert!(body.properties.contains_key("name"));
    assert!(!body.properties.contains_key("result"),
        "result must be stripped from request body");
    assert!(!body.properties.contains_key("error"),
        "error must be stripped from request body");
    assert!(!body.properties.contains_key("dateTime"),
        "dateTime must be stripped from request body");
}

#[test]
fn response_envelope_fields_kept_in_response_body() {
    let std_def = OpenApiStandard::defaults();
    let mut ep = ep_with_response(body_with_keys(&[
        "result", "error", "dateTime", "requestId",
    ]));
    strip_header_fields_from_request_body(&mut ep, &std_def);
    let body = ep.response_body.unwrap();
    assert!(body.properties.contains_key("result"),
        "response body keeps result");
    assert!(body.properties.contains_key("error"));
    assert!(body.properties.contains_key("dateTime"));
    assert!(body.properties.contains_key("requestId"));
}

#[test]
fn empty_body_is_safe() {
    let std_def = OpenApiStandard::defaults();
    let body = DevSpecSchema {
        ty: "object".into(), format: None, description: None,
            example: None,
        properties: BTreeMap::new(), items: None,
    };
    let mut ep = ep_with_request(body);
    strip_header_fields_from_request_body(&mut ep, &std_def);
    assert!(ep.request_body.unwrap().properties.is_empty());
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
    strip_header_fields_from_request_body(&mut ep, &std_def);
    assert!(ep.request_body.is_none());
}

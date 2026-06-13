//! Tests that:
//! 1. The parser extracts response schemas from `### Responses` →
//!    `#### Type: object` / `#### 200` (modern dtcard convention).
//! 2. The emitter inlines Dev Spec `result.properties` into the envelope's
//!    `result` field (E2 hybrid).
//! 3. Tracking-field examples (requestId/correlationId/responseId/dateTime)
//!    are copied from Dev Spec onto the envelope.

use said_forge::dev_spec::parser::parse_endpoint_file;
use said_forge::dev_spec::openapi_emit::emit_components_schemas;
use said_forge::OpenApiStandard;
use std::path::Path;

#[test]
fn response_body_parsed_from_responses_subsection() {
    let path = Path::new(
        "../../dtcard/4-expectations/Dev Planning/Accounts/GET-accounts-_account_id_.md"
    );
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(path, &std_def).unwrap();
    let body = ep.response_body
        .expect("response body must be parsed from `### Responses` → `#### Type: object`");

    // Top-level envelope shape should be parsed.
    let keys: Vec<&str> = body.properties.keys().map(|s| s.as_str()).collect();
    assert!(keys.contains(&"requestId"), "got: {keys:?}");
    assert!(keys.contains(&"result"), "got: {keys:?}");

    // `result` should carry Account fields (camelCased).
    let result = body.properties.get("result").expect("result present");
    let result_keys: Vec<&str> = result.properties.keys().map(|s| s.as_str()).collect();
    assert!(result_keys.contains(&"id"),
        "result.id missing; got: {result_keys:?}");
    assert!(result_keys.contains(&"name"),
        "result.name missing; got: {result_keys:?}");
    assert!(
        result_keys.contains(&"currencyCode") || result_keys.contains(&"currency_code"),
        "result.currencyCode missing; got: {result_keys:?}"
    );
}

#[test]
fn envelope_yaml_inlines_result_properties_and_carries_examples() {
    let path = Path::new(
        "../../dtcard/4-expectations/Dev Planning/Accounts/GET-accounts-_account_id_.md"
    );
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(path, &std_def).unwrap();
    let comps = emit_components_schemas(&[ep], &std_def);
    let map = comps.as_mapping().expect("components mapping");

    // Find any *Response schema (the GET response envelope).
    let (_name, env) = map
        .iter()
        .find(|(k, _)| k.as_str().map(|s| s.contains("Response")).unwrap_or(false))
        .expect("at least one Response envelope present");
    let env_map = env.as_mapping().unwrap();
    let props = env_map
        .get(&serde_yaml::Value::String("properties".into()))
        .and_then(|v| v.as_mapping())
        .expect("envelope properties present");

    // requestId carries example.
    let req_id = props
        .get(&serde_yaml::Value::String("requestId".into()))
        .and_then(|v| v.as_mapping())
        .expect("requestId present");
    let req_id_example = req_id.get(&serde_yaml::Value::String("example".into()))
        .and_then(|v| v.as_str());
    assert!(req_id_example.is_some(),
        "envelope requestId must carry the Dev Spec example, got envelope: {req_id:?}");

    // result has typed properties (not the placeholder).
    let result = props
        .get(&serde_yaml::Value::String("result".into()))
        .and_then(|v| v.as_mapping())
        .expect("result present");
    let result_props = result
        .get(&serde_yaml::Value::String("properties".into()))
        .and_then(|v| v.as_mapping());
    assert!(result_props.is_some(),
        "result must inline payload properties (E2), got result: {result:?}");
    let result_props = result_props.unwrap();
    assert!(result_props.contains_key(&serde_yaml::Value::String("id".into())));
}

//! Tests for body-field casing normalisation.
//!
//! Rule (`parameters.body_field_casing`):
//!   - `camel_case`  → `account_owner_id` rewritten to `accountOwnerId`,
//!                     applies recursively to nested object fields,
//!                     covers BOTH request and response bodies (S4).
//!   - `preserve`    → emit verbatim.

use said_forge::dev_spec::parser::{parse_endpoint_file_logged, PathRewriteLog, snake_to_camel};
use said_forge::OpenApiStandard;
use said_forge::openapi_standard::BodyFieldCasing;
use std::path::Path;

#[test]
fn snake_to_camel_basic() {
    assert_eq!(snake_to_camel("account_owner_id"), "accountOwnerId");
    assert_eq!(snake_to_camel("first_name"), "firstName");
    assert_eq!(snake_to_camel("wallet_account_identifier"), "walletAccountIdentifier");
}

#[test]
fn snake_to_camel_already_camel_unchanged() {
    assert_eq!(snake_to_camel("accountOwnerId"), "accountOwnerId");
    assert_eq!(snake_to_camel("firstName"), "firstName");
    assert_eq!(snake_to_camel("id"), "id");
}

#[test]
fn snake_to_camel_pascal_left_alone() {
    // PascalCase isn't snake_case — leave it alone. The harvester
    // canonicalises ERD column names separately.
    assert_eq!(snake_to_camel("FirstName"), "FirstName");
    assert_eq!(snake_to_camel("AccountOwnerId"), "AccountOwnerId");
}

#[test]
fn snake_to_camel_preserves_leading_lowercase() {
    assert_eq!(snake_to_camel("a_b_c"), "aBC");
    assert_eq!(snake_to_camel("x"), "x");
}

#[test]
fn parser_rewrites_top_level_request_body_keys() {
    let std_def = OpenApiStandard::defaults();
    let mut rewrites: Vec<PathRewriteLog> = Vec::new();
    let path = Path::new("tests/fixtures/dev_spec/POST-cardholders-createCardholder.md");
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let ep = parse_endpoint_file_logged(path, &std_def, &mut rewrites).unwrap();
    let body = ep.request_body.expect("body present");
    // Top-level keys must be camelCase, never snake.
    for k in body.properties.keys() {
        assert!(!k.contains('_'),
            "top-level key contains underscore: {k}");
    }
}

#[test]
fn parser_rewrites_nested_request_body_keys_recursively() {
    let std_def = OpenApiStandard::defaults();
    let mut rewrites: Vec<PathRewriteLog> = Vec::new();
    let path = Path::new("tests/fixtures/dev_spec/POST-cardholders-createCardholder.md");
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let ep = parse_endpoint_file_logged(path, &std_def, &mut rewrites).unwrap();
    let body = ep.request_body.expect("body present");
    // Walk recursively: every key in every nested object must be camelCase.
    fn assert_camel(node: &said_forge::dev_spec::types::DevSpecSchema) {
        for (k, v) in &node.properties {
            assert!(!k.contains('_'),
                "nested key contains underscore: {k}");
            assert_camel(v);
        }
    }
    assert_camel(&body);
}

#[test]
fn preserve_mode_keeps_snake_case_keys_verbatim() {
    use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecSchema};
    use std::collections::BTreeMap;

    // Build a synthetic endpoint with snake_case keys, run the
    // body-casing pass, and verify they stay verbatim.
    let mut std = OpenApiStandard::defaults();
    std.parameters.body_field_casing = BodyFieldCasing::Preserve;

    let mut props = BTreeMap::new();
    props.insert("account_owner_id".into(), DevSpecSchema {
        ty: "string".into(), format: Some("uuid".into()),
        description: None, properties: BTreeMap::new(), items: None,
        example: None,
    });
    let body = DevSpecSchema {
        ty: "object".into(), format: None, description: None,
            example: None,
        properties: props, items: None,
    };
    let mut ep = DevSpecEndpoint {
        method: "POST".into(),
        path: "/test".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![],
        request_body: Some(body),
        response_body: None,
    };
    said_forge::dev_spec::parser::normalise_body_field_casing(&mut ep, &std);
    let out = ep.request_body.unwrap();
    assert!(out.properties.contains_key("account_owner_id"),
        "preserve mode must keep snake_case: keys={:?}",
        out.properties.keys().collect::<Vec<_>>());
}

#[test]
fn camel_case_mode_rewrites_response_body_too() {
    use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecSchema};
    use std::collections::BTreeMap;

    let std_def = OpenApiStandard::defaults();
    let mut props = BTreeMap::new();
    props.insert("response_id".into(), DevSpecSchema {
        ty: "string".into(), format: Some("uuid".into()),
        description: None, properties: BTreeMap::new(), items: None,
        example: None,
    });
    let body = DevSpecSchema {
        ty: "object".into(), format: None, description: None,
            example: None,
        properties: props, items: None,
    };
    let mut ep = DevSpecEndpoint {
        method: "GET".into(),
        path: "/test".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![],
        request_body: None,
        response_body: Some(body),
    };
    said_forge::dev_spec::parser::normalise_body_field_casing(&mut ep, &std_def);
    let out = ep.response_body.unwrap();
    assert!(out.properties.contains_key("responseId"),
        "camelCase mode must rewrite response body keys");
    assert!(!out.properties.contains_key("response_id"));
}

#[test]
fn path_param_names_camelcased_by_body_field_rule_too() {
    // S4: rewrite path_param.name field as well, so any consumer
    // that reads the parsed endpoint sees camelCase everywhere.
    use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecParam};

    let std_def = OpenApiStandard::defaults();
    let mut ep = DevSpecEndpoint {
        method: "GET".into(),
        path: "/account/{accountId}".into(),
        summary: String::new(),
        source_file: "T.md".into(),
        path_params: vec![DevSpecParam {
            name: "account_id".into(),
            ty: "uuid".into(),
            required: true,
            description: String::new(),
            ..Default::default()
        }],
        request_body: None,
        response_body: None,
    };
    said_forge::dev_spec::parser::normalise_body_field_casing(&mut ep, &std_def);
    assert_eq!(ep.path_params[0].name, "accountId");
}

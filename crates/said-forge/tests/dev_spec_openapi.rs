use said_forge::dev_spec::openapi_emit::{
    build_operation_yaml, emit_components_schemas, standard_headers, tag_from_source_file,
};
use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecParam, DevSpecSchema};
use std::collections::BTreeMap;

fn cardholder_post_endpoint() -> DevSpecEndpoint {
    let mut props = BTreeMap::new();
    props.insert("id".into(), DevSpecSchema {
        ty: "string".into(),
        format: Some("uuid".into()),
        description: Some("Cardholder ID".into()),
        example: None,
        properties: BTreeMap::new(),
        items: None,
    });
    props.insert("firstName".into(), DevSpecSchema {
        ty: "string".into(),
        format: None,
        description: Some("First name".into()),
        example: None,
        properties: BTreeMap::new(),
        items: None,
    });
    DevSpecEndpoint {
        method: "POST".into(),
        path: "/cardholders".into(),
        summary: "Creates a new cardholder.".into(),
        source_file: "Cardholders/POST-cardholders-createCardholder.md".into(),
        path_params: vec![],
        request_body: Some(DevSpecSchema {
            ty: "object".into(),
            format: None,
            description: None,
            example: None,
            properties: props,
            items: None,
        }),
        response_body: None,
    }
}

#[test]
fn standard_headers_includes_request_id_and_subscription() {
    let std_def = said_forge::OpenApiStandard::defaults();
    let h = standard_headers(&std_def);
    let names: Vec<&str> = h.iter()
        .filter_map(|m| m.get(&serde_yaml::Value::String("name".into()))
            .and_then(|v| v.as_str()))
        .collect();
    assert!(names.contains(&"requestId"));
    assert!(names.contains(&"correlationId"));
    assert!(names.contains(&"x-api-version"));
    assert!(names.contains(&"Ocp-Apim-Subscription-Key"));
}

#[test]
fn tag_extracted_from_source_file_path() {
    assert_eq!(tag_from_source_file("Cardholders/POST-cardholders-createCardholder.md"), "Cardholder");
    assert_eq!(tag_from_source_file("Accounts/GET-accounts-_account_id_.md"), "Account");
    assert_eq!(tag_from_source_file("POST-anything.md"), "Default");
}

#[test]
fn build_operation_yaml_includes_tags_params_request_body_responses() {
    let ep = cardholder_post_endpoint();
    let std_def = said_forge::OpenApiStandard::defaults();
    let op = build_operation_yaml(&ep, "cardholder.p_txn_Create_Cardholder", &std_def);
    let map = op.as_mapping().expect("operation is mapping");

    let tags = map.get(&serde_yaml::Value::String("tags".into()))
        .expect("tags present").as_sequence().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].as_str(), Some("Cardholder"));

    let summary = map.get(&serde_yaml::Value::String("summary".into()))
        .expect("summary present").as_str().unwrap();
    assert!(summary.contains("Creates a new cardholder"));

    let params = map.get(&serde_yaml::Value::String("parameters".into()))
        .expect("parameters present").as_sequence().unwrap();
    assert!(params.len() >= 4);

    let rb = map.get(&serde_yaml::Value::String("requestBody".into()))
        .expect("requestBody present").as_mapping().unwrap();
    let content = rb.get(&serde_yaml::Value::String("content".into())).unwrap()
        .as_mapping().unwrap();
    let app_json = content.get(&serde_yaml::Value::String("application/json".into())).unwrap()
        .as_mapping().unwrap();
    let schema = app_json.get(&serde_yaml::Value::String("schema".into())).unwrap()
        .as_mapping().unwrap();
    let r#ref = schema.get(&serde_yaml::Value::String("$ref".into())).unwrap()
        .as_str().unwrap();
    assert!(r#ref.starts_with("#/components/schemas/"), "expected $ref, got {}", r#ref);
    assert!(r#ref.contains("Cardholder"));

    let responses = map.get(&serde_yaml::Value::String("responses".into()))
        .expect("responses present").as_mapping().unwrap();
    assert!(responses.contains_key(&serde_yaml::Value::String("200".into())));
    assert!(responses.contains_key(&serde_yaml::Value::String("400".into())));
}

#[test]
fn components_schemas_emit_request_body_shapes() {
    let ep = cardholder_post_endpoint();
    let std_def = said_forge::OpenApiStandard::defaults();
    let comps = emit_components_schemas(&[ep], &std_def);
    let map = comps.as_mapping().expect("components is mapping");

    assert!(map.contains_key(&serde_yaml::Value::String("ApiError".into())));

    let req = map.iter()
        .find(|(k, _)| k.as_str().map(|s| s.contains("CreateCardholderRequest")).unwrap_or(false))
        .map(|(_, v)| v)
        .expect("CreateCardholderRequest schema present");
    let req_map = req.as_mapping().unwrap();
    assert_eq!(req_map.get(&serde_yaml::Value::String("type".into())).and_then(|v| v.as_str()),
        Some("object"));
    let props = req_map.get(&serde_yaml::Value::String("properties".into())).unwrap()
        .as_mapping().unwrap();
    assert!(props.contains_key(&serde_yaml::Value::String("id".into())));
    assert!(props.contains_key(&serde_yaml::Value::String("firstName".into())));
}

#[test]
fn fixes_md_lists_path_param_rewrites() {
    use said_forge::fitter::{write_fixes, FitReport, StandardNormalisation};
    let mut report = FitReport::default();
    report.standard_normalisations.push(StandardNormalisation {
        from: "/cards/{card_id}".into(),
        to: "/cards/{cardId}".into(),
        reason: "paths.param_casing = camel_case_id".into(),
    });
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("fixes.md");
    write_fixes(&report, "TXN", &path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# Standard normalisations — TXN"));
    assert!(text.contains("`/cards/{card_id}`"));
    assert!(text.contains("`/cards/{cardId}`"));
    assert!(text.contains("paths.param_casing = camel_case_id"));
}

#[test]
fn fixes_md_not_written_when_no_rewrites() {
    use said_forge::fitter::{write_fixes, FitReport};
    let report = FitReport::default();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("fixes.md");
    write_fixes(&report, "TXN", &path).unwrap();
    assert!(!path.exists(), "fixes.md should not exist when no rewrites occurred");
}

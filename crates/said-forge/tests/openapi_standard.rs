use said_forge::openapi_standard::{BraceStyle, OpenApiStandard, ParamCasing};

#[test]
fn defaults_match_current_hard_coded_values() {
    let s = OpenApiStandard::defaults();
    assert!(matches!(s.paths.param_casing, ParamCasing::CamelCaseId));
    assert!(matches!(s.brace_style.output, BraceStyle::Single));
    let header_names: Vec<&str> = s.parameters.standard_headers.headers.keys()
        .map(|s| s.as_str()).collect();
    assert!(header_names.contains(&"requestId"));
    assert!(header_names.contains(&"correlationId"));
    assert!(header_names.contains(&"x-api-version"));
    assert!(header_names.contains(&"Ocp-Apim-Subscription-Key"));
    assert_eq!(s.responses.success_code, "200");
    assert_eq!(s.responses.error_code, "400");
    assert_eq!(s.responses.shared_error_schema, "ApiError");
    assert!(s.components.include_api_error);
    assert!(s.content_types.request.iter().any(|c| c == "application/json"));
}

#[test]
fn loads_workspace_toml_when_present() {
    use std::fs;
    let tmp = tempfile::tempdir().unwrap();
    let forge_dir = tmp.path().join(".forge");
    fs::create_dir(&forge_dir).unwrap();
    fs::write(forge_dir.join("openapi-standard.toml"),
        r#"
[paths]
param_casing = "preserve"
collection_casing = "lowercase"
log_normalisations = true

[brace_style]
output = "single"

[parameters.standard_headers]
required = ["requestId"]
optional = []

[parameters.standard_headers.requestId]
type = "string"
format = "uuid"
description = "Trace id."

[parameters.path_params]
uuid_suffix = "Id"

[responses]
success_code = "200"
success_description = "OK"
error_code = "400"
error_description = "Error"
shared_error_schema = "ApiError"

[components]
include_api_error = true

[content_types]
request = ["application/json"]
response = ["application/json"]
"#).unwrap();

    let s = OpenApiStandard::load(tmp.path(), None).unwrap();
    assert!(matches!(s.paths.param_casing, ParamCasing::Preserve));
    assert_eq!(s.parameters.standard_headers.headers.len(), 1);
}

#[test]
fn client_overrides_layer_atop_workspace_field_by_field() {
    use std::fs;
    let tmp = tempfile::tempdir().unwrap();
    let forge_dir = tmp.path().join(".forge");
    fs::create_dir(&forge_dir).unwrap();
    fs::write(forge_dir.join("openapi-standard.toml"),
        r#"
[paths]
param_casing = "camel_case_id"
collection_casing = "lowercase"
log_normalisations = true
[brace_style]
output = "single"
[parameters.standard_headers]
required = ["requestId", "x-api-version"]
optional = ["correlationId"]
[parameters.standard_headers.requestId]
type = "string"
format = "uuid"
description = "Trace."
[parameters.standard_headers.correlationId]
type = "string"
format = "uuid"
description = "Correlation."
[parameters.standard_headers."x-api-version"]
type = "string"
description = "Version."
[parameters.path_params]
uuid_suffix = "Id"
[responses]
success_code = "200"
success_description = "OK"
error_code = "400"
error_description = "Err"
shared_error_schema = "ApiError"
[components]
include_api_error = true
[content_types]
request = ["application/json"]
response = ["application/json"]
"#).unwrap();
    let client_dir = tmp.path().join("1-ground-truth").join("Vivere");
    fs::create_dir_all(&client_dir).unwrap();
    fs::write(client_dir.join(".forge-overrides.toml"),
        r#"
[paths]
param_casing = "preserve"
"#).unwrap();

    let s = OpenApiStandard::load(tmp.path(), Some("Vivere")).unwrap();
    assert!(matches!(s.paths.param_casing, ParamCasing::Preserve));
    assert!(matches!(s.paths.collection_casing, said_forge::openapi_standard::CollectionCasing::Lowercase));
    assert_eq!(s.parameters.standard_headers.headers.len(), 3);
}

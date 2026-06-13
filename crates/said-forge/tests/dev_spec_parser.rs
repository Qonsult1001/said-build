use said_forge::dev_spec::parser::parse_endpoint_file;
use said_forge::OpenApiStandard;

#[test]
fn parses_post_cardholders_create() {
    let path = "tests/fixtures/dev_spec/POST-cardholders-createCardholder.md";
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(std::path::Path::new(path), &std_def).unwrap();

    assert_eq!(ep.method, "POST");
    // POST creates a single resource → singular per the
    // collection_pluralisation rule.
    assert_eq!(ep.path, "/cardholder");
    assert!(ep.summary.contains("Creates a new cardholder"));

    // Body should have parsed at least the top-level fields.
    let body = ep.request_body.expect("request body present");
    assert_eq!(body.ty, "object");
    assert!(body.properties.contains_key("id"));
    assert!(body.properties.contains_key("personalDetails"));
    assert!(body.properties.contains_key("billingAddress"));

    // Nested object — personalDetails must have firstName, lastName.
    let pd = body.properties.get("personalDetails").unwrap();
    assert_eq!(pd.ty, "object");
    assert!(pd.properties.contains_key("firstName"));
    assert!(pd.properties.contains_key("lastName"));

    // UUID format on the id field.
    let id_field = body.properties.get("id").unwrap();
    assert_eq!(id_field.ty, "string");
    assert_eq!(id_field.format.as_deref(), Some("uuid"));
}

#[test]
fn extracts_method_and_path_from_filename() {
    use said_forge::dev_spec::parser::method_path_from_filename;

    let std_def = OpenApiStandard::defaults();

    let (m, p) = method_path_from_filename("GET-cardholders-_cardholder_id_-transitions.md", &std_def).unwrap();
    assert_eq!(m, "GET");
    // Path-param names follow the BRU/OpenAPI standard: camelCase
    // ending in `Id` (not snake_case). Filename uses `_cardholder_id_`
    // marker syntax; the parser normalises to `cardholderId`.
    assert_eq!(p, "/cardholders/{cardholderId}/transitions");

    let (m, p) = method_path_from_filename("POST-cardholders-createCardholder.md", &std_def).unwrap();
    assert_eq!(m, "POST");
    assert_eq!(p, "/cardholders");

    let (m, p) = method_path_from_filename("PATCH-cardholders-updateCardholder.md", &std_def).unwrap();
    assert_eq!(m, "PATCH");
    assert_eq!(p, "/cardholders");
}

#[test]
fn walks_dev_planning_directory() {
    use said_forge::dev_spec::parser::walk_dev_spec_dir;

    // Real Dev Planning directory — we ship the test against the
    // workspace's actual contract.
    let root = std::path::Path::new("../../dtcard/4-expectations/Dev Planning");
    if !root.exists() {
        eprintln!("skipping: dev planning dir not present in this checkout");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let endpoints = walk_dev_spec_dir(root, &std_def).unwrap();
    assert!(endpoints.len() >= 50, "expected dozens of endpoints, got {}", endpoints.len());

    // Cardholder POST must be there. Path is singular under the
    // collection_pluralisation rule (POST creates one resource).
    let create_cardholder = endpoints.iter()
        .find(|e| e.method == "POST" && e.path == "/cardholder")
        .expect("POST /cardholder not found");
    assert!(create_cardholder.request_body.is_some());

    // Output is sorted (deterministic).
    for w in endpoints.windows(2) {
        let a = format!("{} {}", w[0].method, w[0].path);
        let b = format!("{} {}", w[1].method, w[1].path);
        assert!(a <= b, "endpoints not sorted: {a} > {b}");
    }
}

#[test]
fn preserve_mode_keeps_snake_case_path_params_verbatim() {
    use said_forge::dev_spec::parser::method_path_from_filename;
    use said_forge::openapi_standard::{OpenApiStandard, ParamCasing};

    let mut s = OpenApiStandard::defaults();
    s.paths.param_casing = ParamCasing::Preserve;

    let (_m, p) = method_path_from_filename(
        "GET-cardholders-_cardholder_id_-transitions.md", &s).unwrap();
    assert_eq!(p, "/cardholders/{cardholder_id}/transitions",
        "preserve mode should keep snake_case");
}

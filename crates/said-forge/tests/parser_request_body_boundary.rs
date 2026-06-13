//! Tests that the parser correctly bounds a request body's yaml block
//! and does not slurp adjacent response-envelope content. Regression
//! guard for the bug where `PutAccountRequest` ended up containing
//! `result`/`error`/`dateTime` because the Dev Spec markdown forgot
//! to close its yaml fence and the parser kept walking until it found
//! the next ` ``` ` 195 lines later.

use said_forge::dev_spec::parser::parse_endpoint_file;
use said_forge::OpenApiStandard;
use std::path::Path;

#[test]
fn put_account_request_body_excludes_response_envelope() {
    // Test against the real Dev Spec file that exhibited the bug.
    let path = Path::new(
        "../../dtcard/4-expectations/Dev Planning/Accounts/PUT-accounts-_account_id_.md"
    );
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(path, &std_def).unwrap();
    assert_eq!(ep.method, "PUT");
    let body = ep.request_body.expect("request body present");

    // Request body must contain the actual fields (post body-casing).
    let keys: Vec<&str> = body.properties.keys().map(|s| s.as_str()).collect();
    assert!(keys.contains(&"name"), "name missing; got keys: {keys:?}");
    assert!(
        keys.contains(&"accountOwnerId") || keys.contains(&"account_owner_id"),
        "accountOwnerId missing; got keys: {keys:?}"
    );

    // After the parser fix + strip rule, request body MUST NOT include
    // any response-envelope fields. Strip rule catches what would
    // otherwise leak; parser fix means they shouldn't even reach the
    // schema in the first place.
    // `id` is also stripped for PUT/PATCH/DELETE per R4 (the resource
    // id is in the URL `{accountId}` path-param, not the body).
    for forbidden in ["id", "result", "error", "dateTime",
                      "requestId", "correlationId", "responseId"] {
        assert!(
            !keys.contains(&forbidden),
            "PUT /accounts/{{accountId}} request body must NOT include `{forbidden}`; got: {keys:?}"
        );
    }
}

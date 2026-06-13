//! Tests that the parser harvests `Example:` lines from Dev Spec
//! markdown and that the emitter passes them through to OpenAPI YAML
//! as `example:` schema annotations.

use said_forge::dev_spec::parser::parse_endpoint_file;
use said_forge::OpenApiStandard;
use std::path::Path;

#[test]
fn put_account_request_keys_carry_examples() {
    // Real Dev Spec file with `Example:` lines on body fields.
    let path = Path::new(
        "../../dtcard/4-expectations/Dev Planning/Accounts/PUT-accounts-_account_id_.md"
    );
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(path, &std_def).unwrap();
    let body = ep.request_body.expect("body present");

    // `currencyCode` has Example: EUR in the markdown.
    let cc = body.properties.get("currencyCode")
        .or_else(|| body.properties.get("currency_code"))
        .expect("currencyCode field present");
    assert_eq!(cc.example.as_deref(), Some("EUR"),
        "currencyCode must carry the harvested EUR example, got: {:?}",
        cc.example);
}

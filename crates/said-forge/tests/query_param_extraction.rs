//! Tests that the parser extracts `### Parameters` → `#### <name>`
//! blocks (query/header params) from Dev Spec markdown, with their
//! schema (type, default, minimum, maximum) and example values.

use said_forge::dev_spec::parser::parse_endpoint_file;
use said_forge::OpenApiStandard;
use std::path::Path;

#[test]
fn get_accounts_carries_pagination_query_params() {
    let path = Path::new(
        "../../dtcard/4-expectations/Dev Planning/Accounts/GET-accounts.md"
    );
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(path, &std_def).unwrap();

    let names: Vec<&str> = ep.path_params.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"page"), "page missing; got: {names:?}");
    assert!(names.contains(&"limit"), "limit missing; got: {names:?}");
    assert!(names.contains(&"sort"), "sort missing; got: {names:?}");

    // page: integer, default 1, minimum 1
    let page = ep.path_params.iter().find(|p| p.name == "page").unwrap();
    assert_eq!(page.location, "query", "page location wrong: {:?}", page.location);
    assert_eq!(page.ty, "integer");
    assert_eq!(page.default.as_deref(), Some("1"));
    assert_eq!(page.minimum.as_deref(), Some("1"));
    assert!(!page.required, "page must be optional");

    // limit: integer, default 20, minimum 1, maximum 100
    let limit = ep.path_params.iter().find(|p| p.name == "limit").unwrap();
    assert_eq!(limit.location, "query");
    assert_eq!(limit.ty, "integer");
    assert_eq!(limit.default.as_deref(), Some("20"));
    assert_eq!(limit.minimum.as_deref(), Some("1"));
    assert_eq!(limit.maximum.as_deref(), Some("100"));

    // sort: string, optional, no min/max
    let sort = ep.path_params.iter().find(|p| p.name == "sort").unwrap();
    assert_eq!(sort.location, "query");
    assert_eq!(sort.ty, "string");
    assert!(sort.minimum.is_none());
    assert!(sort.maximum.is_none());
}

#[test]
fn get_accounts_response_carries_paginated_shape() {
    let path = Path::new(
        "../../dtcard/4-expectations/Dev Planning/Accounts/GET-accounts.md"
    );
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(path, &std_def).unwrap();
    let body = ep.response_body.expect("response body present");
    let result = body.properties.get("result").expect("result present");

    // `data` array (post body-casing rewrite)
    let data_keys: Vec<&str> = result.properties.keys().map(|s| s.as_str()).collect();
    assert!(data_keys.contains(&"data"),
        "result.data missing; got: {data_keys:?}");
    assert!(data_keys.contains(&"pagination"),
        "result.pagination missing; got: {data_keys:?}");

    // pagination.total / page / limit / pages
    let pagination = result.properties.get("pagination").unwrap();
    let pg_keys: Vec<&str> = pagination.properties.keys().map(|s| s.as_str()).collect();
    assert!(pg_keys.contains(&"total"), "pagination.total missing; got: {pg_keys:?}");
    assert!(pg_keys.contains(&"page"), "pagination.page missing; got: {pg_keys:?}");
    assert!(pg_keys.contains(&"limit"), "pagination.limit missing; got: {pg_keys:?}");
    assert!(pg_keys.contains(&"pages"), "pagination.pages missing; got: {pg_keys:?}");
}

#[test]
fn get_account_by_id_does_not_carry_query_params() {
    // Pagination only appears in GET /<collection> endpoints. The
    // by-id GET (GET /accounts/{account_id}) must NOT have page/limit/sort.
    let path = Path::new(
        "../../dtcard/4-expectations/Dev Planning/Accounts/GET-accounts-_account_id_.md"
    );
    if !path.exists() {
        eprintln!("fixture missing — skipping");
        return;
    }
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(path, &std_def).unwrap();
    let names: Vec<&str> = ep.path_params.iter().map(|p| p.name.as_str()).collect();
    for forbidden in ["page", "limit", "sort"] {
        assert!(!names.contains(&forbidden),
            "by-id GET must not carry {forbidden}; got: {names:?}");
    }
    // But it must still have the path-param.
    assert!(
        names.contains(&"accountId") || names.contains(&"account_id"),
        "accountId path-param missing; got: {names:?}"
    );
}

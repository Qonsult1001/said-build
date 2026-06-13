//! Per-entity L3 lifecycle runner. Drives the synthetic HTTP server
//! with EVERY Bruno fixture in this entity's folder, in a fixed bucket
//! order, capturing ids and propagating them to later fixtures.
//!
//! Bucket order per entity:
//!
//!   1. Root POST                  POST /<entity>
//!   2. GET by id                  GET  /<entity>/{ID}
//!   3. PUT/PATCH by id            PUT  /<entity>/{ID}     (or PATCH)
//!   4. GET by id (re-read)        same fixture as step 2, only when 3 ran
//!   5. Sub-resource ops           anything else in this folder, in
//!                                 alphabetical fixture-name order
//!   6. Collection LIST            GET /<entity>s
//!
//! New behaviours vs the old harness:
//! - Drives from EVERY fixture in the folder, not a hand-coded 5-step shape
//!   (so e.g. `CreateCardTransition.bru` and `GetAllCardholderCards.bru`
//!   actually run instead of being silently skipped).
//! - Validates each fixture's `(method, normalised path)` against the
//!   OpenAPI spec; mismatches are recorded as failed ops, not skipped.
//! - Continues past failures within an entity — the operator sees the
//!   full picture in one run.

use crate::test_harness::bru_parse::BruRequest;
use crate::test_harness::report::{EntityResult, OperationResult};
use std::collections::{BTreeMap, BTreeSet};

/// Set of `(METHOD, normalised_path_template)` pairs harvested from the
/// OpenAPI spec. Used by the lifecycle to flag fixtures whose URL does
/// not resolve to any spec endpoint.
///
/// Path templates are kept as-authored in the spec (camelCase params,
/// singular roots where the standard says so). The lifecycle normalises
/// fixture URLs to the same shape (`{whatever}` → `{ID}`) before
/// matching, but it ALSO retries with the original-cased param token
/// preserved, since the spec uses meaningful names like `{cardId}`.
pub type SpecPaths = BTreeSet<(String, String)>;

/// Build the spec-paths set from a parsed OpenAPI YAML document. The
/// tester uses this set to validate fixture URLs before EXECing.
pub fn spec_paths_from_yaml(spec: &serde_yaml::Value) -> SpecPaths {
    let mut out = SpecPaths::new();
    let paths = match spec.get("paths").and_then(|v| v.as_mapping()) {
        Some(m) => m,
        None => return out,
    };
    for (path_key, ops_val) in paths {
        let path_str = match path_key.as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let ops = match ops_val.as_mapping() {
            Some(m) => m,
            None => continue,
        };
        for (method_key, _) in ops {
            let method = match method_key.as_str() {
                Some(s) => s.to_uppercase(),
                None => continue,
            };
            if matches!(
                method.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
            ) {
                out.insert((method, path_str.clone()));
            }
        }
    }
    out
}

/// Fixture lifecycle bucket. The order of variants here equals the
/// run order — derive Ord and the harness sorts by bucket first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bucket {
    /// POST /<entity> — the create.
    RootPost,
    /// GET /<entity>/{ID} — the read.
    GetById,
    /// PUT or PATCH /<entity>/{ID} — the update.
    UpdateById,
    /// Sub-resource: same entity-prefix but more segments, OR cross-
    /// resource fixture authored in this entity's folder (e.g.
    /// `GetAllCardholderCards.bru` URL `/cardholder/{ID}/cards` lives
    /// in `Card/`).
    SubResource,
    /// GET /<entities> — the plural collection list.
    CollectionList,
    /// Anything else — kept so it still runs and reports.
    Other,
}

impl Bucket {
    /// Short stable label used in the markdown report. Not the only
    /// description — the runner enriches it with the fixture name and
    /// a path hint (`POST sub:transition`, `GET cross:cardholder/cards`).
    pub fn coarse_label(&self) -> &'static str {
        match self {
            Bucket::RootPost       => "POST create",
            Bucket::GetById        => "GET by id",
            Bucket::UpdateById     => "PUT update",
            Bucket::SubResource    => "sub-resource",
            Bucket::CollectionList => "GET list",
            Bucket::Other          => "other",
        }
    }
}

/// Strip query-string + lower-case + replace `{...}` placeholders with
/// the literal `{ID}`. Used by both the bucket classifier and the spec-
/// paths validator. Returns the segments split on `/` (empty segments
/// removed) and the rejoined path.
///
/// Handles a few authoring shapes: rendered HTTP URLs
/// (`http://127.0.0.1:5050/card`), Bruno-templated URLs with the
/// localBaseURL placeholder (`{{localBaseURL}}/card`), and plain paths
/// (`/card`). In all three cases the host/origin/template is stripped
/// and only the path segments remain.
pub fn normalise_path(raw: &str) -> (Vec<String>, String) {
    // 1. Strip a Bruno `{{...}}` template prefix (typically
    // `{{localBaseURL}}`) so the segment splitter doesn't see it as a
    // path-param. This must precede the `://` check because Bruno
    // templates don't include the scheme.
    let after_template = if let Some(rest) = raw.strip_prefix("{{") {
        match rest.find("}}") {
            Some(end) => &rest[end + 2..],
            None => raw,
        }
    } else {
        raw
    };
    // 2. Strip a real origin (`http://host:port/...`).
    let without_origin = match after_template.find("://") {
        Some(idx) => match after_template[idx + 3..].find('/') {
            Some(slash) => &after_template[idx + 3 + slash..],
            None => "/",
        },
        None => after_template,
    };
    let no_query = without_origin.split('?').next().unwrap_or("");
    let segs: Vec<String> = no_query
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let lc = s.to_lowercase();
            if lc.starts_with('{') && lc.ends_with('}') {
                "{ID}".to_string()
            } else if is_bare_uuid(&lc) {
                // Post-substitution URLs have bare UUIDs in place of
                // `{accountId}`. Collapse them to `{ID}` so the registry
                // lookup matches the bundle.toml-derived shape.
                "{ID}".to_string()
            } else {
                lc
            }
        })
        .collect();
    let joined = format!("/{}", segs.join("/"));
    (segs, joined)
}

/// UUID detector: 8-4-4-4-12 hex with dashes at fixed positions.
fn is_bare_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        let is_dash_pos = i == 8 || i == 13 || i == 18 || i == 23;
        if is_dash_pos {
            if *b != b'-' { return false; }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Classify a fixture into a bucket given the entity name (PascalCase).
pub fn classify_bucket(method: &str, raw_url: &str, entity: &str) -> Bucket {
    use crate::dev_spec::parser::{pluralise_segment, singularise};
    let entity_lc = entity.to_lowercase();
    let entity_singular = singularise(&entity_lc);
    let entity_plural = pluralise_segment(&entity_singular);
    let method_uc = method.to_uppercase();
    let (segs, _) = normalise_path(raw_url);

    let is_root = |first: &str| {
        first == entity_lc || first == entity_singular || first == entity_plural
    };

    match (method_uc.as_str(), segs.as_slice()) {
        ("POST", [first]) if is_root(first) && first != &entity_plural
            => Bucket::RootPost,
        // Defensive: some Bruno files POST to the plural collection root.
        // Treat that as a root POST too (the spec validator will catch
        // a true mismatch).
        ("POST", [first]) if is_root(first) => Bucket::RootPost,

        ("GET", [first, id]) if is_root(first) && id == "{ID}"
            => Bucket::GetById,
        ("PUT" | "PATCH", [first, id]) if is_root(first) && id == "{ID}"
            => Bucket::UpdateById,
        ("GET", [first]) if first == &entity_plural
            => Bucket::CollectionList,
        // Single-segment plural with non-GET — probably a list-style
        // mutation (PATCH /cardholders, etc.) — treat as Other so it
        // still runs but doesn't masquerade as create/update/list.
        (_, _) if segs.len() <= 1 => Bucket::Other,
        // Anything else with this entity as first segment is a sub-resource.
        // Anything else with a DIFFERENT first segment is cross-resource —
        // we still group it under SubResource because the run-order rule
        // is identical (after the entity's own GET-by-id+update, before
        // the collection list).
        _ => Bucket::SubResource,
    }
}

/// Run the full lifecycle for a single entity. Public signature is
/// unchanged from the prior implementation — only the body has been
/// reworked.
///
/// `session_ids` carries captured ids across entity boundaries. After a
/// successful root POST the captured id is published under the entity's
/// PascalCase name (e.g. `"BinSponsor"` → uuid).
pub async fn run_for_entity(
    entity: &str,
    fixtures: &[BruRequest],
    base_url: &str,
    session_ids: &mut BTreeMap<String, String>,
    spec_paths: &SpecPaths,
) -> EntityResult {
    let mut result = EntityResult::new(entity);
    use crate::dev_spec::parser::{pluralise_segment, singularise};
    let entity_lc = entity.to_lowercase();
    let entity_plural = pluralise_segment(&entity_lc);
    let entity_singular = singularise(&entity_lc);

    // Filter to this entity's folder. Build-order names are singular
    // PascalCase (`BinSponsor`); Bruno authors sometimes pluralise
    // (`Bins`), sometimes don't.
    let folder_match = |bru: &&BruRequest| -> bool {
        let f = bru.entity_folder.to_lowercase();
        f == entity_lc || f == entity_plural || f == entity_singular
    };
    let entity_fixtures: Vec<&BruRequest> = fixtures.iter()
        .filter(folder_match)
        .collect();

    if entity_fixtures.is_empty() {
        result.skipped_reason = Some(format!(
            "no Bruno fixtures found in collection for {}", entity
        ));
        return result;
    }

    // Bucket every fixture. We enumerate to keep a deterministic
    // tie-breaker (file-name alphabetic via the upstream sort already
    // applied in load_collection).
    //
    // Negative fixtures (filename starts with `neg-`) always land in
    // `Bucket::Other` regardless of their URL shape, so they run AFTER
    // every positive bucket. This stops a `neg-CreateAccount-*.bru`
    // from being misread as THE create step (it'd block the GET-by-id
    // that depends on the real POST's response id).
    let mut bucketed: Vec<(Bucket, &BruRequest)> = entity_fixtures.iter()
        .map(|f| {
            let bucket = if f.is_negative() {
                Bucket::Other
            } else {
                classify_bucket(&f.method, &f.url, entity)
            };
            (bucket, *f)
        })
        .collect();

    // Build the run order. Buckets 1-4 honour the design rules; bucket 5
    // collects sub-resources in stable alphabetic name order; bucket 6
    // runs LIST last. Multiple fixtures in the same bucket all run.
    let root_post: Vec<&BruRequest> = bucketed.iter()
        .filter(|(b, _)| *b == Bucket::RootPost)
        .map(|(_, f)| *f).collect();
    let get_by_id: Vec<&BruRequest> = bucketed.iter()
        .filter(|(b, _)| *b == Bucket::GetById)
        .map(|(_, f)| *f).collect();
    let update: Vec<&BruRequest> = bucketed.iter()
        .filter(|(b, _)| *b == Bucket::UpdateById)
        .map(|(_, f)| *f).collect();
    bucketed.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    let sub_resource: Vec<&BruRequest> = bucketed.iter()
        .filter(|(b, _)| *b == Bucket::SubResource || *b == Bucket::Other)
        .map(|(_, f)| *f).collect();
    let list: Vec<&BruRequest> = bucketed.iter()
        .filter(|(b, _)| *b == Bucket::CollectionList)
        .map(|(_, f)| *f).collect();

    let client = reqwest::Client::new();
    let mut captured_id: Option<String> = None;

    // Bucket 1 — root POSTs. The FIRST successful POST publishes its
    // captured id to session_ids; subsequent root POSTs (e.g. a second
    // create variant) still run, but don't overwrite the published id.
    for req in &root_post {
        let label = format!("{} {}", Bucket::RootPost.coarse_label(),
            short_path_hint(&req.url));
        let op = exec_with_validation(
            &client, req, base_url, &captured_id, session_ids,
            &label, spec_paths,
        ).await;
        if op.passed && captured_id.is_none() {
            if let Some(body) = &op.response_body {
                if let Some(id) = extract_result_id(body) {
                    captured_id = Some(id.clone());
                    session_ids.insert(entity.to_string(), id);
                }
            }
        }
        result.operations.push(op);
    }

    // Bucket 2 — GET by id.
    for req in &get_by_id {
        let label = format!("{} {}", Bucket::GetById.coarse_label(),
            short_path_hint(&req.url));
        let op = exec_with_validation(
            &client, req, base_url, &captured_id, session_ids,
            &label, spec_paths,
        ).await;
        result.operations.push(op);
    }

    // Bucket 3 — PUT/PATCH by id.
    let mut any_update_ran = false;
    for req in &update {
        any_update_ran = true;
        let label = format!("{} {}", Bucket::UpdateById.coarse_label(),
            short_path_hint(&req.url));
        let op = exec_with_validation(
            &client, req, base_url, &captured_id, session_ids,
            &label, spec_paths,
        ).await;
        result.operations.push(op);
    }

    // Bucket 4 — re-read, only when an update fixture actually ran. No
    // point reading twice if there was nothing to confirm.
    if any_update_ran {
        for req in &get_by_id {
            let label = format!("GET re-read {}", short_path_hint(&req.url));
            let op = exec_with_validation(
                &client, req, base_url, &captured_id, session_ids,
                &label, spec_paths,
            ).await;
            result.operations.push(op);
        }
    }

    // Bucket 5 — sub-resources / cross-resource fixtures.
    // POST sub-resources publish their response id under the entity
    // segment immediately following `{xId}` in the URL (e.g.
    // `/cardholder/{cardholderId}/transition` POST → publish under
    // `Transition` so a sibling GET fixture using `{transitionId}` in
    // its URL can substitute it. Without this, GET-by-id sub-resources
    // can only use placeholder GUIDs that don't exist in the test DB.
    for req in &sub_resource {
        let kind = subresource_kind(&req.url, entity);
        let label = format!("{} {} {}", req.method.to_uppercase(), kind,
            short_path_hint(&req.url));
        let op = exec_with_validation(
            &client, req, base_url, &captured_id, session_ids,
            &label, spec_paths,
        ).await;
        if op.passed && req.method.eq_ignore_ascii_case("POST") {
            if let Some(body) = &op.response_body {
                if let Some(new_id) = extract_result_id(body) {
                    if let Some(sub_entity) = sub_resource_publish_key(&req.url) {
                        // Capture under PascalCase so build_substitutions
                        // produces all four casings (e.g. `transitionId`,
                        // `TransitionId`, `transitionid`, `transitionId`).
                        session_ids.entry(sub_entity).or_insert(new_id);
                    }
                }
            }
        }
        result.operations.push(op);
    }

    // Bucket 6 — LIST last.
    for req in &list {
        let label = format!("{} {}", Bucket::CollectionList.coarse_label(),
            short_path_hint(&req.url));
        let op = exec_with_validation(
            &client, req, base_url, &captured_id, session_ids,
            &label, spec_paths,
        ).await;
        result.operations.push(op);
    }

    result.finalise()
}

/// Run one fixture, but FIRST validate that its `(method, path)` resolves
/// to an OpenAPI spec entry. If not, record a failed op and skip the
/// HTTP call — there's no point hitting the synthetic server with a
/// fixture the spec doesn't model.
async fn exec_with_validation(
    client: &reqwest::Client,
    req: &BruRequest,
    base_url: &str,
    captured_id: &Option<String>,
    session_ids: &BTreeMap<String, String>,
    label: &str,
    spec_paths: &SpecPaths,
) -> OperationResult {
    if !fixture_in_spec(req, spec_paths) {
        // Render the URL as it would have been hit so the operator sees
        // the actual offending path in the report.
        let rendered_url = render_for_report(req, base_url, captured_id, session_ids);
        return OperationResult {
            label: label.to_string(),
            fixture_name: req.name.clone(),
            method: req.method.clone(),
            url: rendered_url.clone(),
            status_code: None,
            passed: false,
            failure_reason: Some(format!(
                "fixture URL not in spec: {} {}",
                req.method.to_uppercase(),
                strip_origin(&rendered_url),
            )),
            response_body: None,
            bundle: None,
        };
    }
    exec_request(client, req, base_url, captured_id, session_ids, label).await
}

/// Match a fixture to the spec by its `(method, path-template-shape)`.
/// Spec keys preserve named params (`{cardId}`); fixture URLs use any
/// names the author picked or even literal `{ID}` after rendering. We
/// normalise both sides to `<segment-count>:<entity-prefix>:<is-{}-mask>`
/// — i.e. compare by structure, not by param name.
fn fixture_in_spec(req: &BruRequest, spec_paths: &SpecPaths) -> bool {
    let method = req.method.to_uppercase();
    // Substitute `{{localBaseURL}}` cosmetically — we only need the path
    // shape. The URL after `{{localBaseURL}}` is what matters.
    let raw = strip_origin(&req.url);
    let (req_segs, _) = normalise_path(&raw);
    for (m, path) in spec_paths {
        if *m != method { continue; }
        let (spec_segs, _) = normalise_path(path);
        if shape_matches(&req_segs, &spec_segs) {
            return true;
        }
    }
    false
}

/// Two paths have the same shape iff the segment count matches AND each
/// segment is either the same literal OR both sides are `{ID}` (params).
fn shape_matches(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() { return false; }
    for (x, y) in a.iter().zip(b.iter()) {
        let xparam = x == "{id}" || x == "{ID}";
        let yparam = y == "{id}" || y == "{ID}";
        if xparam && yparam { continue; }
        if xparam || yparam { return false; }
        if x != y { return false; }
    }
    true
}

fn strip_origin(url: &str) -> String {
    // Bruno-templated URL (`{{localBaseURL}}/foo`) — drop the template
    // segment and keep the path.
    let after_template = if let Some(rest) = url.strip_prefix("{{") {
        match rest.find("}}") {
            Some(end) => rest[end + 2..].to_string(),
            None => url.to_string(),
        }
    } else {
        url.to_string()
    };
    match after_template.find("://") {
        Some(idx) => match after_template[idx + 3..].find('/') {
            Some(slash) => after_template[idx + 3 + slash..].to_string(),
            None => "/".to_string(),
        },
        None => after_template,
    }
}

/// Best-effort short hint of the URL path used in the operation label
/// to make the markdown report scannable. Returns the path part only,
/// truncated to 60 chars.
fn short_path_hint(raw_url: &str) -> String {
    let stripped = strip_origin(raw_url);
    if stripped.len() > 60 {
        format!("`{}…`", &stripped[..59])
    } else {
        format!("`{}`", stripped)
    }
}

/// Distinguish a sub-resource (`/card/{ID}/transition` for entity=Card)
/// from a cross-resource fixture (`/cardholder/{ID}/cards` for entity=Card).
///
/// Compares whole path segments — `cardholder` is NOT a `card` sub-
/// resource even though one string is a prefix of the other.
fn subresource_kind(url: &str, entity: &str) -> &'static str {
    let (segs, _) = normalise_path(url);
    let entity_lc = entity.to_lowercase();
    let entity_plural = crate::dev_spec::parser::pluralise_segment(&entity_lc);
    match segs.first() {
        Some(first) if first == &entity_lc || first == &entity_plural => "sub:",
        _ => "cross:",
    }
}

/// Send one HTTP request based on a Bruno fixture. Substitutes
/// `{{variable}}` placeholders in url/body using `pre_request_vars` +
/// the captured id (when available) + every prior entity's published id.
async fn exec_request(
    client: &reqwest::Client,
    req: &BruRequest,
    base_url: &str,
    captured_id: &Option<String>,
    session_ids: &BTreeMap<String, String>,
    label: &str,
) -> OperationResult {
    let substitutions = build_substitutions(
        req, base_url, captured_id, session_ids,
    );

    let url = render_template(&req.url, &substitutions);
    let body_json = req.body_json.as_ref().map(|b| render_template(b, &substitutions));
    let mut http_headers = reqwest::header::HeaderMap::new();
    for (k, v) in &req.headers {
        let rendered = render_template(v, &substitutions);
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(&rendered),
        ) {
            http_headers.insert(name, value);
        }
    }

    let method = match req.method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "PATCH" => reqwest::Method::PATCH,
        "DELETE" => reqwest::Method::DELETE,
        _ => reqwest::Method::GET,
    };

    // Bruno fixtures with `body: json` carry a JSON payload but the
    // `Content-Type` header is implied by the body-type declaration —
    // Bruno's own UI sets it before sending. Our harness must do the
    // same explicitly so ASP.NET MVC's `[FromBody]` model binder
    // accepts the request (otherwise the controller returns HTTP 415
    // Unsupported Media Type before the body even gets read).
    let mut builder = client.request(method, &url).headers(http_headers);
    if let Some(b) = body_json {
        // Trace body when SAID_HARNESS_DEBUG_BODY is set — helps diagnose
        // when MVC says `<param> field is required` despite the fixture
        // declaring `body: json` (placeholder substitution, content-type
        // miss, etc).
        if std::env::var("SAID_HARNESS_DEBUG_BODY").is_ok() {
            eprintln!("[harness body] {} {}\n{}", req.method, url, b);
        }
        builder = builder
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(b);
    }

    let result = builder.send().await;
    match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body_text = resp.text().await.unwrap_or_default();
            let envelope_ok = envelope_error_is_null(&body_text);

            // Negative fixtures invert the verdict: the API was supposed
            // to REJECT this request. A 4xx (or matching `expected_status`)
            // with the expected PrcCode is a *pass*; a 2xx is a *fail*
            // because we wanted rejection and got success.
            let (passed, failure_reason): (bool, Option<String>) = if req.is_negative() {
                let want_status = req.expected_status();
                let want_prc = req.expected_prc_code();
                let actual_prc = envelope_prc_code(&body_text);

                // Status check: when `expected_status` is set, require
                // exact match. Otherwise accept any 4xx as the rejection.
                let status_match = match want_status {
                    Some(want) => status == want,
                    None => status >= 400 && status < 500,
                };
                // PrcCode check: when declared, must match exactly.
                let prc_match = match (&want_prc, &actual_prc) {
                    (Some(want), Some(actual)) => want == actual,
                    (Some(_), None) => false,
                    (None, _) => true,
                };

                let pass = status_match && prc_match;
                let reason = if pass {
                    None
                } else if !status_match {
                    Some(format!(
                        "negative fixture expected status {} but got {}: {}",
                        want_status.map(|s| s.to_string()).unwrap_or_else(|| "4xx".into()),
                        status,
                        body_text.chars().take(200).collect::<String>()
                    ))
                } else {
                    // prc mismatch
                    Some(format!(
                        "negative fixture expected PrcCode {} but got {}: {}",
                        want_prc.unwrap_or_else(|| "<any>".into()),
                        actual_prc.unwrap_or_else(|| "<none>".into()),
                        body_text.chars().take(200).collect::<String>()
                    ))
                };
                (pass, reason)
            } else {
                // Positive (default) fixture: 2xx + envelope.error == null.
                let pass = status < 400 && envelope_ok;
                let reason = if pass {
                    None
                } else if status >= 400 {
                    Some(format!(
                        "HTTP {}: {}",
                        status,
                        body_text.chars().take(200).collect::<String>()
                    ))
                } else if !envelope_ok {
                    Some(format!(
                        "envelope.error not null: {}",
                        body_text.chars().take(200).collect::<String>()
                    ))
                } else {
                    None
                };
                (pass, reason)
            };

            OperationResult {
                label: label.to_string(),
                fixture_name: req.name.clone(),
                method: req.method.clone(),
                url: url.clone(),
                status_code: Some(status),
                passed,
                failure_reason,
                response_body: Some(body_text),
                bundle: None,
            }
        }
        Err(e) => OperationResult {
            label: label.to_string(),
            fixture_name: req.name.clone(),
            method: req.method.clone(),
            url: url.clone(),
            status_code: None,
            passed: false,
            failure_reason: Some(format!("HTTP request failed: {}", e)),
            response_body: None,
            bundle: None,
        },
    }
}

/// Build the substitution map shared across url/body/header rendering
/// AND the spec-mismatch reporter (so the report shows the URL the
/// runner WOULD have hit).
fn build_substitutions(
    req: &BruRequest,
    base_url: &str,
    captured_id: &Option<String>,
    session_ids: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut subs = req.pre_request_vars.clone();
    subs.insert("localBaseURL".to_string(), base_url.to_string());
    // Precedence rule:
    //   - Positive fixtures (the default) → SESSION ids win. Most
    //     fixtures carry a hardcoded fallback `accountId`/`cardholderId`
    //     so they're usable from Bruno's UI without a session — when
    //     the harness propagates a real id from a parent POST, that
    //     real id must override the stale hardcoded value.
    //   - Negative fixtures (`neg-*.bru`) → FIXTURE-LOCAL vars win.
    //     They need to plant deliberately-wrong values (a non-existent
    //     accountId, a bad UUID, etc.) and the session map would
    //     otherwise overwrite them with the just-created entity's id.
    let session_wins = !req.is_negative();
    // Bundle name → entity-name aliases when they differ.
    // `Notification` bundle owns the `Webhook` entity; `MerchantControl`
    // bundle has both `MerchantControl` and `MerchantControlGroup`; etc.
    // Each bundle name maps to one or more placeholder roots so a POST
    // capture publishes under EVERY name a fixture might use.
    let entity_aliases: &[(&str, &[&str])] = &[
        ("Notification", &["Webhook", "Notification"]),
        ("MerchantControl", &["MerchantControl", "MerchantControlGroup"]),
        ("DelegatedApprovalClientRequest", &["DelegatedApprovalClientRequest", "DACR"]),
    ];
    for (entity, id) in session_ids {
        // Determine the placeholder-name roots to publish under.
        let aliases: Vec<String> = entity_aliases
            .iter()
            .find(|(b, _)| b.eq_ignore_ascii_case(entity))
            .map(|(_, a)| a.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| vec![entity.clone()]);

        for root in &aliases {
            let root_lc = root.to_lowercase();
            let camel = format!("{}{}Id", &root[..1].to_lowercase(), &root[1..]);
            let pascal = format!("{}Id", root);
            let lower = format!("{}id", root_lc);
            let lower_camel_id = format!("{}Id", root_lc);
            for k in [&camel, &pascal, &lower, &lower_camel_id] {
                if session_wins {
                    subs.insert(k.clone(), id.clone());
                } else {
                    subs.entry(k.clone()).or_insert_with(|| id.clone());
                }
            }
        }
    }
    if let Some(id) = captured_id {
        for k in ["id", "ID", "Id"] {
            if session_wins {
                subs.insert(k.to_string(), id.clone());
            } else {
                subs.entry(k.to_string()).or_insert_with(|| id.clone());
            }
        }
    }
    subs
}

/// Render the URL the way `exec_request` would, used by the spec-
/// mismatch reporter so the markdown shows the actual offending URL.
fn render_for_report(
    req: &BruRequest,
    base_url: &str,
    captured_id: &Option<String>,
    session_ids: &BTreeMap<String, String>,
) -> String {
    let subs = build_substitutions(req, base_url, captured_id, session_ids);
    render_template(&req.url, &subs)
}

fn render_template(s: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = s.to_string();
    for (k, v) in vars {
        let needle = format!("{{{{{}}}}}", k);
        out = out.replace(&needle, v);
    }
    for (k, v) in vars {
        let needle = format!("{{{}}}", k);
        out = out.replace(&needle, v);
    }
    while let Some(start) = out.find("{{$guid}}") {
        let g = uuid::Uuid::new_v4().to_string();
        out.replace_range(start..start + "{{$guid}}".len(), &g);
    }
    out
}

fn envelope_error_is_null(body: &str) -> bool {
    let v: Result<serde_json::Value, _> = serde_json::from_str(body);
    match v {
        Ok(json) => {
            let err = json.get("error").or_else(|| json.get("Error"));
            match err {
                None => true,
                Some(serde_json::Value::Null) => true,
                Some(_) => false,
            }
        }
        Err(_) => true,
    }
}

/// Extract the PrcCode from a 4xx response envelope. Two shapes are
/// emitted by the framework's procs:
///
///   1. **Domain-handled error** (the C# Domain layer caught a non-2xx
///      Response_Code from the proc and turned it into the API's
///      conventional envelope shape):
///      `{"error": {"code": "1000", "description": "..."}}`
///
///   2. **Proc-internal error envelope** (raw proc Result projection,
///      surfaces directly when the Domain layer hits an exception
///      decoding the proc output):
///      `{"Result": {"PrcCode": "1000", "PrcDesc": "..."}}`
///
/// Returns the code as a string (most are numeric like `"1000"` but a
/// few framework codes are negative integers or alphanumeric — keeping
/// it text-shaped avoids parsing surprises). Returns `None` when no
/// PrcCode can be located.
fn envelope_prc_code(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    // Shape 1: envelope.error.code (camelCase or PascalCase).
    for k in ["error", "Error"] {
        if let Some(err) = json.get(k) {
            for ck in ["code", "Code"] {
                if let Some(code) = err.get(ck) {
                    if let Some(s) = code.as_str() {
                        return Some(s.to_string());
                    }
                    if let Some(n) = code.as_i64() {
                        return Some(n.to_string());
                    }
                }
            }
        }
    }
    // Shape 2: result.PrcCode (legacy numeric prc envelope) +
    // result.errorCode (new erc string envelope). Both ride under
    // the same `result` wrapper; the proc decides which it emits.
    for k in ["result", "Result"] {
        if let Some(res) = json.get(k) {
            for ck in ["PrcCode", "prcCode", "prccode", "errorCode", "ErrorCode", "errorcode"] {
                if let Some(code) = res.get(ck) {
                    if let Some(s) = code.as_str() {
                        return Some(s.to_string());
                    }
                    if let Some(n) = code.as_i64() {
                        return Some(n.to_string());
                    }
                }
            }
        }
    }
    None
}

/// For a sub-resource POST URL like `/cardholder/{cardholderId}/transition`,
/// return the PascalCase singular entity to publish into session_ids
/// (`Transition`). When `build_substitutions` later runs, it'll
/// produce `transitionId` / `TransitionId` / `transitionid` casings,
/// any of which a sibling GET fixture's `{transitionId}` URL token
/// can resolve against.
///
/// Returns `None` for non-singular-create shapes (lists, by-id GETs,
/// etc.) — only POST-create-one URLs end with a non-param singular
/// segment that's safe to publish.
fn sub_resource_publish_key(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let segs: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty() && !s.contains("{{"))
        .collect();
    let last = segs.last()?;
    if last.starts_with('{') && last.ends_with('}') {
        return None;
    }
    if segs.len() < 3 {
        return None;
    }
    let prev = segs.get(segs.len() - 2)?;
    if !(prev.starts_with('{') && prev.ends_with('}')) {
        return None;
    }
    let mut pascal = String::with_capacity(last.len());
    let mut upper_next = true;
    for c in last.chars() {
        if c == '_' || c == '-' {
            upper_next = true;
        } else if upper_next {
            pascal.extend(c.to_uppercase());
            upper_next = false;
        } else {
            pascal.push(c);
        }
    }
    Some(pascal)
}

fn extract_result_id(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let result = json.get("result")
        .or_else(|| json.get("Result"))?;
    for id_key in ["id", "Id", "ID"] {
        if let Some(id) = result.get(id_key).and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
    }
    // Some procs return the new entity's id under an entity-prefixed
    // key like `transitionId`, `cardId`, etc. Walk top-level result
    // keys looking for any string ending in "Id"/"_id" that parses
    // as a UUID.
    if let Some(obj) = result.as_object() {
        for (k, v) in obj {
            let lk = k.to_ascii_lowercase();
            if (lk.ends_with("id") || lk.ends_with("_id")) && v.is_string() {
                if let Some(s) = v.as_str() {
                    if uuid::Uuid::parse_str(s).is_ok() {
                        return Some(s.to_string());
                    }
                }
            }
        }
        for (_, v) in obj {
            for id_key in ["id", "Id", "ID"] {
                if let Some(id) = v.get(id_key).and_then(|x| x.as_str()) {
                    return Some(id.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, url: &str, name: &str, folder: &str) -> BruRequest {
        BruRequest {
            name: name.to_string(),
            entity_folder: folder.to_string(),
            method: method.to_string(),
            url: url.to_string(),
            headers: BTreeMap::new(),
            body_json: None,
            pre_request_vars: BTreeMap::new(),
            seq: 0,
        }
    }

    #[test]
    fn classify_root_post() {
        let r = req("POST", "{{localBaseURL}}/card", "CreateCard", "Card");
        assert_eq!(classify_bucket(&r.method, &r.url, "Card"), Bucket::RootPost);
    }

    #[test]
    fn classify_get_by_id() {
        let r = req("GET", "{{localBaseURL}}/card/{cardId}", "GetCardById", "Card");
        assert_eq!(classify_bucket(&r.method, &r.url, "Card"), Bucket::GetById);
    }

    #[test]
    fn classify_put_by_id() {
        let r = req("PUT", "{{localBaseURL}}/binsponsor/{binSponsorId}", "UpdateBinSponsor", "BinSponsor");
        assert_eq!(classify_bucket(&r.method, &r.url, "BinSponsor"), Bucket::UpdateById);
    }

    #[test]
    fn classify_patch_by_id() {
        let r = req("PATCH", "{{localBaseURL}}/account/{accountId}", "PatchAccount", "Account");
        assert_eq!(classify_bucket(&r.method, &r.url, "Account"), Bucket::UpdateById);
    }

    #[test]
    fn classify_collection_list() {
        let r = req("GET", "{{localBaseURL}}/cards", "GetAllCards", "Card");
        assert_eq!(classify_bucket(&r.method, &r.url, "Card"), Bucket::CollectionList);
    }

    #[test]
    fn classify_sub_resource_post() {
        let r = req("POST", "{{localBaseURL}}/card/{cardId}/transition", "CreateCardTransition", "Card");
        assert_eq!(classify_bucket(&r.method, &r.url, "Card"), Bucket::SubResource);
    }

    #[test]
    fn classify_sub_resource_get_collection() {
        let r = req("GET", "{{localBaseURL}}/card/{cardId}/transitions", "GetCardTransitions", "Card");
        assert_eq!(classify_bucket(&r.method, &r.url, "Card"), Bucket::SubResource);
    }

    #[test]
    fn classify_cross_resource() {
        // GetAllCardholderCards lives in Card folder but its URL hangs
        // off /cardholder. Expected: SubResource (the run-order rule
        // is identical; the label distinguishes sub: vs cross:).
        let r = req("GET", "{{localBaseURL}}/cardholder/{cardHolderId}/cards", "GetAllCardholderCards", "Card");
        assert_eq!(classify_bucket(&r.method, &r.url, "Card"), Bucket::SubResource);
    }

    #[test]
    fn classify_query_string_stripped() {
        let r = req("GET", "{{localBaseURL}}/cardholder/{cardHolderId}/cards?page=1&limit=10", "GetAllCardholderCards", "Card");
        assert_eq!(classify_bucket(&r.method, &r.url, "Card"), Bucket::SubResource);
    }

    #[test]
    fn classify_plural_post_treated_as_root() {
        // Defensive: some Bruno files POST to /<entityPlural>. Treat
        // it as root POST so it still runs; spec validator decides.
        let r = req("POST", "{{localBaseURL}}/cardholders", "CreateCardholder", "Cardholder");
        assert_eq!(classify_bucket(&r.method, &r.url, "Cardholder"), Bucket::RootPost);
    }

    #[test]
    fn classify_plural_get_is_collection_list() {
        let r = req("GET", "{{localBaseURL}}/cardholders", "ListCardholders", "Cardholder");
        assert_eq!(classify_bucket(&r.method, &r.url, "Cardholder"), Bucket::CollectionList);
    }

    #[test]
    fn shape_matches_handles_param_name_drift() {
        let req_segs: Vec<String> = vec!["card".into(), "{ID}".into()];
        let spec_segs: Vec<String> = vec!["card".into(), "{ID}".into()];
        assert!(shape_matches(&req_segs, &spec_segs));
    }

    #[test]
    fn shape_matches_rejects_segment_count_mismatch() {
        let a: Vec<String> = vec!["card".into(), "{ID}".into()];
        let b: Vec<String> = vec!["card".into(), "{ID}".into(), "transition".into()];
        assert!(!shape_matches(&a, &b));
    }

    #[test]
    fn fixture_in_spec_recognises_param_name_drift() {
        let mut spec = SpecPaths::new();
        spec.insert(("GET".to_string(), "/card/{cardId}".to_string()));
        let r = req("GET", "{{localBaseURL}}/card/{someOtherName}", "GetCardById", "Card");
        assert!(fixture_in_spec(&r, &spec));
    }

    #[test]
    fn fixture_in_spec_rejects_unknown_path() {
        let mut spec = SpecPaths::new();
        spec.insert(("GET".to_string(), "/card/{cardId}".to_string()));
        let r = req("GET", "{{localBaseURL}}/card/{cardId}/nope", "WhoKnows", "Card");
        assert!(!fixture_in_spec(&r, &spec));
    }

    #[test]
    fn fixture_in_spec_rejects_wrong_method() {
        let mut spec = SpecPaths::new();
        spec.insert(("GET".to_string(), "/card".to_string()));
        let r = req("DELETE", "{{localBaseURL}}/card", "Bogus", "Card");
        assert!(!fixture_in_spec(&r, &spec));
    }

    #[test]
    fn normalise_path_strips_query() {
        let (segs, joined) = normalise_path("/cardholder/{ID}/cards?page=1");
        assert_eq!(segs, vec!["cardholder".to_string(), "{ID}".into(), "cards".into()]);
        assert_eq!(joined, "/cardholder/{ID}/cards");
    }

    #[test]
    fn normalise_path_strips_origin() {
        let (segs, _) = normalise_path("http://127.0.0.1:5050/card/abc");
        assert_eq!(segs, vec!["card".to_string(), "abc".into()]);
    }

    #[test]
    fn subresource_kind_self_vs_cross() {
        assert_eq!(subresource_kind("/card/{ID}/transition", "Card"), "sub:");
        assert_eq!(subresource_kind("/cardholder/{ID}/cards", "Card"), "cross:");
    }
}

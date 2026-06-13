//! Synthetic HTTP server backed by direct stored-procedure execution.
//! Routes incoming `(method, path)` to the proc declared in the
//! OpenAPI spec, EXECs it, returns the proc's `Response_Message` JSON
//! envelope as the response body.

use crate::sql_verify::SandboxInfo;
use crate::test_harness::proc_invoke::{self, InvocationEnvelope, ProcBindings};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Clone)]
struct AppState {
    sandbox: SandboxInfo,
    bindings: Arc<ProcBindings>,
    /// `(path-template, METHOD) → ars_Api_Id` from the live registry.
    /// Used to bind `@uApiId`/`@sApiId` so the audit-log FK passes.
    api_ids: Arc<BTreeMap<(String, String), uuid::Uuid>>,
}

/// Server handle — used by the test runner to shut the server down at
/// the end of the run.
pub struct ServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
}

impl ServerHandle {
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Best effort — the join handle resolves when the server task ends.
        let join = self.join;
        tokio::spawn(async move {
            let _ = join.await;
        });
    }
}

/// Spawn the server on `127.0.0.1:port`. Returns a handle the caller
/// uses to shut it down.
pub async fn spawn(
    sandbox: SandboxInfo,
    bindings: ProcBindings,
    port: u16,
) -> Result<ServerHandle, String> {
    // Read the live registry once at startup so each request can
    // bind `@uApiId`/`@sApiId` to the matching row's uuid.
    let api_ids = fetch_registry_api_ids(&sandbox).await
        .unwrap_or_default();
    let state = AppState {
        sandbox,
        bindings: Arc::new(bindings),
        api_ids: Arc::new(api_ids),
    };
    let app = Router::new()
        .fallback(any(handle_request))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {}: {}", addr, e))?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    Ok(ServerHandle {
        shutdown_tx: Some(shutdown_tx),
        join,
    })
}

/// Read `(ars_Path, aml_Code) → ars_Api_Id` from the live registry.
/// Path normalisation (camelCase params, singular collections) is
/// applied so the keys match the OpenAPI spec's path templates that
/// the matcher uses.
async fn fetch_registry_api_ids(
    sandbox: &SandboxInfo,
) -> Result<BTreeMap<(String, String), uuid::Uuid>, String> {
    use crate::sql_verify::sandbox_config;
    use futures_util::TryStreamExt;
    use tiberius::{Client, QueryItem};
    use tokio_util::compat::TokioAsyncWriteCompatExt;

    let cfg = sandbox_config(sandbox.host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", sandbox.host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;

    let mut out = BTreeMap::new();
    // Cast the UUID to a string so tiberius hands us a varchar regardless
    // of the column's native `uniqueidentifier` type. Avoids a tiberius-
    // specific Uuid import (which is wrapped behind a feature flag).
    // Filter to enabled rows only. Legacy registry rows are disabled
    // (`ars_Enabled = 0`) rather than deleted to preserve FK references
    // from audits.ala_Api_Live_Audit. Without this filter, the BTreeMap
    // last-write-wins between active + disabled rows on the same
    // (path, method) pair — non-deterministic and binds the wrong api_id.
    let sql = "\
        SELECT CAST(ars_Api_Id AS VARCHAR(36)) AS api_id, ars_Path, aml_Code \
        FROM lookups.ars_Api_Rule_Settings \
        WHERE ars_Enabled = 1";
    let mut stream = client.simple_query(sql).await
        .map_err(|e| format!("registry query: {}", e))?;
    while let Some(item) = stream.try_next().await
        .map_err(|e| format!("stream: {}", e))?
    {
        if let QueryItem::Row(row) = item {
            let api_id_str: Option<&str> = row.try_get(0).ok().flatten();
            let path: Option<&str> = row.try_get(1).ok().flatten();
            let method: Option<&str> = row.try_get(2).ok().flatten();
            if let (Some(api_id), Some(p), Some(m)) = (api_id_str, path, method) {
                if let Ok(u) = uuid::Uuid::parse_str(api_id) {
                    out.insert((p.to_string(), m.to_uppercase()), u);
                }
            }
        }
    }
    Ok(out)
}

/// One-and-only handler. Looks up the proc binding for the request's
/// `(path-template, method)` and EXECs it.
async fn handle_request(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let raw_path = uri.path().to_string();
    let method_str = method.as_str().to_uppercase();

    // Match this concrete path against every templated path key in the
    // bindings map. Capture path-params on match.
    let (template_match, mut path_params) = match match_template(&raw_path, &state.bindings, &method_str) {
        Some(m) => m,
        None => {
            return (
                StatusCode::NOT_FOUND,
                format!("no proc binding for {} {}\n", method_str, raw_path),
            )
                .into_response();
        }
    };

    let proc_full = match state.bindings.get(&(template_match.clone(), method_str.clone())) {
        Some(p) => p.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                "internal: matched template but no binding\n",
            )
                .into_response();
        }
    };

    if proc_full == "ambiguous (see corrections.md)" || proc_full == "unbound (see corrections.md)" {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("not implementable: {}\n", proc_full),
        )
            .into_response();
    }

    // Build the envelope.
    let request_id = parse_or_new_uuid(&headers, "RequestId");
    let correlation_id = parse_or_new_uuid(&headers, "CorrelationId");
    let headers_json = serialise_headers(&headers);
    let body_json = if body.is_empty() {
        None
    } else {
        match std::str::from_utf8(&body) {
            Ok(s) => Some(s.to_string()),
            Err(_) => None,
        }
    };

    // Look up the registry uuid for this (path, method). The proc's
    // audit-log INSERT FKs to `ars_Api_Rule_Settings.ars_Api_Id`, so
    // we MUST pass a value that exists in that table — not nil-uuid,
    // not whatever the proc's hardcoded default happens to be.
    //
    // Spec-side templates are normalised (singular root + camelCase
    // params), but registry rows may carry the older raw form
    // (`/binsponsor/{id}`, `/cards/{card_id}`, etc.). Try a few key
    // variants — first the normalised template, then the registry's
    // typical raw variants — until one matches.
    let api_id = lookup_api_id(&state.api_ids, &template_match, &method_str);

    // When the matched template uses a generic `{id}` placeholder
    // (registry's raw shape), the proc parameter is named after the
    // entity (`@uBinSponsorId`, `@uCardId`, etc.). Add an entity-derived
    // alias so proc_invoke.rs's path-param lookup finds the captured
    // value. The first non-templated path segment is the entity root.
    if let Some(id_val) = path_params.get("id").cloned() {
        let entity = template_match
            .split('/')
            .filter(|s| !s.is_empty())
            .find(|s| !s.starts_with('{'))
            .unwrap_or("");
        if !entity.is_empty() {
            // `binsponsor` → `binSponsorId` (camelCase + Id suffix).
            // `programmanager` → `programManagerId`. Heuristic: lowercase
            // entity + "Id". The proc-invoke lookup is case-insensitive
            // and Hungarian-prefix-stripping, so this hits.
            let alias = format!("{}Id", entity);
            path_params.entry(alias).or_insert(id_val);
        }
    }

    // Cardholder-specific alias. The procs use `@uProfileId` for the
    // cardholder id (legacy: the client_profile table calls it
    // `cpf_Profile_Id`). When the URL template is
    // `/cardholder/{cardholderId}` we capture `cardholderId` but the
    // proc looks for `profileid`. Alias them both directions so either
    // proc-param name matches without ambiguity-resolution gymnastics.
    if template_match.starts_with("/cardholder/") || template_match.starts_with("/cardholders/") {
        if let Some(v) = path_params.get("cardholderId").cloned() {
            path_params.entry("profileId".to_string()).or_insert(v);
        } else if let Some(v) = path_params.get("profileId").cloned() {
            path_params.entry("cardholderId".to_string()).or_insert(v);
        }
    }

    // Fold query-string params into path_params so list-shape procs
    // (`@iPage`, `@iLimit`, `@sSortingParameters`) get bound. The proc-
    // invoke layer treats `path_params` as a generic "named value bag"
    // and matches by Hungarian-stripped key — `?page=1` ⇄ `@iPage`,
    // `?limit=10` ⇄ `@iLimit`, `?sort=created_at:desc` ⇄
    // `@sSortingParameters` (we also alias common ergonomic names).
    if let Some(q) = uri.query() {
        for pair in q.split('&').filter(|s| !s.is_empty()) {
            if let Some((k, v)) = pair.split_once('=') {
                let key = url_decode(k);
                let val = url_decode(v);
                path_params.entry(key.clone()).or_insert(val.clone());
                // Common alias: `sort` → `sortingParameters` (matches
                // `@sSortingParameters` after the Hungarian strip).
                if key.eq_ignore_ascii_case("sort") {
                    path_params
                        .entry("sortingParameters".to_string())
                        .or_insert(val);
                }
            }
        }
    }

    let envelope = InvocationEnvelope {
        request_id,
        correlation_id,
        headers_json,
        body_json,
        path_params,
        api_id,
    };

    match proc_invoke::execute(&state.sandbox, &proc_full, &envelope).await {
        Ok((Some(msg), code)) => {
            let status = match code {
                Some(c) if (200..300).contains(&c) => StatusCode::OK,
                Some(c) if c == 400 => StatusCode::BAD_REQUEST,
                Some(c) if c == 404 => StatusCode::NOT_FOUND,
                Some(c) if c == 401 => StatusCode::UNAUTHORIZED,
                Some(c) if c == 403 => StatusCode::FORBIDDEN,
                Some(c) if c == 500 => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::OK, // proc didn't return a code; treat as success
            };
            (
                status,
                [("content-type", "application/json")],
                msg,
            )
                .into_response()
        }
        Ok((None, _)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("proc {} returned no Response_Message\n", proc_full),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("proc {} failed: {}\n", proc_full, e),
        )
            .into_response(),
    }
}

/// Walk every (path_template, method) in bindings and return the
/// FIRST template that matches the concrete URL. Returns the template
/// + the captured `{param}` → value map.
fn match_template(
    raw_path: &str,
    bindings: &ProcBindings,
    method: &str,
) -> Option<(String, BTreeMap<String, String>)> {
    let req_segs: Vec<&str> = raw_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for ((template, m), _) in bindings {
        if !m.eq_ignore_ascii_case(method) {
            continue;
        }
        let tmpl_segs: Vec<&str> = template
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if tmpl_segs.len() != req_segs.len() {
            continue;
        }
        let mut params = BTreeMap::new();
        let mut all_match = true;
        for (t, r) in tmpl_segs.iter().zip(req_segs.iter()) {
            if t.starts_with('{') && t.ends_with('}') {
                let name = &t[1..t.len() - 1];
                params.insert(name.to_string(), (*r).to_string());
            } else if !t.eq_ignore_ascii_case(r) {
                all_match = false;
                break;
            }
        }
        if all_match {
            return Some((template.clone(), params));
        }
    }
    None
}

/// Look up an `ars_Api_Id` for a given (path-template, method) pair.
/// Tries the canonical key first, then falls back to common registry
/// variants — the registry can carry the raw seed form
/// (`/binsponsor/{id}`) while the spec emits the normalised form
/// (`/binsponsor/{binSponsorId}`). Returns the first hit.
fn lookup_api_id(
    api_ids: &BTreeMap<(String, String), uuid::Uuid>,
    template: &str,
    method: &str,
) -> Option<uuid::Uuid> {
    // 1. Canonical match.
    if let Some(u) = api_ids.get(&(template.to_string(), method.to_string())) {
        return Some(*u);
    }
    // 2. Replace any `{paramId}` with `{id}` (registry's raw shape).
    let with_bare_id = collapse_id_placeholders(template);
    if with_bare_id != template {
        if let Some(u) = api_ids.get(&(with_bare_id.clone(), method.to_string())) {
            return Some(*u);
        }
    }
    // 3. Same path text, different param-name casing — match by shape.
    let target_shape = path_shape(template);
    for ((p, m), u) in api_ids.iter() {
        if !m.eq_ignore_ascii_case(method) {
            continue;
        }
        if path_shape(p) == target_shape {
            return Some(*u);
        }
    }
    None
}

fn collapse_id_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            // Skip to matching `}`, replace whole token with `{id}`.
            while let Some(c2) = chars.next() {
                if c2 == '}' { break; }
            }
            out.push_str("{id}");
        } else {
            out.push(c);
        }
    }
    out
}

/// Reduce a path to its shape: collapse all `{xxx}` to `{x}` and
/// lowercase remaining segments. Used to match registry rows whose
/// param names disagree (`{id}` vs `{binSponsorId}`).
fn path_shape(p: &str) -> String {
    p.split('/')
        .map(|seg| {
            if seg.starts_with('{') && seg.ends_with('}') {
                "{x}".to_string()
            } else {
                seg.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn parse_or_new_uuid(headers: &HeaderMap, name: &str) -> uuid::Uuid {
    for (k, v) in headers {
        if k.as_str().eq_ignore_ascii_case(name) {
            if let Ok(s) = v.to_str() {
                if let Ok(u) = uuid::Uuid::parse_str(s) {
                    return u;
                }
            }
        }
    }
    uuid::Uuid::new_v4()
}

fn serialise_headers(headers: &HeaderMap) -> String {
    // Bruno header values are strings, but some procs read JSON-shaped
    // values via JSON_VALUE / JSON_QUERY (e.g.
    // `JSON_VALUE(@jRequestHeaders, '$.id[0]')` on Cardholder transition
    // procs expects an `id` header that is a JSON ARRAY of GUIDs).
    //
    // When a header value parses as a JSON value (array or object), we
    // serialize it as that value — so `id: ["BBBBBBBB-..."]` becomes
    // `"id":["BBBBBBBB-..."]` not `"id":"[\"BBBBBBBB-...\"]"`.
    use serde_json::{Map, Value};
    let mut map: Map<String, Value> = Map::new();
    for (k, v) in headers {
        if let Ok(s) = v.to_str() {
            let parsed = serde_json::from_str::<Value>(s).ok();
            let value = match parsed {
                Some(v @ Value::Array(_)) | Some(v @ Value::Object(_)) => v,
                _ => Value::String(s.to_string()),
            };
            map.insert(k.as_str().to_string(), value);
        }
    }
    serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string())
}

/// Minimal application/x-www-form-urlencoded decoder for query-string
/// values. Handles `+` → space and `%HH` → byte. Bad sequences are
/// passed through verbatim — harness queries are author-controlled, so
/// we don't need to be a hardened production decoder.
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push(((h << 4) | l) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

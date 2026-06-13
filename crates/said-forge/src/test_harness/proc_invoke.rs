//! Direct stored-procedure invocation. Given `(proc_full, body_json,
//! path_params, request_id, correlation_id)`, builds an `EXEC` call
//! that matches the proc's actual parameter list (introspected via
//! `sys.parameters`) and returns the proc's `Response_Message` JSON
//! envelope.

use crate::sql_verify::{sandbox_config, SandboxInfo};
use futures_util::TryStreamExt;
use std::collections::BTreeMap;
use tiberius::{Client, QueryItem};
use tokio_util::compat::TokioAsyncWriteCompatExt;

/// Map of `(path, method) → proc_full_name` parsed from the OpenAPI
/// spec's operation descriptions.
pub type ProcBindings = BTreeMap<(String, String), String>;

/// Parse `Implemented by `<schema>.<proc>`.` out of every operation's
/// description in the OpenAPI spec. Returns a map keyed by the
/// (path, method-uppercase) pair.
pub fn collect_bindings_from_spec(spec: &serde_yaml::Value) -> ProcBindings {
    let mut out = ProcBindings::new();
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
        for (method_key, op_val) in ops {
            let method = match method_key.as_str() {
                Some(s) => s.to_uppercase(),
                None => continue,
            };
            if !matches!(
                method.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
            ) {
                continue;
            }
            let desc = op_val
                .as_mapping()
                .and_then(|m| m.get(&serde_yaml::Value::String("description".into())))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Description shape: "Implemented by `<schema>.<proc>`. Endpoint..."
            if let Some(start) = desc.find("Implemented by `") {
                let rest = &desc[start + "Implemented by `".len()..];
                if let Some(end) = rest.find('`') {
                    let proc_full = &rest[..end];
                    if proc_full.contains('.') && !proc_full.contains(' ') {
                        out.insert((path_str.clone(), method.clone()), proc_full.to_string());
                    }
                }
            }
        }
    }
    out
}

/// One parameter of a target stored procedure (read from `sys.parameters`).
#[derive(Debug, Clone)]
pub struct ProcParamInfo {
    pub name: String,        // including leading `@`
    pub data_type: String,   // e.g. "uniqueidentifier", "nvarchar"
    pub max_length: i16,
    pub has_default: bool,
}

/// Bracket-quote a `schema.name` proc identifier so reserved keywords
/// (`identity`, `user`, `system`, etc.) work in EXEC statements. Each
/// half is escaped — `]` doubled per T-SQL convention.
pub fn bracket_proc_name(proc_full: &str) -> String {
    let escape = |s: &str| s.replace(']', "]]");
    match proc_full.split_once('.') {
        Some((schema, name)) => format!("[{}].[{}]", escape(schema), escape(name)),
        None => format!("[{}]", escape(proc_full)),
    }
}

/// Read `sys.parameters` for a proc. `proc_full` = `"schema.name"`.
pub async fn fetch_proc_params(
    sandbox: &SandboxInfo,
    proc_full: &str,
) -> Result<Vec<ProcParamInfo>, String> {
    let (schema, name) = match proc_full.split_once('.') {
        Some((s, n)) => (s.to_string(), n.to_string()),
        None => ("dbo".into(), proc_full.to_string()),
    };
    let cfg = sandbox_config(sandbox.host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", sandbox.host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;
    let sql = format!(
        "SELECT p.name, t.name, p.max_length, p.has_default_value \
         FROM sys.parameters p \
         INNER JOIN sys.objects o ON o.object_id = p.object_id \
         INNER JOIN sys.schemas s ON s.schema_id = o.schema_id \
         INNER JOIN sys.types t ON t.user_type_id = p.user_type_id \
         WHERE s.name = '{}' AND o.name = '{}' AND p.parameter_id > 0 \
         ORDER BY p.parameter_id;",
        schema.replace('\'', "''"),
        name.replace('\'', "''"),
    );
    let mut params = Vec::new();
    let mut stream = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("sys.parameters: {}", e))?;
    while let Some(item) = stream.try_next().await.map_err(|e| format!("stream: {}", e))? {
        if let QueryItem::Row(row) = item {
            let pname: Option<&str> = row.try_get(0).ok().flatten();
            let dtype: Option<&str> = row.try_get(1).ok().flatten();
            let max_len: Option<i16> = row.try_get(2).ok().flatten();
            let has_def: Option<bool> = row.try_get(3).ok().flatten();
            if let (Some(n), Some(t)) = (pname, dtype) {
                params.push(ProcParamInfo {
                    name: n.to_string(),
                    data_type: t.to_lowercase(),
                    max_length: max_len.unwrap_or(0),
                    has_default: has_def.unwrap_or(false),
                });
            }
        }
    }
    Ok(params)
}

/// The four standard envelope inputs every TXN proc takes (some
/// optional). Used to fill the procedure's parameter list when the
/// HTTP request doesn't carry these explicitly (UUIDs are minted per
/// call, headers stringified to JSON).
#[derive(Debug, Clone)]
pub struct InvocationEnvelope {
    pub request_id: uuid::Uuid,
    pub correlation_id: uuid::Uuid,
    pub headers_json: String,           // serialised request headers
    pub body_json: Option<String>,      // None when method has no body
    pub path_params: BTreeMap<String, String>, // e.g. {"binSponsorId": "..."}
    /// Registry-row UUID for the (path, method) being invoked. Looked
    /// up from `lookups.ars_Api_Rule_Settings` and passed as
    /// `@uApiId`/`@sApiId` so the proc's audit insert satisfies its
    /// FK to the registry table. None when no registry row matched.
    pub api_id: Option<uuid::Uuid>,
}

/// EXEC the proc and return the raw `Response_Message` JSON string
/// (already a JSON-encoded envelope `{requestId, correlationId, ...}`)
/// + the `Response_Code` integer (200 = success).
pub async fn execute(
    sandbox: &SandboxInfo,
    proc_full: &str,
    envelope: &InvocationEnvelope,
) -> Result<(Option<String>, Option<i32>), String> {
    let proc_params = fetch_proc_params(sandbox, proc_full).await?;

    let cfg = sandbox_config(sandbox.host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", sandbox.host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;

    // Build the EXEC: bind each proc parameter from the envelope by name.
    // We use named parameters so we tolerate any param order the proc declares.
    let mut bindings: Vec<String> = Vec::new();
    for p in &proc_params {
        let lc_name = p.name.trim_start_matches('@').to_lowercase();
        let value_sql = match lc_name.as_str() {
            // Standard envelope.
            "urequestid" => format!("'{}'", envelope.request_id),
            "ucorrelationid" => format!("'{}'", envelope.correlation_id),
            "jrequestheaders" => format!("N'{}'", envelope.headers_json.replace('\'', "''")),
            "jrequest" => match &envelope.body_json {
                Some(b) => format!("N'{}'", b.replace('\'', "''")),
                None => "NULL".to_string(),
            },
            "uapiid" | "sapiid" => {
                // Bind to the registry-row uuid for the (path, method)
                // being invoked. The audit-log table has a FK to
                // `ars_Api_Rule_Settings.ars_Api_Id`, so the value MUST
                // be a real registry row — nil-uuid would FK-fail.
                //
                // Two TXN proc shapes:
                //   - Create:     `@sApiId UNIQUEIDENTIFIER = '<hardcoded>'`
                //   - Get/Update: `@uApiId UNIQUEIDENTIFIER = ''`
                //
                // Caller passes the looked-up uuid via envelope.api_id;
                // when missing, fall back to the proc's own default by
                // skipping (works for Create's hardcoded default).
                match envelope.api_id {
                    Some(u) => format!("'{}'", u),
                    None => continue,
                }
            }
            // Path-param explosion — `@uBinSponsorId` ⇄ path-param `binSponsorId`.
            other => {
                // Heuristic: strip the FIRST Hungarian char (one of
                // u/s/b/j/i), then match against path-params
                // case-insensitively. Stripping more than one char
                // mangles `binsponsorid` → `nsponsorid`.
                let without_hungarian = match other.chars().next() {
                    Some(c) if matches!(c, 'u' | 's' | 'b' | 'j' | 'i') => &other[c.len_utf8()..],
                    _ => other,
                };
                // Some procs use `route` prefix for URL-bound path params
                // (e.g. `@uRouteProductId` for `/product/{productId}`).
                // Try stripping it after the Hungarian char.
                let without_route = without_hungarian
                    .strip_prefix("route")
                    .unwrap_or(without_hungarian);
                // Some Cardholder procs use `@uCardholderProfileId` (legacy
                // `cpf_Profile_Id` naming) but the URL path param is
                // `cardholderId`. Map `<entity>profileid` → `<entity>id`.
                let without_profile = without_route
                    .strip_suffix("profileid")
                    .map(|s| format!("{}id", s));
                let candidate_keys: Vec<String> = [
                    Some(other.to_string()),
                    Some(without_hungarian.to_string()),
                    Some(without_route.to_string()),
                    without_profile,
                ]
                .into_iter()
                .flatten()
                .collect();
                let mut value: Option<String> = None;
                for key in candidate_keys {
                    for (pk, pv) in &envelope.path_params {
                        if pk.eq_ignore_ascii_case(&key) {
                            value = Some(pv.clone());
                            break;
                        }
                    }
                    if value.is_some() {
                        break;
                    }
                }
                match value {
                    Some(v) if p.data_type.starts_with("uniqueidentifier") =>
                        format!("'{}'", v.replace('\'', "''")),
                    Some(v) if p.data_type.starts_with("nvarchar")
                        || p.data_type.starts_with("varchar")
                        || p.data_type.starts_with("char") =>
                        format!("N'{}'", v.replace('\'', "''")),
                    Some(v) if p.data_type.starts_with("int")
                        || p.data_type.starts_with("bigint")
                        || p.data_type.starts_with("smallint") =>
                        v,
                    Some(v) => format!("N'{}'", v.replace('\'', "''")),
                    None => {
                        if p.has_default {
                            // Skip — the proc's default kicks in.
                            continue;
                        }
                        // Fall back to NULL; proc may handle it or err.
                        "NULL".to_string()
                    }
                }
            }
        };
        bindings.push(format!("{} = {}", p.name, value_sql));
    }

    // Bracket-quote `schema.name` for SQL Server reserved keywords
    // (e.g. `identity`, `user`, `system`). A bare `EXEC identity.foo`
    // raises "Incorrect syntax near 'identity'"; `EXEC [identity].[foo]`
    // works for any schema/name regardless of reserved-word status.
    let proc_full_bracketed = bracket_proc_name(proc_full);

    let sql = format!(
        "SET NOCOUNT ON; EXEC {} {};",
        proc_full_bracketed,
        bindings.join(", "),
    );

    let mut response_message: Option<String> = None;
    let mut response_code: Option<i32> = None;
    let mut stream = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("EXEC {} failed: {}", proc_full, e))?;
    while let Some(item) = stream.try_next().await.map_err(|e| format!("stream: {}", e))? {
        if let QueryItem::Row(row) = item {
            // The first column is `Response_Message` per TXN convention.
            // Fall back to looking at columns by best-effort if proc
            // returns a different shape.
            for i in 0..row.len() {
                if response_message.is_none() {
                    if let Ok(Some(v)) = row.try_get::<&str, _>(i) {
                        if v.trim_start().starts_with('{') {
                            response_message = Some(v.to_string());
                            continue;
                        }
                    }
                }
                if response_code.is_none() {
                    if let Ok(Some(v)) = row.try_get::<i32, _>(i) {
                        response_code = Some(v);
                    }
                }
            }
        }
    }
    Ok((response_message, response_code))
}

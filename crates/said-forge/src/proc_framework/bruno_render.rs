//! Bruno fixture renderer — emits per-bundle `.bru` files from the
//! bundle declaration + the rendered SQL proc body.
//!
//! For each endpoint:
//!   - One positive fixture per `<shape>-positive.bru` template
//!   - One negative fixture per `prc_Code = <N>` reference found inside
//!     a `@said-managed: Ignore` slot in the rendered SQL proc
//!
//! Plus one `folder.bru` per bundle (collection-level metadata).
//!
//! Templates live at `profiles/<P>/bruno/_templates/`. Defaults
//! (localBaseURL, apim key, api-version) come from `profile.toml`
//! `[bruno_defaults]`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::manifest::{BrunoDefaults, Bundle, EndpointRow, Profile};

/// Frozen literal — Bruno's built-in `{{$guid}}` function ref. Kept as a
/// constant rather than a template var so callers don't accidentally
/// pass a literal GUID and bake it into every fixture.
pub const GUID_TOKEN: &str = "{{$guid}}";
pub const LOCAL_BASE_URL_TOKEN: &str = "{{localBaseURL}}";

/// Codes that mean "framework infrastructure rejected the request" and
/// don't need a per-bundle negative fixture (every bundle would otherwise
/// emit the same fixture). Add to this list when a new shared code becomes
/// load-bearing across every shape.
const SHARED_PRC_CODES: &[i64] = &[
    -1, -10, -11, -12, -13, -14, -15, -16, -17, -18, -19, -20, 0,
    1000, 1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009, 1010,
];

/// Outcome of rendering a single endpoint — paths the renderer wrote.
#[derive(Debug, Clone)]
pub struct BrunoEmission {
    pub endpoint_id: String,
    pub fixture_files: Vec<PathBuf>,
}

/// Render every endpoint's Bruno fixtures for one bundle.
///
/// `bundle` is the bundle declaration (read via `manifest::load_bundle`).
/// `profile` holds the BrunoDefaults + path templates.
/// `workspace_root` is the dir to join all relative paths against.
/// `proc_root_template` is the source for rendered SQL procs (used to
/// scan for negative prc_Codes); typically `profile.generated_paths.sql_proc_root`.
/// `out_bruno_root` is the directory where this bundle's .bru files land.
pub fn render_bundle(
    bundle: &Bundle,
    profile: &Profile,
    workspace_root: &Path,
    proc_root_template: &str,
    out_bruno_root: &Path,
) -> Result<Vec<BrunoEmission>, String> {
    let templates = TemplateSet::load(profile_templates_root(profile, workspace_root)?)?;
    let defaults = &profile.bruno_defaults;

    std::fs::create_dir_all(out_bruno_root)
        .map_err(|e| format!("mkdir {}: {}", out_bruno_root.display(), e))?;

    // ---- folder.bru (collection-level, one per bundle) --------------------
    write_folder_bru(&templates, &bundle.bundle, out_bruno_root)?;

    // ---- per-endpoint fixtures --------------------------------------------
    let mut out = Vec::<BrunoEmission>::new();
    let mut next_pos_seq: u32 = 1;
    let mut next_neg_seq: u32 = 101;

    for ep in &bundle.endpoints {
        let mut files: Vec<PathBuf> = Vec::new();

        // Resolve the rendered proc body once. Used both for negative
        // prc_Code scanning AND for derive_request_body (command shape).
        let proc_path = resolve_proc_path(workspace_root, proc_root_template, ep);
        let proc_body = if proc_path.exists() {
            std::fs::read_to_string(&proc_path).ok()
        } else {
            None
        };

        // Positive
        let pos_path = write_positive_fixture(
            &templates,
            ep,
            defaults,
            next_pos_seq,
            out_bruno_root,
            proc_body.as_deref(),
        )?;
        if let Some(p) = pos_path {
            files.push(p);
        }
        next_pos_seq += 1;

        // Negatives — scan the rendered proc for prc_Codes inside Ignore slots.
        let neg_codes = if proc_path.exists() {
            scan_ignore_slot_prc_codes(&proc_path)?
        } else {
            Vec::new()
        };

        for (code, desc) in neg_codes {
            let neg_path = write_negative_fixture(
                &templates,
                ep,
                defaults,
                next_neg_seq,
                code,
                &desc,
                out_bruno_root,
                proc_body.as_deref(),
            )?;
            if let Some(p) = neg_path {
                files.push(p);
            }
            next_neg_seq += 1;
        }

        out.push(BrunoEmission {
            endpoint_id: ep.id.clone(),
            fixture_files: files,
        });
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Template loading
// ---------------------------------------------------------------------------

struct TemplateSet {
    folder: String,
    query_list_positive: Option<String>,
    query_list_negative: Option<String>,
    /// New in 2026-05 — Bruno fixtures for command-shape endpoints
    /// (POST/PUT/DELETE with a JSON body). The `.bru` file embeds the
    /// HTTP verb token and a `body:json { ... }` block; the body
    /// payload is derived from the proc's `OPENJSON(...) WITH (...)`
    /// columns via the sql-parser sidecar.
    command_positive: Option<String>,
    /// Negative-path command fixture: same shape as command_positive,
    /// plus `expected_status` / `expected_prc_code` in `vars:pre-request`.
    command_negative: Option<String>,
    /// Single-record GET fixture. Path includes a `{routeParam}`
    /// placeholder; the var is set via `vars:pre-request` so Bruno
    /// substitutes it at request time. Negative variant is deferred
    /// (4xx-not-found is harness-handled later).
    query_by_id_positive: Option<String>,
}

impl TemplateSet {
    fn load(root: PathBuf) -> Result<Self, String> {
        let read = |name: &str| -> Result<String, String> {
            let p = root.join(name);
            std::fs::read_to_string(&p)
                .map_err(|e| format!("read template {}: {}", p.display(), e))
        };
        Ok(Self {
            folder: read("folder.bru")?,
            query_list_positive: read("query-list-positive.bru").ok(),
            query_list_negative: read("query-list-negative.bru").ok(),
            command_positive: read("command-positive.bru").ok(),
            command_negative: read("command-negative.bru").ok(),
            query_by_id_positive: read("query-by-id-positive.bru").ok(),
        })
    }

    fn positive_for(&self, shape: &str) -> Option<&str> {
        match shape {
            "query-list" => self.query_list_positive.as_deref(),
            "command" => self.command_positive.as_deref(),
            "query-by-id" => self.query_by_id_positive.as_deref(),
            _ => None,
        }
    }

    fn negative_for(&self, shape: &str) -> Option<&str> {
        match shape {
            "query-list" => self.query_list_negative.as_deref(),
            "command" => self.command_negative.as_deref(),
            // query-by-id negative (4xx-not-found) is a harness-handled case
            // — we don't emit a per-fixture negative for it today.
            _ => None,
        }
    }
}

fn profile_templates_root(profile: &Profile, workspace_root: &Path) -> Result<PathBuf, String> {
    // Templates live inside the framework, NOT under workspace. The caller
    // passes us a workspace_root that's the **framework profile root**
    // when calling for templates. Actually the cleanest move is to pass
    // the framework root directly. For now, derive from a known anchor:
    // the proc-framework is structured as `<framework>/profiles/<name>/...`
    // so the templates root is `<framework>/profiles/<name>/bruno/_templates`.
    //
    // But this function only gets the workspace_root + the profile struct
    // (which doesn't carry its own framework path). We solve this by having
    // the caller resolve and pass the bruno templates path explicitly.
    //
    // To keep the API simple right now, we accept that the caller will
    // build the templates root before calling render_bundle, and instead
    // expose a separate helper. Refactor: caller passes templates_root in.
    let _ = (profile, workspace_root);
    Err("templates_root must be passed by caller — call render_bundle_with_templates instead".into())
}

/// Same as `render_bundle` but the caller passes the templates directory
/// directly. Use this until profile carries its own framework_root.
pub fn render_bundle_with_templates(
    bundle: &Bundle,
    profile: &Profile,
    templates_root: &Path,
    workspace_root: &Path,
    proc_root_template: &str,
    out_bruno_root: &Path,
) -> Result<Vec<BrunoEmission>, String> {
    let templates = TemplateSet::load(templates_root.to_path_buf())?;
    let defaults = &profile.bruno_defaults;

    std::fs::create_dir_all(out_bruno_root)
        .map_err(|e| format!("mkdir {}: {}", out_bruno_root.display(), e))?;

    write_folder_bru(&templates, &bundle.bundle, out_bruno_root)?;

    let mut out = Vec::<BrunoEmission>::new();
    let mut next_pos_seq: u32 = 1;
    let mut next_neg_seq: u32 = 101;

    for ep in &bundle.endpoints {
        let mut files: Vec<PathBuf> = Vec::new();

        // Resolve the rendered proc body once — used for both body
        // derivation (command shape) and negative-fixture prc_Code
        // discovery. See the matching block in render_bundle().
        let proc_path = resolve_proc_path(workspace_root, proc_root_template, ep);
        let proc_body = if proc_path.exists() {
            std::fs::read_to_string(&proc_path).ok()
        } else {
            None
        };

        let pos_path = write_positive_fixture(
            &templates,
            ep,
            defaults,
            next_pos_seq,
            out_bruno_root,
            proc_body.as_deref(),
        )?;
        if let Some(p) = pos_path {
            files.push(p);
        }
        next_pos_seq += 1;

        let neg_codes = if proc_path.exists() {
            scan_ignore_slot_prc_codes(&proc_path)?
        } else {
            Vec::new()
        };

        for (code, desc) in neg_codes {
            let neg_path = write_negative_fixture(
                &templates,
                ep,
                defaults,
                next_neg_seq,
                code,
                &desc,
                out_bruno_root,
                proc_body.as_deref(),
            )?;
            if let Some(p) = neg_path {
                files.push(p);
            }
            next_neg_seq += 1;
        }

        out.push(BrunoEmission {
            endpoint_id: ep.id.clone(),
            fixture_files: files,
        });
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Folder
// ---------------------------------------------------------------------------

fn write_folder_bru(templates: &TemplateSet, bundle: &str, out_root: &Path) -> Result<(), String> {
    let text = templates.folder.replace("{{bundle}}", bundle);
    let target = out_root.join("folder.bru");
    std::fs::write(&target, text.as_bytes())
        .map_err(|e| format!("write {}: {}", target.display(), e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Positive
// ---------------------------------------------------------------------------

fn write_positive_fixture(
    templates: &TemplateSet,
    ep: &EndpointRow,
    defaults: &BrunoDefaults,
    seq: u32,
    out_root: &Path,
    proc_body: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let Some(tpl) = templates.positive_for(&ep.shape) else {
        return Ok(None);
    };

    let fixture_name = bare_stem(&ep.id);
    let (params_block, query_string) = build_query_params(&ep.shape);
    let http_method_lc = http_method_lc(ep);
    let request_body = if ep.shape == "command" {
        derive_request_body(proc_body, &http_method_lc)
    } else {
        String::new()
    };
    let route_param_vars = build_route_param_vars(ep);

    let text = render_template(
        tpl,
        &ep.path,
        &fixture_name,
        seq,
        &params_block,
        &query_string,
        defaults,
        None,
        None,
        &request_body,
        &http_method_lc,
        &route_param_vars,
    );

    let target = out_root.join(format!("{}.bru", fixture_name));
    std::fs::write(&target, text.as_bytes())
        .map_err(|e| format!("write {}: {}", target.display(), e))?;
    Ok(Some(target))
}

// ---------------------------------------------------------------------------
// Negative
// ---------------------------------------------------------------------------

fn write_negative_fixture(
    templates: &TemplateSet,
    ep: &EndpointRow,
    defaults: &BrunoDefaults,
    seq: u32,
    prc_code: i64,
    prc_desc: &str,
    out_root: &Path,
    proc_body: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let Some(tpl) = templates.negative_for(&ep.shape) else {
        return Ok(None);
    };

    let suffix = slugify_for_filename(prc_desc);
    let fixture_name = format!("neg-{}-{}", bare_stem(&ep.id), suffix);

    // Negatives for query-list use boundary-violating query params so the
    // proc's slot=5_sort_parse branches return the right prc_Code.
    // Negative-path command fixtures send a structurally-valid body and
    // rely on the proc's business-rule rejection (e.g. cardIds empty for
    // 3DSecure prc_Code 80100) to return the expected error.
    let (params_block, query_string) = negative_query_for(prc_code, &ep.shape);
    let http_method_lc = http_method_lc(ep);
    let request_body = if ep.shape == "command" {
        // For now: the negative fixture sends an empty-array body, which
        // is the common-shape trigger for command negatives across the
        // corpus. Per-prc-code body customisation can come later if a
        // specific bundle's negatives need different shapes.
        negative_request_body_for(prc_code, proc_body, &http_method_lc)
    } else {
        String::new()
    };
    let route_param_vars = build_route_param_vars(ep);

    let text = render_template(
        tpl,
        &ep.path,
        &fixture_name,
        seq,
        &params_block,
        &query_string,
        defaults,
        Some(400),
        Some(prc_code),
        &request_body,
        &http_method_lc,
        &route_param_vars,
    );

    let target = out_root.join(format!("{}.bru", fixture_name));
    std::fs::write(&target, text.as_bytes())
        .map_err(|e| format!("write {}: {}", target.display(), e))?;
    Ok(Some(target))
}

// ---------------------------------------------------------------------------
// Variable substitution
// ---------------------------------------------------------------------------

/// Substitute all `{{...}}` placeholders in a Bruno template. The
/// argument count has grown over time as new shapes have come online —
/// query-list (page/limit/sort) needed `params_query_block` + `query_string`,
/// command shape added `request_body` + `http_method_lc`, query-by-id added
/// `route_param_vars` so the route param's value lives in `vars:pre-request`.
///
/// Unused placeholders pass through as-is — that's intentional, so a
/// template can choose which subset it wants without forcing every call
/// site to fabricate empty strings for unrelated fields.
#[allow(clippy::too_many_arguments)]
fn render_template(
    tpl: &str,
    path: &str,
    fixture_name: &str,
    seq: u32,
    params_query_block: &str,
    query_string: &str,
    defaults: &BrunoDefaults,
    expected_status: Option<u16>,
    expected_prc_code: Option<i64>,
    request_body: &str,
    http_method_lc: &str,
    route_param_vars: &str,
) -> String {
    tpl.replace("{{fixture_name}}", fixture_name)
        .replace("{{seq}}", &seq.to_string())
        .replace("{{path}}", path)
        .replace("{{query_string}}", query_string)
        .replace("{{params_query_block}}", params_query_block)
        .replace("{{request_body}}", request_body)
        .replace("{{http_method_lc}}", http_method_lc)
        .replace("{{route_param_vars}}", route_param_vars)
        .replace("{{guid_token}}", GUID_TOKEN)
        .replace("{{local_base_url_token}}", LOCAL_BASE_URL_TOKEN)
        .replace(
            "{{local_base_url}}",
            defaults.local_base_url.as_deref().unwrap_or(""),
        )
        .replace(
            "{{api_version}}",
            defaults.api_version.as_deref().unwrap_or(""),
        )
        .replace(
            "{{apim_subscription_key}}",
            defaults.apim_subscription_key.as_deref().unwrap_or(""),
        )
        .replace(
            "{{expected_status}}",
            &expected_status
                .map(|s| s.to_string())
                .unwrap_or_default(),
        )
        .replace(
            "{{expected_prc_code}}",
            &expected_prc_code
                .map(|c| c.to_string())
                .unwrap_or_default(),
        )
}

// ---------------------------------------------------------------------------
// Shape-specific query parameter generation
// ---------------------------------------------------------------------------

fn build_query_params(shape: &str) -> (String, String) {
    match shape {
        "query-list" => (
            "  page: 1\n  limit: 10\n  sort: created_at:desc".to_string(),
            "?page=1&limit=10&sort=created_at:desc".to_string(),
        ),
        // command / query-by-id added in later passes
        _ => (String::new(), String::new()),
    }
}

/// For a negative prc_Code, pick query params that trigger the proc's
/// matching reject branch. The mapping comes from the canonical shape
/// validation rules (5_sort_parse slot for query-list).
fn negative_query_for(prc_code: i64, shape: &str) -> (String, String) {
    if shape != "query-list" {
        return (String::new(), String::new());
    }
    match prc_code {
        60301 => (
            "  page: 0\n  limit: 10".to_string(),
            "?page=0&limit=10".to_string(),
        ),
        60302 => (
            "  page: 1\n  limit: 0".to_string(),
            "?page=1&limit=0".to_string(),
        ),
        60303 => (
            "  page: 1\n  limit: 999".to_string(),
            "?page=1&limit=999".to_string(),
        ),
        _ => (
            // Fallback — just send the default; harness verifies prc_code separately.
            "  page: 1\n  limit: 10".to_string(),
            "?page=1&limit=10".to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Helpers: id-stem extraction, slugify, proc path resolve, prc_Code scan
// ---------------------------------------------------------------------------

fn bare_stem(id: &str) -> String {
    // "Alert.GetAllAlerts" -> "GetAllAlerts"
    id.split_once('.').map(|(_, s)| s.to_string()).unwrap_or_else(|| id.to_string())
}

fn slugify_for_filename(desc: &str) -> String {
    // "[60301] - Page cannot be less than 1" -> "pageCannotBeLessThan1"
    let mut chars: Vec<char> = Vec::new();
    let mut upper_next = false;
    let mut started = false;
    for c in desc.chars().skip_while(|c| !c.is_alphabetic()) {
        if c.is_alphanumeric() {
            if upper_next && started {
                chars.extend(c.to_uppercase());
                upper_next = false;
            } else {
                chars.extend(c.to_lowercase());
            }
            started = true;
        } else {
            upper_next = true;
        }
    }
    let mut s: String = chars.into_iter().collect();
    if s.is_empty() {
        s = "negative".into();
    }
    // Cap length
    if s.len() > 40 {
        s.truncate(40);
    }
    s
}

/// Lowercase HTTP verb for the `{{http_method_lc}}` placeholder. Bruno's
/// per-request block keyword is the lowercase verb (`post {`, `put {`,
/// `delete {`). When the endpoint row has no explicit method, falls
/// back to `post` (commands default to POST per the convention).
fn http_method_lc(ep: &EndpointRow) -> String {
    ep.method
        .as_deref()
        .unwrap_or("POST")
        .to_lowercase()
}

/// Build the `vars:pre-request` route-param assignments for query-by-id
/// fixtures. Returns multi-line indented text — one `<paramName>: <value>`
/// per route param.
///
/// The value is a fixed seed GUID (`00000000-0000-0000-0000-000000000001`)
/// matching the existing 3DSecure/Account legacy Bruno conventions. The
/// harness seeds the sandbox with this id before running the fixture; the
/// human can edit the file post-render if a different seed is needed.
fn build_route_param_vars(ep: &EndpointRow) -> String {
    if ep.route_params.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for rp in &ep.route_params {
        // Strip the leading `@` and any case prefix (`u`, `i`, `s`, `j`)
        // — convention is `@uAuthenticationId` → `authenticationId`.
        let var_name = bruno_var_name(&rp.name);
        let seed = if rp.ty.to_uppercase().contains("UNIQUEIDENTIFIER") {
            "00000000-0000-0000-0000-000000000001"
        } else {
            "0"
        };
        out.push_str(&format!("  {}: {}\n", var_name, seed));
    }
    // Trim trailing newline so the template's closing `}` lines up.
    out.trim_end().to_string()
}

/// Convert a SQL param name like `@uAuthenticationId` to its Bruno var
/// equivalent `authenticationId` (matches the legacy convention of
/// stripping the `@` + Hungarian type prefix `u`/`i`/`s`/`j` and
/// lower-casing the first letter).
fn bruno_var_name(sql_name: &str) -> String {
    let trimmed = sql_name.trim_start_matches('@');
    let mut chars = trimmed.chars();
    let first = chars.next();
    let rest: String = chars.collect();
    match first {
        Some(c) if matches!(c, 'u' | 'i' | 's' | 'j' | 'd' | 'b') && rest.starts_with(|x: char| x.is_ascii_uppercase()) => {
            let mut r = rest.chars();
            let first_rest = r.next().unwrap();
            let tail: String = r.collect();
            format!("{}{}", first_rest.to_ascii_lowercase(), tail)
        }
        Some(c) => {
            let mut s = c.to_string();
            s.push_str(&rest);
            s
        }
        None => String::new(),
    }
}

/// Derive a minimal valid JSON body for a command-shape endpoint by
/// asking the sql-parser sidecar what columns the proc reads from
/// `OPENJSON(@jRequest)`. Returns a 2-space-indented JSON object
/// (without enclosing braces — the template provides those).
///
/// When the proc body isn't available (e.g. running before the proc
/// has been rendered) or the parser is offline / returns no
/// `openjsonWith[0]`, we emit a single TODO placeholder so the
/// fixture is still syntactically valid but obvious about being a
/// stub.
///
/// For POST (Create) endpoints, the `$.id` path is filtered out — the
/// server generates the entity id (`NEWID()` in the proc), and a
/// hardcoded body id leads to test cross-contamination (fixture re-runs
/// hit duplicate-key errors instead of clean inserts). For PUT/PATCH
/// (Update), `$.id` is kept because the server enforces
/// `body.id == route.{entity}Id` and a missing body id is a 4xx.
fn derive_request_body(proc_body: Option<&str>, http_method_lc: &str) -> String {
    let Some(body) = proc_body else {
        return "  \"_TODO\": \"proc body unavailable when fixture rendered\"".into();
    };
    match parser_openjson_columns(body) {
        Some(cols) if !cols.is_empty() => {
            let filtered = filter_body_id_for_post(&cols, http_method_lc);
            render_body_columns(&filtered)
        }
        _ => "  \"_TODO\": \"add request fields here\"".into(),
    }
}

/// On POST (Create) endpoints, drop the `$.id` column from the body.
/// The server mints a fresh id with `NEWID()`. Other methods keep `id`
/// because the server validates body.id matches the route param.
fn filter_body_id_for_post(
    cols: &[OpenJsonColumn],
    http_method_lc: &str,
) -> Vec<OpenJsonColumn> {
    if http_method_lc != "post" {
        return cols.to_vec();
    }
    cols.iter()
        .filter(|c| {
            // Drop the top-level `$.id` reader. Keep nested `$.xxx.id`
            // (e.g. transition references) and non-id paths.
            !matches!(c.json_path.as_deref(), Some("$.id"))
        })
        .cloned()
        .collect()
}

/// Best-effort negative-path body. For the prc_Codes the corpus
/// commonly uses to test "empty array / missing required field"
/// (e.g. 3DSecure's 80100 — empty cardIds), emit an empty payload
/// shape that hits that branch. Otherwise reuse the positive body —
/// the harness verifies prc_Code separately so a structurally-valid
/// body is fine.
fn negative_request_body_for(
    prc_code: i64,
    proc_body: Option<&str>,
    http_method_lc: &str,
) -> String {
    // For known "missing array" prc_Codes (the 3DSecure 80100 family),
    // emit the array field explicitly as `[]` so the proc reads the
    // body cleanly, then short-circuits on the count = 0 branch
    // returning the typed prc_Code. Falls back to an empty object for
    // unknown codes — that triggers the proc's OPENJSON "missing
    // required field" path and returns its generic typed code.
    let raw_cols = proc_body.and_then(parser_openjson_columns).unwrap_or_default();
    let cols = filter_body_id_for_post(&raw_cols, http_method_lc);
    if cols.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, c) in cols.iter().enumerate() {
        let key = body_key_for(c);
        // For the empty-array-trigger prc_Codes, set arrays to `[]`.
        // For all other fields, send the positive sample — the proc's
        // own validation rejects the request on the missing-array path.
        let value = if c.as_json {
            "[]".to_string()
        } else {
            sample_value_for(&c.sql_type, c.as_json)
        };
        let comma = if i + 1 < cols.len() { "," } else { "" };
        out.push_str(&format!("    \"{}\": {}{}\n", key, value, comma));
    }
    let _ = prc_code;
    out.trim_end().to_string()
}

/// Format a column list as a 2-space-indented JSON body fragment.
/// Each column gets a sample value based on its SQL type — fixed seed
/// for UNIQUEIDENTIFIER, `"sample"` for varchar, `1` for int, etc.
///
/// The body key is derived from the column's `jsonPath` (e.g.
/// `$.cardIds` → `cardIds`) when present — that's the actual JSON
/// API contract. Falls back to the SQL column alias name when the
/// proc didn't specify a path (in which case the alias IS the path).
fn render_body_columns(cols: &[OpenJsonColumn]) -> String {
    // Build a nested-object representation so dotted paths like
    // `$.billingAddress.line1` produce
    //   "billingAddress": { "line1": "sample", ... }
    // rather than the invalid flat shape
    //   "billingAddress.line1": "sample"
    // The Bruno fixture is JSON sent to the API as a request body; SQL
    // Server's OPENJSON treats `"billingAddress.line1"` as a single
    // top-level key (no nesting), which doesn't match the proc's
    // `$.billingAddress.line1` JSON-path reads, so the request is
    // rejected with prc_Code 1000 ("Required field not in JSON object").
    //
    // The 4-space indent keeps the rendered .bru aligned with the
    // legacy Bruno-collection convention:
    //   body:json {
    //     {
    //       "key": value
    //     }
    //   }
    use std::collections::BTreeMap;

    enum Node {
        Leaf(String),                // already-rendered JSON literal
        Tree(BTreeMap<String, Node>), // nested object
    }

    let mut root: BTreeMap<String, Node> = BTreeMap::new();
    for c in cols {
        let key = body_key_for(c);
        let segments: Vec<&str> = key.split('.').collect();
        let sample = sample_value_for(&c.sql_type, c.as_json);
        insert_path(&mut root, &segments, sample);
    }

    fn insert_path(map: &mut BTreeMap<String, Node>, segments: &[&str], value: String) {
        if segments.is_empty() {
            return;
        }
        if segments.len() == 1 {
            map.insert(segments[0].to_string(), Node::Leaf(value));
            return;
        }
        let head = segments[0].to_string();
        match map.entry(head).or_insert_with(|| Node::Tree(BTreeMap::new())) {
            Node::Tree(sub) => insert_path(sub, &segments[1..], value),
            // Collision: a leaf was already set for this name. Replace
            // with a tree containing the prior leaf as a placeholder
            // and the new value. (No-op — keep leaf, drop new value.
            // This is a deterministic tiebreak; collisions in practice
            // would indicate a proc-shape ambiguity worth investigating.)
            Node::Leaf(_) => {}
        }
    }

    fn render_node(out: &mut String, node: &Node, indent: usize) {
        match node {
            Node::Leaf(v) => out.push_str(v),
            Node::Tree(m) => {
                out.push('{');
                let entries: Vec<(&String, &Node)> = m.iter().collect();
                out.push('\n');
                for (i, (k, v)) in entries.iter().enumerate() {
                    out.push_str(&" ".repeat(indent + 2));
                    out.push_str(&format!("\"{}\": ", k));
                    render_node(out, v, indent + 2);
                    if i + 1 < entries.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&" ".repeat(indent));
                out.push('}');
            }
        }
    }

    let mut out = String::new();
    let entries: Vec<(&String, &Node)> = root.iter().collect();
    for (i, (k, v)) in entries.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&format!("\"{}\": ", k));
        render_node(&mut out, v, 4);
        if i + 1 < entries.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Extract the JSON body key from an OPENJSON column. `$.cardIds` →
/// `cardIds`; `$.cardholder.firstName` → `cardholder.firstName` (this
/// is a leaf — the body builder uses the path verbatim so nested keys
/// produce a dotted string the caller can interpret). When no
/// jsonPath is set, returns the column alias verbatim.
fn body_key_for(c: &OpenJsonColumn) -> String {
    match c.json_path.as_deref() {
        Some(path) if path.starts_with("$.") => path[2..].to_string(),
        Some(path) => path.trim_start_matches('$').trim_start_matches('.').to_string(),
        None => c.name.clone(),
    }
}

/// Picks a JSON-literal sample for a given OPENJSON column SQL type.
/// The values are deliberately deterministic so two renders produce
/// byte-identical Bruno fixtures.
///
/// For `AS JSON` columns we emit a singleton GUID array — that's the
/// shape every command-shape array-payload field uses in the corpus
/// today (cardIds, accountIds, customerIds, ...). The positive
/// fixture's purpose is to deliver a request the proc accepts; an
/// empty array fails immediately on most procs' length checks. If a
/// future bundle's array isn't GUIDs, the human can edit the fixture
/// (Bruno fixtures are human-friendly).
fn sample_value_for(sql_type: &str, as_json: bool) -> String {
    if as_json {
        return "[\"00000000-0000-0000-0000-000000000001\"]".into();
    }
    let upper = sql_type.to_uppercase();
    let upper = upper.trim();
    if upper.contains("UNIQUEIDENTIFIER") {
        "\"00000000-0000-0000-0000-000000000001\"".into()
    } else if upper.starts_with("INT") || upper.contains(" INT") || upper.starts_with("BIGINT")
        || upper.starts_with("SMALLINT") || upper.starts_with("TINYINT")
    {
        "1".into()
    } else if upper.starts_with("BIT") {
        "true".into()
    } else if upper.starts_with("DECIMAL") || upper.starts_with("NUMERIC")
        || upper.starts_with("FLOAT") || upper.starts_with("MONEY")
        || upper.starts_with("REAL")
    {
        "1.0".into()
    } else if upper.contains("DATE") || upper.contains("TIME") {
        "\"2026-01-01T00:00:00Z\"".into()
    } else {
        // NVARCHAR / VARCHAR / CHAR / NCHAR / NTEXT / TEXT / etc.
        "\"sample\"".into()
    }
}

/// Minimal wire model for one OPENJSON column — we deserialise the
/// fields the body builder uses (name + sqlType + jsonPath + asJson).
/// The full schema is in
/// `dtcard/.forge-sandbox/services/sql-parser/Models.cs`.
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct OpenJsonColumn {
    name: String,
    sql_type: String,
    /// The `$.fieldName` JSON path the proc reads. We use this to
    /// determine the BODY key (camelCase per the JSON convention),
    /// because the SQL column alias is typically the snake_case
    /// version and the actual API contract uses the path's final
    /// segment.
    #[serde(default)]
    json_path: Option<String>,
    #[serde(default)]
    as_json: bool,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OpenJsonInfo {
    #[serde(default)]
    columns: Vec<OpenJsonColumn>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct BrunoParseResponse {
    #[serde(default)]
    openjson_with: Vec<OpenJsonInfo>,
}

/// POST the proc body to the parser sidecar and return the first
/// `openjsonWith[0].columns` list. Returns None when the sidecar is
/// unreachable, the parse fails, or no OPENJSON clause was found —
/// in any of those cases the body builder falls back to a TODO stub.
fn parser_openjson_columns(proc_body: &str) -> Option<Vec<OpenJsonColumn>> {
    use std::sync::OnceLock;
    use std::time::Duration;
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(3000))
            .build()
            .expect("reqwest blocking client for bruno_render")
    });
    let url = std::env::var("SAID_FORGE_SQL_PARSER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:5002".to_string());
    let resp = client
        .post(format!("{}/parse", url))
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(proc_body.to_string())
        .send()
        .ok()?;
    let parsed: BrunoParseResponse = resp.json().ok()?;

    // Walk EVERY OPENJSON block and union their columns by jsonPath.
    // Older procs (BinSponsor, Business) parse the body across multiple
    // OPENJSON statements piecewise — the first block is often empty
    // (just a key-iteration to assert presence) and the real shape only
    // appears in later blocks. Dedup by jsonPath so nested fields like
    // `$.settlement.host` are only registered once even if parsed in
    // two places.
    let mut seen_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out: Vec<OpenJsonColumn> = Vec::new();
    for block in parsed.openjson_with {
        for col in block.columns {
            let path = col.json_path.clone().unwrap_or_default();
            if path.is_empty() {
                continue;
            }
            if seen_paths.insert(path) {
                out.push(col);
            }
        }
    }

    // JSON_VALUE fallback: when OPENJSON discovers nothing, scan the
    // proc body for `JSON_VALUE(@jRequest, '$.field')` references and
    // emit each path as a string-typed synthetic column. Older procs
    // (Business.CreateBusinessTransition) parse the body via JSON_VALUE
    // calls rather than OPENJSON WITH, and the body-deriver previously
    // emitted `_TODO` for those.
    if out.is_empty() {
        for path in extract_json_value_paths(proc_body) {
            if seen_paths.insert(path.clone()) {
                out.push(OpenJsonColumn {
                    name: path
                        .trim_start_matches("$.")
                        .replace('.', "_"),
                    sql_type: "NVARCHAR(MAX)".to_string(),
                    json_path: Some(path),
                    as_json: false,
                });
            }
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Scan the proc body for `JSON_VALUE(@jRequest, '$.path')` references.
/// Returns the distinct paths in first-seen order. Same logic as
/// `derive_arc_seed.extract_json_value_paths` (Python sister script);
/// kept in sync by hand because the two run in different toolchains.
fn extract_json_value_paths(proc_body: &str) -> Vec<String> {
    // Plain-bytes search to avoid pulling in regex as a dep here. The
    // pattern is `JSON_VALUE(@jRequest, '$.x.y')`; we scan case-
    // insensitively for the literal `JSON_VALUE(@jRequest,` prefix and
    // then read the path string that follows.
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::<String>::new();
    let body_lc = proc_body.to_ascii_lowercase();
    let needle = "json_value(@jrequest,";
    let mut i = 0;
    while let Some(rel) = body_lc[i..].find(needle) {
        let start = i + rel + needle.len();
        let rest = &proc_body[start..];
        // Skip whitespace.
        let rest = rest.trim_start();
        let bytes = rest.as_bytes();
        if bytes.first() != Some(&b'\'') {
            i = start;
            continue;
        }
        // Read string literal up to the closing single quote (T-SQL
        // escapes a single quote by doubling — handle that).
        let mut j = 1;
        let mut path = String::new();
        while j < bytes.len() {
            let c = bytes[j];
            if c == b'\'' {
                if j + 1 < bytes.len() && bytes[j + 1] == b'\'' {
                    path.push('\'');
                    j += 2;
                    continue;
                }
                break;
            }
            path.push(c as char);
            j += 1;
        }
        if path.starts_with("$.") && seen.insert(path.clone()) {
            out.push(path);
        }
        i = start + j + 1;
    }
    out
}

fn resolve_proc_path(workspace_root: &Path, proc_root_template: &str, ep: &EndpointRow) -> PathBuf {
    let schema = ep.schema.clone().unwrap_or_default();
    let dir = workspace_root.join(proc_root_template.replace("{schema}", &schema));
    dir.join(format!("{}.sql", ep.proc_name()))
}

/// Walk the rendered proc, return list of (prc_Code, prc_Desc) for codes
/// the bundle should generate negative fixtures for. Sources:
///   - `@said-managed: Ignore` slots (entity-specific error branches).
///   - `@said-managed: Fully` shared validation fragments (e.g.
///     in-try-list-paging-validation). Shared error codes that belong to
///     framework canon (1006 / -15 / -1) are filtered via SHARED_PRC_CODES.
fn scan_ignore_slot_prc_codes(proc_path: &Path) -> Result<Vec<(i64, String)>, String> {
    let text = std::fs::read_to_string(proc_path)
        .map_err(|e| format!("read {}: {}", proc_path.display(), e))?;
    let mut inside_scannable = false;
    let mut codes_with_desc: Vec<(i64, String)> = Vec::new();
    let mut seen: BTreeSet<i64> = BTreeSet::new();

    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with("-- @said-managed: Ignore")
            || stripped.starts_with("-- @said-managed: Fully")
        {
            inside_scannable = true;
            continue;
        }
        if stripped.starts_with("-- @said-managed: end") {
            inside_scannable = false;
            continue;
        }
        if !inside_scannable {
            continue;
        }
        // Look for: WHERE prc_Code = <N> ... -- [<N>] - <desc>
        if let Some(code_match) = find_prc_code(line) {
            if SHARED_PRC_CODES.contains(&code_match) {
                continue;
            }
            if !seen.insert(code_match) {
                continue;
            }
            let desc = extract_inline_desc(line).unwrap_or_default();
            codes_with_desc.push((code_match, desc));
        }
    }
    Ok(codes_with_desc)
}

fn find_prc_code(line: &str) -> Option<i64> {
    // Match `WHERE prc_Code = <int>` (case-insensitive on WHERE/prc_Code).
    let lower = line.to_lowercase();
    let idx = lower.find("where prc_code")?;
    let after = &line[idx..];
    let eq_pos = after.find('=')?;
    let tail = &after[eq_pos + 1..];
    let mut num = String::new();
    let mut started = false;
    for c in tail.chars() {
        if c == '-' && !started {
            num.push(c);
            started = true;
            continue;
        }
        if c.is_ascii_digit() {
            num.push(c);
            started = true;
        } else if started {
            break;
        }
    }
    if num.is_empty() {
        None
    } else {
        num.parse().ok()
    }
}

fn extract_inline_desc(line: &str) -> Option<String> {
    // Find `-- [<code>] - <text>` and return the text.
    let dash_dash = line.find("-- [")?;
    let after = &line[dash_dash + 4..];
    let close = after.find(']')?;
    let after_close = &after[close + 1..];
    let dash = after_close.find('-')?;
    Some(after_close[dash + 1..].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(
            slugify_for_filename("[60301] - Page cannot be less than 1"),
            "pageCannotBeLessThan1"
        );
        assert_eq!(
            slugify_for_filename("[60303] - Maximum limit is XXX"),
            "maximumLimitIsXxx"
        );
    }

    #[test]
    fn find_prc_code_basic() {
        assert_eq!(
            find_prc_code("                WHERE prc_Code = 60301 -- [60301] - foo"),
            Some(60301)
        );
        assert_eq!(
            find_prc_code("    WHERE prc_Code = -15 -- [-15] - bar"),
            Some(-15)
        );
        assert_eq!(find_prc_code("SELECT 1"), None);
    }

    #[test]
    fn extract_desc_basic() {
        assert_eq!(
            extract_inline_desc("WHERE prc_Code = 60301 -- [60301] - Page cannot be less than 1"),
            Some("Page cannot be less than 1".into())
        );
    }
}

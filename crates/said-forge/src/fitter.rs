//! Spec-to-database fitter — the self-healing loop.
//!
//! After `generate_openapi` produces a first-cut spec from the SQL
//! catalog, this module reads the **live database's API registry** and
//! corrects the spec to match. Where the database disagrees with the
//! generator's heuristic guess, the database wins.
//!
//! Three correction classes:
//!
//! 1. **Path corrections.** Generator emits `/cardholder` (singular,
//!    derived from proc name) but the registry has `/cardholders`
//!    (plural, dt convention). Rewrite spec path to match registry.
//!
//! 2. **Parameter corrections.** Generator infers `format: uuid` from
//!    a proc-body OPENJSON walk but `sys.parameters` says the actual
//!    type is `VARCHAR(20)`. Rewrite to match `sys.parameters`.
//!
//! 3. **Enum corrections.** Generator emits a `description` for a
//!    lookup-validated field but the live row count says it's a
//!    closed set. Inject the enum values. (Already done in the
//!    existing `--verify-against-sandbox` pass — this module just
//!    surfaces it in the corrections log.)
//!
//! Two diagnostic categories — never auto-applied, always logged:
//!
//! - **Missing-op (Scenario A — auto-fix):** registry has
//!   `/products GET` and `sys.procedures` contains a proc whose name
//!   matches (e.g. `p_txn_API_Get_Products`) but our generator's
//!   verb-mapper missed it. Add the op to the spec, tag
//!   `x-source: registry-mapped`.
//! - **Missing-op (Scenarios B, C — log only):** registry references
//!   a proc that doesn't exist anywhere, OR the proc exists but is
//!   out of catalog scope. Log to `corrections.md`.
//!
//! Surplus-op (spec has it, registry doesn't): for v1, **leave in
//! the spec untouched.** Decide on the next run after seeing what
//! the corrected output looks like.
//!
//! The whole pass is database-agnostic — we look for "any table that
//! pairs UUIDs with paths" and treat it as the registry. Falls back
//! gracefully when nothing matches.

#![cfg(feature = "forge-sql-verify")]

use crate::sql_verify::{sandbox_config, SandboxInfo};
use futures_util::TryStreamExt;
use std::collections::BTreeMap;
use std::path::Path;
use tiberius::{Client, QueryItem};
use tokio_util::compat::TokioAsyncWriteCompatExt;

/// One row of the database's API registry. Two registry shapes
/// supported today:
///
///   - **TXN-shape** (`lookups.ars_Api_Rule_Settings`): credentialled,
///     each row carries a UUID `api_id` that the proc validates against.
///   - **Vivere-shape** (`dbo.ael_API_Endpoint_Lookup`): documentation-
///     only, no per-row credential. `api_id` is `Uuid::nil()` for
///     these rows; downstream code checks `has_credential` to know
///     whether Gate 4 can bind a real ID.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Caller credential when the registry has one. `Uuid::nil()`
    /// when the registry is documentation-only (Vivere shape).
    pub api_id: uuid::Uuid,
    /// True when `api_id` is a real registered credential the proc
    /// validates against. False for documentation-only registries.
    pub has_credential: bool,
    /// Path template — REST-normalised (lowercase, dashed). Used for
    /// matching against spec paths and emitted into the spec.
    pub path: String,
    /// Original path as it appears in the source registry (e.g.
    /// `/Card/Activate` for Vivere). Preserved for diagnostics +
    /// the corrections.md "registry uses non-REST case" log.
    pub original_path: String,
    /// Optional HTTP method (GET/POST/PUT/PATCH/DELETE).
    pub method: Option<String>,
    /// Whether the row is enabled (defaults to true when the
    /// underlying table doesn't carry an enabled flag).
    pub enabled: bool,
    /// Origin tag for diagnostics (e.g. `"ars_Api_Rule_Settings"`,
    /// `"ael_API_Endpoint_Lookup"`).
    pub source_table: String,
}

/// Aggregated correction-pass result. Returned to the caller so it
/// can print a one-liner ("12 corrections applied") and write the
/// detailed log to disk.
#[derive(Debug, Default)]
pub struct FitReport {
    /// Path rewrites applied (`/cardholder` → `/cardholders`).
    pub path_rewrites: Vec<PathRewrite>,
    /// Parameter type/name rewrites from `sys.parameters`.
    pub param_rewrites: Vec<ParamRewrite>,
    /// Missing-op auto-adds (Scenario A).
    pub missing_ops_added: Vec<MissingOpAdd>,
    /// Missing-op diagnostics (Scenarios B + C).
    pub missing_ops_logged: Vec<MissingOpLog>,
    /// Surplus-op count (left in spec untouched for v1).
    pub surplus_ops: Vec<String>,
    /// Total registry rows we read.
    pub registry_size: usize,
    /// Origin of the registry — `ars_Api_Rule_Settings` (TXN-shape),
    /// `ael_API_Endpoint_Lookup` (Vivere-shape), or empty when no
    /// registry was found.
    pub registry_source: String,
    /// REST-normalisation rewrites — registry path → spec path when
    /// they differed (Vivere's `/Card/Activate` → `/card/activate`).
    /// Documented in corrections.md so consumers see the convention
    /// shift between live API and forward-looking spec.
    pub rest_normalisations: Vec<RestNormalisation>,
    /// Path-param-casing rewrites driven by `OpenApiStandard.paths.param_casing`.
    /// Recorded when the configured rule rewrote a `{snake_case_id}` to
    /// `{camelCaseId}`. Surfaced in `fixes.md` (separate from
    /// corrections.md so standard-driven rewrites don't get mixed
    /// with proc-binding diagnostics).
    pub standard_normalisations: Vec<StandardNormalisation>,
}

#[derive(Debug, Clone)]
pub struct StandardNormalisation {
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RestNormalisation {
    pub registry_path: String,
    pub spec_path: String,
}

#[derive(Debug, Clone)]
pub struct PathRewrite {
    pub from: String,
    pub to: String,
    pub method: String,
    pub backed_by: String,
}

#[derive(Debug, Clone)]
pub struct ParamRewrite {
    pub op_path: String,
    pub method: String,
    pub field: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct MissingOpAdd {
    pub path: String,
    pub method: String,
    pub backed_by: String,
}

#[derive(Debug, Clone)]
pub struct MissingOpLog {
    pub path: String,
    pub method: Option<String>,
    pub reason: String,
}

impl FitReport {
    pub fn total_corrections(&self) -> usize {
        self.path_rewrites.len()
            + self.param_rewrites.len()
            + self.missing_ops_added.len()
    }

    pub fn has_anything(&self) -> bool {
        self.total_corrections() > 0
            || !self.missing_ops_logged.is_empty()
            || !self.surplus_ops.is_empty()
            || !self.rest_normalisations.is_empty()
    }
}

/// String-in / string-out wrapper. Parses the input YAML, runs the
/// fit pass, serialises back. Returns `(corrected_yaml_string, report)`.
///
/// `catalog` is the SQL catalog forge already built — used by Gate 6
/// (FK-walk) to discriminate sibling-candidate procs by which table
/// they primarily SELECT from. Passing an empty catalog disables
/// Gate 6 (the fitter degrades to Gates 1-5 only).
/// Registry-first spec generation. Skips the heuristic name-mapper:
/// every (path, aml_Code) pair in `ars_Api_Rule_Settings` (or the
/// equivalent Vivere lookup) becomes one operation in the spec, and
/// the existing fitter binds each one to a proc via the 6-gate matcher.
///
/// Falls back to `Ok(None)` when the sandbox has no recognised
/// registry — the caller decides whether to use the catalog-walk
/// generator instead.
pub async fn build_spec_from_registry(
    sandbox: &SandboxInfo,
    title: &str,
    catalog: &crate::sql_catalog::SqlCatalog,
    dev_spec_endpoints: Option<&[crate::dev_spec::types::DevSpecEndpoint]>,
    standard: &crate::OpenApiStandard,
    resolutions: &AmbiguityResolutions,
) -> Result<Option<(String, FitReport)>, String> {
    let registry = fetch_registry(sandbox).await.unwrap_or_default();
    if registry.is_empty() {
        return Ok(None);
    }

    // Build a minimal OpenAPI document with one (path, method) entry
    // per registry row. The fit() pass below then runs apply_missing_op_pass
    // on EVERY entry (since the spec is empty of paths) — binding each
    // path to a proc through the existing 6-gate matcher.
    let mut doc = serde_yaml::Mapping::new();
    doc.insert(serde_yaml::Value::String("openapi".into()),
               serde_yaml::Value::String("3.0.3".into()));
    let mut info = serde_yaml::Mapping::new();
    info.insert(serde_yaml::Value::String("title".into()),
                serde_yaml::Value::String(title.to_string()));
    info.insert(serde_yaml::Value::String("version".into()),
                serde_yaml::Value::String("0.1.0".into()));
    info.insert(serde_yaml::Value::String("description".into()),
                serde_yaml::Value::String(format!(
                    "Generated from API registry — every endpoint listed in the live database's `{}` table is emitted here. Proc bindings are auto-applied where the matcher finds a single confident candidate.",
                    registry.first().map(|e| e.source_table.clone()).unwrap_or_default()
                )));
    doc.insert(serde_yaml::Value::String("info".into()),
               serde_yaml::Value::Mapping(info));
    doc.insert(serde_yaml::Value::String("paths".into()),
               serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

    let spec = serde_yaml::Value::Mapping(doc);
    let (corrected_yaml_value, report) = fit_with_resolutions(
        sandbox, spec, catalog, standard, resolutions,
    ).await?;
    let mut corrected = corrected_yaml_value;
    // Optional Dev Spec enrichment: replace each operation body with a
    // fully-populated one (headers, requestBody schema ref, response
    // envelope) and inject `components/schemas`. Only runs when the
    // caller has parsed Dev Spec markdown and passed the endpoints in.
    if let Some(dev_eps) = dev_spec_endpoints {
        let dev_map: std::collections::BTreeMap<
            (String, String),
            &crate::dev_spec::types::DevSpecEndpoint,
        > = dev_eps
            .iter()
            .map(|ep| ((ep.method.to_uppercase(), ep.path.clone()), ep))
            .collect();
        enrich_operations_with_dev_spec(&mut corrected, &dev_map, standard);
        inject_components_schemas(&mut corrected, dev_eps, standard);
    }
    let yaml = serde_yaml::to_string(&corrected)
        .map_err(|e| format!("serialize spec: {}", e))?;
    Ok(Some((yaml, report)))
}

/// Walk every `(path, method)` operation in a fitted spec and, when
/// the Dev Spec map carries a matching entry, replace the whole
/// operation body with a freshly-built one from `build_operation_yaml`.
/// The proc binding is preserved by reading it out of the existing
/// `operationId` (best effort).
fn enrich_operations_with_dev_spec(
    spec: &mut serde_yaml::Value,
    dev_map: &std::collections::BTreeMap<
        (String, String),
        &crate::dev_spec::types::DevSpecEndpoint,
    >,
    standard: &crate::OpenApiStandard,
) {
    use crate::dev_spec::openapi_emit::build_operation_yaml;
    let paths_node = match spec.get_mut("paths").and_then(|v| v.as_mapping_mut()) {
        Some(m) => m,
        None => return,
    };
    let path_keys: Vec<String> = paths_node
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect();
    for path_key in path_keys {
        let path_val = match paths_node
            .get_mut(&serde_yaml::Value::String(path_key.clone()))
        {
            Some(v) => v,
            None => continue,
        };
        let ops_map = match path_val.as_mapping_mut() {
            Some(m) => m,
            None => continue,
        };
        let method_keys: Vec<String> = ops_map
            .keys()
            .filter_map(|k| k.as_str().map(|s| s.to_uppercase()))
            .collect();
        for method in method_keys {
            let dev_ep = match dev_map.get(&(method.clone(), path_key.clone())) {
                Some(ep) => ep,
                None => continue,
            };
            let lc_method = method.to_lowercase();
            // Pull proc binding out of the existing operation's
            // operationId — best effort.
            // operationId encoding: "<verb>_<schema>_<proc_with_underscores>".
            // Only the FIRST underscore after stripping the verb prefix
            // separates schema from proc-name; subsequent underscores are
            // part of the proc-name itself (e.g. `p_txn_Create_Cardholder`).
            let proc_full = ops_map
                .get(&serde_yaml::Value::String(lc_method.clone()))
                .and_then(|v| v.as_mapping())
                .and_then(|m| m.get(&serde_yaml::Value::String("operationId".into())))
                .and_then(|v| v.as_str())
                .map(|s| {
                    let stripped = s.trim_start_matches(&format!("{}_", lc_method));
                    stripped.splitn(2, '_').collect::<Vec<_>>().join(".")
                })
                .unwrap_or_else(|| "registry.unknown".to_string());
            let new_op = build_operation_yaml(dev_ep, &proc_full, standard);
            ops_map.insert(serde_yaml::Value::String(lc_method), new_op);
        }
    }
}

/// Insert a `components/schemas` block at the spec root, generated
/// from every Dev Spec endpoint's request/response schemas plus the
/// always-present `ApiError` model.
fn inject_components_schemas(
    spec: &mut serde_yaml::Value,
    endpoints: &[crate::dev_spec::types::DevSpecEndpoint],
    standard: &crate::OpenApiStandard,
) {
    use crate::dev_spec::openapi_emit::emit_components_schemas;
    let schemas = emit_components_schemas(endpoints, standard);
    let root = match spec.as_mapping_mut() {
        Some(m) => m,
        None => return,
    };
    let mut components = serde_yaml::Mapping::new();
    components.insert(serde_yaml::Value::String("schemas".into()), schemas);
    root.insert(
        serde_yaml::Value::String("components".into()),
        serde_yaml::Value::Mapping(components),
    );
}

pub async fn fit_yaml(
    sandbox: &SandboxInfo,
    spec_yaml_str: &str,
    catalog: &crate::sql_catalog::SqlCatalog,
    standard: &crate::OpenApiStandard,
) -> Result<(String, FitReport), String> {
    let parsed: serde_yaml::Value = serde_yaml::from_str(spec_yaml_str)
        .map_err(|e| format!("parse spec yaml: {}", e))?;
    let (corrected, report) = fit(sandbox, parsed, catalog, standard).await?;
    let out = serde_yaml::to_string(&corrected)
        .map_err(|e| format!("serialize corrected spec: {}", e))?;
    Ok((out, report))
}

/// Run the self-healing pass over an in-memory spec YAML, returning
/// the corrected YAML + a structured report. The caller writes both
/// to disk.
pub async fn fit(
    sandbox: &SandboxInfo,
    spec_yaml: serde_yaml::Value,
    catalog: &crate::sql_catalog::SqlCatalog,
    standard: &crate::OpenApiStandard,
) -> Result<(serde_yaml::Value, FitReport), String> {
    fit_with_resolutions(sandbox, spec_yaml, catalog, standard, &AmbiguityResolutions::empty()).await
}

/// Same as `fit` but lets the caller pre-load operator-supplied
/// ambiguity resolutions. The matcher checks the resolutions before
/// running its 6 gates; resolved (path, method) pairs are bound
/// directly to the chosen proc.
pub async fn fit_with_resolutions(
    sandbox: &SandboxInfo,
    spec_yaml: serde_yaml::Value,
    catalog: &crate::sql_catalog::SqlCatalog,
    standard: &crate::OpenApiStandard,
    resolutions: &AmbiguityResolutions,
) -> Result<(serde_yaml::Value, FitReport), String> {
    let mut report = FitReport::default();

    // 1. Read the database's API registry. Returns empty when the
    //    sandbox doesn't have one — fitter degrades gracefully.
    let registry = fetch_registry(sandbox).await.unwrap_or_default();
    report.registry_size = registry.len();
    report.registry_source = registry
        .first()
        .map(|e| e.source_table.clone())
        .unwrap_or_default();
    // Collect REST-normalisation diffs as a separate signal — every
    // registry row whose original_path differs from the normalised
    // path counts as a "live API uses non-REST shape" note.
    for entry in &registry {
        if entry.original_path != entry.path {
            report.rest_normalisations.push(RestNormalisation {
                registry_path: entry.original_path.clone(),
                spec_path: entry.path.clone(),
            });
        }
    }
    if registry.is_empty() {
        // Nothing to correct against. Pass-through.
        return Ok((spec_yaml, report));
    }

    let mut spec = spec_yaml;

    // 2. Apply path corrections. Walk every operation in the spec,
    //    look up the proc it's backed by in the registry by proc-name
    //    affinity (proc → likely registry row by entity overlap),
    //    rewrite the path key when the registry says different.
    apply_path_corrections(&mut spec, &registry, &mut report);

    // 3. Apply parameter corrections from sys.parameters. For each
    //    op, fetch the proc's actual parameter list and rewrite the
    //    spec's path/header parameter types where they disagree.
    apply_param_corrections(sandbox, &mut spec, &mut report).await?;

    // 4. Auto-add missing ops (Scenario A) + log B/C.
    apply_missing_op_pass(sandbox, &mut spec, &registry, catalog, &mut report, standard, resolutions).await?;

    // 5. Catalog surplus ops (left in spec, just count them).
    catalog_surplus(&spec, &registry, &mut report);

    Ok((spec, report))
}

/// Read the API registry from any sandbox. Looks for
/// `lookups.ars_Api_Rule_Settings` first (dt convention); future
/// extension: discover any table pairing UUIDs with paths.
pub async fn fetch_registry(sandbox: &SandboxInfo) -> Result<Vec<RegistryEntry>, String> {
    let cfg = sandbox_config(sandbox.host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", sandbox.host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;

    // Try registries in priority order. First hit wins. Each query
    // returns Vec<RegistryEntry>; an empty result means "table absent
    // / no rows" and we fall through to the next shape.
    //
    // Future Gate (b): replace this hardcoded list with a
    // `sys.tables` introspection that finds any column where rows
    // start with `/` — universal "this column holds API paths"
    // signal. For now, list shipping shapes explicitly.

    // 1. TXN shape — `lookups.ars_Api_Rule_Settings` (UUID-credentialled).
    // `aml_Code` IS the verb ("GET", "PUT", ...); the lookup's
    // `aml_Description` is human prose ("Retrieve resource"), not the
    // verb. So the registry row's method is simply `ars.aml_Code`.
    let txn_full = "\
        SELECT ars_Api_Id, ars_Path, aml_Code AS method, \
               ISNULL(ars_Enabled, 1) AS enabled \
        FROM lookups.ars_Api_Rule_Settings;";
    let txn_bare = "\
        SELECT ars_Api_Id, ars_Path, NULL AS method, \
               ISNULL(ars_Enabled, 1) AS enabled \
        FROM lookups.ars_Api_Rule_Settings;";
    if let Ok(rows) = drain_registry_query_txn(&mut client, txn_full).await {
        if !rows.is_empty() {
            return Ok(rows);
        }
    }
    if let Ok(rows) = drain_registry_query_txn(&mut client, txn_bare).await {
        if !rows.is_empty() {
            return Ok(rows);
        }
    }

    // 2. Vivere shape — `dbo.ael_API_Endpoint_Lookup` (no credential
    //    column; documentation-only).
    let vivere_sql = "\
        SELECT aac_Api_Path, ael_Rest_Type \
        FROM dbo.ael_API_Endpoint_Lookup;";
    if let Ok(rows) = drain_registry_query_vivere(&mut client, vivere_sql).await {
        if !rows.is_empty() {
            return Ok(rows);
        }
    }

    Ok(Vec::new())
}

/// TXN-shape drain: row layout (api_id UUID, path, method, enabled).
async fn drain_registry_query_txn(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
    sql: &str,
) -> Result<Vec<RegistryEntry>, String> {
    let mut stream = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("registry query: {}", e))?;
    let mut out: Vec<RegistryEntry> = Vec::new();
    while let Some(item) = stream
        .try_next()
        .await
        .map_err(|e| format!("registry stream: {}", e))?
    {
        if let QueryItem::Row(row) = item {
            let id: Option<uuid::Uuid> = row.try_get(0).ok().flatten();
            let path: Option<&str> = row.try_get(1).ok().flatten();
            let method: Option<&str> = row.try_get(2).ok().flatten();
            let enabled: Option<bool> = row.try_get(3).ok().flatten();
            if let (Some(id), Some(p)) = (id, path) {
                let normalised = normalise_path_rest(p);
                out.push(RegistryEntry {
                    api_id: id,
                    has_credential: true,
                    path: normalised,
                    original_path: p.to_string(),
                    method: method.map(|s| s.to_string()),
                    enabled: enabled.unwrap_or(true),
                    source_table: "ars_Api_Rule_Settings".to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// Vivere-shape drain: row layout (path, rest_type). No credential.
async fn drain_registry_query_vivere(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
    sql: &str,
) -> Result<Vec<RegistryEntry>, String> {
    let mut stream = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("registry query: {}", e))?;
    let mut out: Vec<RegistryEntry> = Vec::new();
    while let Some(item) = stream
        .try_next()
        .await
        .map_err(|e| format!("registry stream: {}", e))?
    {
        if let QueryItem::Row(row) = item {
            let path: Option<&str> = row.try_get(0).ok().flatten();
            let method: Option<&str> = row.try_get(1).ok().flatten();
            if let Some(p) = path {
                let normalised = normalise_path_rest(p);
                out.push(RegistryEntry {
                    api_id: uuid::Uuid::nil(),
                    has_credential: false,
                    path: normalised,
                    original_path: p.to_string(),
                    method: method.map(|s| s.to_string()),
                    enabled: true,
                    source_table: "ael_API_Endpoint_Lookup".to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// REST-normalise a path: lowercase + dashes between camelCase
/// boundaries. Vivere's registry uses PascalCase paths
/// (`/Card/Activate`); we always emit REST style (`/card/activate`).
/// Path parameters keep their `{name}` shape — only literal segments
/// are normalised.
///
/// Examples:
///   `/Card/Activate`              → `/card/activate`
///   `/Adhoc/AddDisputeRequest`    → `/adhoc/add-dispute-request`
///   `/cards/{cardId}`             → `/cards/{cardId}`  (already REST)
///   `/Account/{accountId}/Balance` → `/account/{accountId}/balance`
fn normalise_path_rest(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 4);
    for seg in path.split('/') {
        if seg.is_empty() {
            continue;
        }
        out.push('/');
        // Path-param `{xxx}` — keep verbatim (consumers rely on the
        // exact param name in their generated client code).
        if seg.starts_with('{') && seg.ends_with('}') {
            out.push_str(seg);
            continue;
        }
        // Insert a `-` before each ASCII uppercase letter that isn't
        // at the start, then lowercase the whole thing.
        let mut prev_upper = false;
        for (i, ch) in seg.chars().enumerate() {
            if i > 0 && ch.is_ascii_uppercase() && !prev_upper {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
            prev_upper = ch.is_ascii_uppercase();
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// Walk every spec operation. When the spec's path doesn't appear in
/// the registry but a registry path matches by entity-overlap shape
/// (same number of segments, same path-param positions, same literal
/// segment with one off-by-one plural difference), rewrite the spec's
/// path key to the registry's path. Database wins.
fn apply_path_corrections(
    spec: &mut serde_yaml::Value,
    registry: &[RegistryEntry],
    report: &mut FitReport,
) {
    let paths_node = match spec
        .get_mut("paths")
        .and_then(|v| v.as_mapping_mut())
    {
        Some(m) => m,
        None => return,
    };

    // Build a (normalised_segments, registry_path) index for fast lookup.
    // `normalise` keys collapse path params to `{x}` so registry vs spec
    // can compare structures regardless of param naming.
    let normalise = |p: &str| -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty())
            .map(|seg| {
                if seg.starts_with('{') && seg.ends_with('}') {
                    "{x}".to_string()
                } else {
                    seg.to_lowercase()
                }
            })
            .collect()
    };

    // Build registry index keyed by structural shape.
    // Multiple registry rows can share a structure (different methods,
    // different param names) — store all of them and pick the best.
    let mut registry_by_shape: BTreeMap<Vec<String>, Vec<&RegistryEntry>> = BTreeMap::new();
    for entry in registry {
        registry_by_shape
            .entry(normalise(&entry.path))
            .or_default()
            .push(entry);
    }

    // Walk the spec's path keys. Collect rewrites first, then apply
    // (can't mutate the map while iterating).
    let mut rewrites: Vec<(String, String, String, String)> = Vec::new(); // (from, to, method, backed_by)
    let keys: Vec<String> = paths_node
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect();
    for spec_path in &keys {
        // Skip internal-marked paths and exact registry matches.
        if spec_path.starts_with("/internal/") {
            continue;
        }
        let spec_norm = normalise(spec_path);
        // Exact-shape match in registry?
        let registry_match = registry_by_shape
            .iter()
            .find(|(shape, _)| {
                if shape.len() != spec_norm.len() {
                    return false;
                }
                shape.iter().zip(&spec_norm).all(|(a, b)| {
                    if a == "{x}" || b == "{x}" {
                        return true;
                    }
                    // Tolerant literal match: identical OR exactly
                    // one plural-s away. Caller decides what wins.
                    if a == b {
                        return true;
                    }
                    plural_off_by_one(a, b)
                })
            });
        let (registry_shape, candidates) = match registry_match {
            Some(m) => m,
            None => continue,
        };
        // Pick the registry path whose literal text differs MOST from
        // ours (i.e. an actual rewrite), preferring same-method when
        // multiple candidates exist. Fall back to first.
        let target = candidates
            .iter()
            .find(|c| &normalise(&c.path) == registry_shape && c.path != *spec_path)
            .copied();
        let target = match target {
            Some(t) => t,
            None => continue, // Already matches verbatim.
        };
        // Find the method(s) under this spec path so we know what
        // proc each rewrite is "backed by". We rewrite ALL methods
        // under the path together — they share the URL.
        let methods_under: Vec<(String, String)> = paths_node
            .get(serde_yaml::Value::String(spec_path.clone()))
            .and_then(|v| v.as_mapping())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| {
                        let method = k.as_str()?.to_string();
                        if !matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete") {
                            return None;
                        }
                        let backed = v
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some((method, backed))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (method, backed_desc) in methods_under {
            // Backed-by is the first ` ` `…` ` ` segment of description.
            let backed = backed_desc
                .find('`')
                .and_then(|i| {
                    backed_desc[i + 1..].find('`').map(|j| {
                        backed_desc[i + 1..i + 1 + j].to_string()
                    })
                })
                .unwrap_or_default();
            rewrites.push((
                spec_path.clone(),
                target.path.clone(),
                method,
                backed,
            ));
        }
    }

    // Apply rewrites (deduplicate by `from` since one path may have
    // had several methods producing one rewrite per method).
    let mut applied_paths: BTreeMap<String, String> = BTreeMap::new();
    for (from, to, method, backed) in &rewrites {
        applied_paths.insert(from.clone(), to.clone());
        report.path_rewrites.push(PathRewrite {
            from: from.clone(),
            to: to.clone(),
            method: method.clone(),
            backed_by: backed.clone(),
        });
    }
    for (from, to) in &applied_paths {
        if from == to {
            continue;
        }
        // Move the entry. If `to` already exists in the map (registry
        // path collision with another spec entry), merge methods.
        let from_key = serde_yaml::Value::String(from.clone());
        let to_key = serde_yaml::Value::String(to.clone());
        if let Some(item) = paths_node.remove(&from_key) {
            // If a `to` entry already exists, merge method maps; else
            // insert directly.
            if let Some(existing) = paths_node.get_mut(&to_key) {
                if let (Some(existing_map), Some(new_map)) =
                    (existing.as_mapping_mut(), item.as_mapping())
                {
                    for (k, v) in new_map {
                        existing_map.insert(k.clone(), v.clone());
                    }
                    continue;
                }
            }
            paths_node.insert(to_key, item);
        }
    }
}

/// Cheap "off-by-one s" check for spec ↔ registry plural mismatch.
fn plural_off_by_one(a: &str, b: &str) -> bool {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if long.len() != short.len() + 1 {
        return false;
    }
    long.starts_with(short) && (long.ends_with('s') || long.ends_with("es"))
}

/// Per-op param rewrites driven by `sys.parameters`. For each spec
/// operation, query the proc's actual parameter list and reconcile
/// against the spec's path/header param types. Body fields are
/// trickier (JSON-shape inference) — punt for v1 and only correct
/// path UUID vs string mismatches.
async fn apply_param_corrections(
    sandbox: &SandboxInfo,
    spec: &mut serde_yaml::Value,
    report: &mut FitReport,
) -> Result<(), String> {
    let cfg = sandbox_config(sandbox.host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", sandbox.host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;

    let paths_node = match spec.get_mut("paths").and_then(|v| v.as_mapping_mut()) {
        Some(m) => m,
        None => return Ok(()),
    };

    // Snapshot the ops first so we can mutate the map after.
    let snapshot: Vec<(String, String, String)> = paths_node
        .iter()
        .filter_map(|(k, v)| {
            let path = k.as_str()?.to_string();
            let methods = v.as_mapping()?;
            // For each method, pull the proc full name from the description.
            let mut out: Vec<(String, String, String)> = Vec::new();
            for (mk, mv) in methods.iter() {
                let method = mk.as_str().unwrap_or("").to_string();
                if !matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete") {
                    continue;
                }
                let desc = mv.get("description").and_then(|d| d.as_str()).unwrap_or("");
                let proc_full = extract_proc_from_backref(desc);
                if !proc_full.is_empty() {
                    out.push((path.clone(), method, proc_full));
                }
            }
            Some(out)
        })
        .flatten()
        .collect();

    for (path, method, proc_full) in snapshot {
        let proc_params = match fetch_proc_params(&mut client, &proc_full).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        // Walk this op's `parameters:` looking for path params whose
        // schema disagrees with the proc.
        let path_key = serde_yaml::Value::String(path.clone());
        let method_key = serde_yaml::Value::String(method.clone());
        let op = match paths_node
            .get_mut(&path_key)
            .and_then(|v| v.as_mapping_mut())
            .and_then(|m| m.get_mut(&method_key))
            .and_then(|v| v.as_mapping_mut())
        {
            Some(m) => m,
            None => continue,
        };
        let params_seq = match op
            .get_mut(&serde_yaml::Value::String("parameters".to_string()))
            .and_then(|v| v.as_sequence_mut())
        {
            Some(s) => s,
            None => continue,
        };
        for param in params_seq.iter_mut() {
            let pmap = match param.as_mapping_mut() {
                Some(m) => m,
                None => continue,
            };
            // Skip $ref params.
            if pmap.contains_key(&serde_yaml::Value::String("$ref".to_string())) {
                continue;
            }
            let pname = pmap
                .get(&serde_yaml::Value::String("name".to_string()))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let in_loc = pmap
                .get(&serde_yaml::Value::String("in".to_string()))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if in_loc != "path" {
                continue;
            }
            // Match this path-param to a proc parameter by name affinity.
            let want = pname.to_lowercase();
            let proc_param = proc_params.iter().find(|p| {
                let n = p.name.to_lowercase();
                n.ends_with(&want) || n == format!("u{}", want) || n == format!("s{}", want)
            });
            let proc_param = match proc_param {
                Some(p) => p,
                None => continue,
            };
            let desired_format = match proc_param.data_type.to_uppercase().as_str() {
                "UNIQUEIDENTIFIER" => "uuid",
                "INT" | "BIGINT" | "SMALLINT" | "TINYINT" => "integer-suggested",
                _ => "string",
            };
            // Read current schema.format / schema.type.
            let schema_map = match pmap
                .get_mut(&serde_yaml::Value::String("schema".to_string()))
                .and_then(|v| v.as_mapping_mut())
            {
                Some(m) => m,
                None => continue,
            };
            let cur_format = schema_map
                .get(&serde_yaml::Value::String("format".to_string()))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // Only correct UUID mismatches for v1.
            if desired_format == "uuid" && cur_format.as_deref() != Some("uuid") {
                schema_map.insert(
                    serde_yaml::Value::String("format".to_string()),
                    serde_yaml::Value::String("uuid".to_string()),
                );
                report.param_rewrites.push(ParamRewrite {
                    op_path: path.clone(),
                    method: method.clone(),
                    field: pname.clone(),
                    from: cur_format.unwrap_or_else(|| "(none)".to_string()),
                    to: "uuid".to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Auto-add missing ops (Scenario A) + log Scenarios B/C.
async fn apply_missing_op_pass(
    sandbox: &SandboxInfo,
    spec: &mut serde_yaml::Value,
    registry: &[RegistryEntry],
    catalog: &crate::sql_catalog::SqlCatalog,
    report: &mut FitReport,
    standard: &crate::OpenApiStandard,
    resolutions: &AmbiguityResolutions,
) -> Result<(), String> {
    // Collect path-param-casing rewrites locally so add_op_to_spec
    // doesn't need a mutable borrow of report alongside other mutations.
    // Drained into report.standard_normalisations at the end.
    let mut local_norms: Vec<StandardNormalisation> = Vec::new();
    let cfg = sandbox_config(sandbox.host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", sandbox.host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;

    let normalise = |p: &str| -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty())
            .map(|seg| {
                if seg.starts_with('{') && seg.ends_with('}') {
                    "{x}".to_string()
                } else {
                    seg.to_lowercase()
                }
            })
            .collect()
    };
    // Build (shape, method) pairs from the existing spec so we only
    // skip a registry entry when the SAME path AND method are already
    // covered. Without this, POST `/cardholders` gets silently dropped
    // when the spec already has PUT `/cardholders` under the same path.
    let mut known_shape_methods: std::collections::BTreeSet<(Vec<String>, String)> =
        std::collections::BTreeSet::new();
    if let Some(paths_map) = spec.get("paths").and_then(|v| v.as_mapping()) {
        for (path_key, ops_val) in paths_map.iter() {
            let path_str = match path_key.as_str() {
                Some(s) => s.to_lowercase(),
                None => continue,
            };
            let shape = normalise(&path_str);
            if let Some(ops_map) = ops_val.as_mapping() {
                for (verb_key, _) in ops_map.iter() {
                    if let Some(verb) = verb_key.as_str() {
                        let v = verb.to_lowercase();
                        if matches!(v.as_str(), "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "trace") {
                            known_shape_methods.insert((shape.clone(), v));
                        }
                    }
                }
            }
        }
    }

    for entry in registry {
        let entry_method = entry.method.as_deref().unwrap_or("get").to_lowercase();
        // Apply the same path normalisation `add_op_to_spec` will run
        // BEFORE deriving the entity token. Otherwise the matcher
        // tokenises against the raw registry path (`/products`,
        // `/cards/{card_id}`, plural+snake-case) and matches procs
        // whose names contain those raw forms — producing many false
        // ambiguities that would resolve cleanly against the singular
        // post-rule entity name.
        let normalised_entry_path = post_normalisation_path(
            &entry.path, &entry_method, standard,
        );
        let entry_shape = normalise(&normalised_entry_path);
        if known_shape_methods.contains(&(entry_shape.clone(), entry_method.clone())) {
            continue;
        }

        // Operator-supplied tiebreaker check. When the operator has
        // resolved this (path, method) in `ambiguity-resolutions.toml`,
        // bind directly to the chosen proc — bypass the matcher entirely.
        // The resolution moves the row from the ambiguous bucket into
        // the bound bucket on this run.
        if let Some(chosen_proc) = resolutions.lookup(&normalised_entry_path, &entry_method) {
            add_op_to_spec(spec, entry, chosen_proc, standard, &mut local_norms);
            report.missing_ops_added.push(MissingOpAdd {
                path: normalised_entry_path.clone(),
                method: entry_method.clone(),
                backed_by: chosen_proc.to_string(),
            });
            continue;
        }
        // Scenario A check: is there a proc whose name plausibly maps
        // to this (now-normalised) registry path? Use the last entity
        // segment as the search token. Pluralisation rule means GET
        // collections stay plural (`/accounts`), POST/PUT/DELETE/by-id
        // are singular (`/account/{accountId}`) — entity is `account`.
        let entity_token = entry_shape
            .iter()
            .rev()
            .find(|s| s.as_str() != "{x}")
            .cloned()
            .unwrap_or_default();
        if entity_token.is_empty() {
            report.missing_ops_logged.push(MissingOpLog {
                path: normalised_entry_path.clone(),
                method: entry.method.clone(),
                reason: "registry path has no resolvable entity token".into(),
            });
            continue;
        }
        // Match must agree on HTTP method too. If the registry says
        // GET, only consider procs whose name implies a read verb.
        let method_str = entry.method.as_deref().unwrap_or("get").to_lowercase();
        let allowed_verbs: &[&str] = match method_str.as_str() {
            "get" | "head" => &["get_", "read_", "find_", "list_", "search_", "is_"],
            "post" => &["create_", "add_", "new_", "insert_", "post_"],
            "put" => &["update_", "set_", "change_", "replace_", "put_"],
            "patch" => &["patch_", "update_", "set_"],
            "delete" => &["delete_", "remove_"],
            _ => &[],
        };

        // Extract path-param names from the registry path so gate 3
        // can verify the proc's signature includes them. Example:
        // `/business/{businessId}` → ["businessId"].
        let path_params: Vec<String> = entry
            .path
            .split('/')
            .filter_map(|seg| {
                if seg.starts_with('{') && seg.ends_with('}') && seg.len() > 2 {
                    Some(seg[1..seg.len() - 1].to_string())
                } else {
                    None
                }
            })
            .collect();

        // Full normalised segment list for Gate 5 — same shape as
        // entry_shape, used to find what comes after the entity word.
        let path_segments = entry_shape.clone();
        let candidate_proc = find_proc_by_entity(
            &mut client,
            &entity_token,
            allowed_verbs,
            &path_params,
            &path_segments,
            catalog,
        )
        .await?;
        match candidate_proc {
            ProcMatch::Single(proc_full) => {
                // Gates 1-3 passed. Apply Gate 4: smoke-EXEC the proc
                // with the registered API ID and synthetic UUIDs. If
                // the EXEC fails with a SQL parser error or missing-
                // object error, the proc isn't actually compatible
                // with the registry path — demote to Ambiguous so a
                // human picks. Business-rule rejections (RAISERROR
                // PrcCode JSON) count as PASS — proc executed.
                // Pass `None` when the registry has no per-row credential
                // (Vivere-shape) so smoke_exec_proc skips the API-ID
                // bind and runs a "bare" smoke instead.
                let bound = if entry.has_credential {
                    Some(entry.api_id)
                } else {
                    None
                };
                let smoke_ok = smoke_exec_proc(&mut client, &proc_full, bound).await;
                match smoke_ok {
                    Ok(()) => {
                        // Gate 4 passed — confident bind.
                        add_op_to_spec(spec, entry, &proc_full, standard, &mut local_norms);
                        report.missing_ops_added.push(MissingOpAdd {
                            path: normalised_entry_path.clone(),
                            method: method_str.clone(),
                            backed_by: proc_full,
                        });
                    }
                    Err(reason) => {
                        // Gate 4 rejected — log the failed binding so
                        // the operator sees what runtime evidence
                        // disqualified it.
                        report.missing_ops_logged.push(MissingOpLog {
                            path: normalised_entry_path.clone(),
                            method: entry.method.clone(),
                            reason: format!(
                                "GATE 4 REJECTED — `{}` failed runtime smoke: {}",
                                proc_full,
                                reason,
                            ),
                        });
                    }
                }
            }
            ProcMatch::Ambiguous(candidates) => {
                // Multiple procs passed gate 1 (and either none passed
                // gate 3, or several did with no clear winner). Don't
                // auto-bind — list candidates so a human picks.
                let preview: Vec<String> = candidates.iter().take(5).cloned().collect();
                let extra = if candidates.len() > 5 {
                    format!(" (+ {} more)", candidates.len() - 5)
                } else {
                    String::new()
                };
                report.missing_ops_logged.push(MissingOpLog {
                    path: normalised_entry_path.clone(),
                    method: entry.method.clone(),
                    reason: format!(
                        "AMBIGUOUS — {} candidate `{}` proc(s) match entity `{}`: {}{}",
                        candidates.len(),
                        method_str.to_uppercase(),
                        entity_token,
                        preview.join(", "),
                        extra,
                    ),
                });
                // Still emit the operation — registry says it exists.
                add_op_to_spec(spec, entry, "ambiguous (see corrections.md)", standard, &mut local_norms);
            }
            ProcMatch::None => {
                // No proc agrees on verb. Story candidate.
                report.missing_ops_logged.push(MissingOpLog {
                    path: normalised_entry_path.clone(),
                    method: entry.method.clone(),
                    reason: format!(
                        "no `{}` proc found matching entity `{}` — story candidate",
                        method_str.to_uppercase(),
                        entity_token
                    ),
                });
                // Still emit the operation — registry says it exists,
                // even if no proc backs it yet.
                add_op_to_spec(spec, entry, "unbound (see corrections.md)", standard, &mut local_norms);
            }
        }
    }
    // Drain locally-collected normalisations into the report.
    report.standard_normalisations.extend(local_norms);
    Ok(())
}

/// Compute surplus ops — spec entries that don't appear in the registry.
fn catalog_surplus(
    spec: &serde_yaml::Value,
    registry: &[RegistryEntry],
    report: &mut FitReport,
) {
    let normalise = |p: &str| -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty())
            .map(|seg| {
                if seg.starts_with('{') && seg.ends_with('}') {
                    "{x}".to_string()
                } else {
                    seg.to_lowercase()
                }
            })
            .collect()
    };
    let registry_shapes: std::collections::BTreeSet<Vec<String>> = registry
        .iter()
        .map(|e| normalise(&e.path))
        .collect();
    if let Some(paths) = spec.get("paths").and_then(|v| v.as_mapping()) {
        for k in paths.keys() {
            if let Some(p) = k.as_str() {
                if p.starts_with("/internal/") {
                    continue;
                }
                if !registry_shapes.contains(&normalise(p)) {
                    report.surplus_ops.push(p.to_string());
                }
            }
        }
    }
}

/// Render a markdown changelog of every correction.
pub fn render_corrections_md(report: &FitReport, client: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Spec Corrections — {}\n\n", client));
    let registry_label = if report.registry_source.is_empty() {
        "(no registry detected)".to_string()
    } else {
        format!("`{}`", report.registry_source)
    };
    out.push_str(&format!(
        "Source: live database registry — {} ({} rows).\n\n",
        registry_label, report.registry_size
    ));
    if !report.rest_normalisations.is_empty() {
        out.push_str(&format!(
            "**{} registry path(s) carry non-REST shape** (e.g. PascalCase). \
             Spec emits the REST-normalised form. See \"REST normalisations\" below.\n\n",
            report.rest_normalisations.len(),
        ));
    }

    if !report.has_anything() {
        out.push_str("**No corrections needed.** Spec matches the database.\n");
        return out;
    }

    out.push_str(&format!(
        "**{} auto-corrections applied** (paths: {}, params: {}, missing-ops added: {}).\n\n",
        report.total_corrections(),
        report.path_rewrites.len(),
        report.param_rewrites.len(),
        report.missing_ops_added.len(),
    ));

    if !report.path_rewrites.is_empty() {
        out.push_str("## Path rewrites (registry wins)\n\n");
        out.push_str("| From | → | To | Backed by |\n|---|---|---|---|\n");
        for r in &report.path_rewrites {
            out.push_str(&format!(
                "| `{}` | → | `{}` | `{}` |\n",
                r.from, r.to, r.backed_by
            ));
        }
        out.push('\n');
    }

    if !report.param_rewrites.is_empty() {
        out.push_str("## Parameter rewrites (sys.parameters wins)\n\n");
        out.push_str("| Op | Field | From | To |\n|---|---|---|---|\n");
        for r in &report.param_rewrites {
            out.push_str(&format!(
                "| `{} {}` | `{}` | `{}` | `{}` |\n",
                r.method.to_uppercase(),
                r.op_path,
                r.field,
                r.from,
                r.to
            ));
        }
        out.push('\n');
    }

    if !report.missing_ops_added.is_empty() {
        out.push_str("## Missing-op auto-adds (Scenario A — proc found)\n\n");
        out.push_str("| Path | Method | Backed by |\n|---|---|---|\n");
        for m in &report.missing_ops_added {
            out.push_str(&format!(
                "| `{}` | `{}` | `{}` |\n",
                m.path, m.method, m.backed_by
            ));
        }
        out.push('\n');
    }

    if !report.missing_ops_logged.is_empty() {
        out.push_str("## Missing-op diagnostics (Scenarios B / C — no auto-fix)\n\n");
        out.push_str("Each row is a registry path with no deployable proc match. Either ");
        out.push_str("the registry is stale (proc was deleted) or the proc lives outside the ");
        out.push_str("catalog scope. A human decides.\n\n");
        out.push_str("| Path | Method | Reason |\n|---|---|---|\n");
        for m in &report.missing_ops_logged {
            out.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                m.path,
                m.method.as_deref().unwrap_or("?"),
                m.reason
            ));
        }
        out.push('\n');
    }

    if !report.surplus_ops.is_empty() {
        out.push_str("## Surplus ops (spec has them, registry doesn't)\n\n");
        out.push_str(&format!(
            "{} spec operations have no matching registry entry. **Left in the spec untouched ",
            report.surplus_ops.len()
        ));
        out.push_str("for v1.** Decide on the next run after seeing the corrected output.\n\n");
        for p in report.surplus_ops.iter().take(20) {
            out.push_str(&format!("- `{}`\n", p));
        }
        if report.surplus_ops.len() > 20 {
            out.push_str(&format!(
                "- ... and {} more\n",
                report.surplus_ops.len() - 20
            ));
        }
        out.push('\n');
    }

    if !report.rest_normalisations.is_empty() {
        out.push_str("## REST normalisations (live API ↔ spec)\n\n");
        out.push_str(
            "The registry uses non-REST shapes (PascalCase, no plural collections, etc.). \
             The spec emits the REST-normalised form because that's the forward-looking \
             contract devs should target. Below is the mapping — clients integrating against \
             the LIVE API today still need to call the registry path verbatim.\n\n",
        );
        out.push_str("| Registry path (live API) | Spec path (REST) |\n|---|---|\n");
        for r in report.rest_normalisations.iter().take(40) {
            out.push_str(&format!(
                "| `{}` | `{}` |\n",
                r.registry_path, r.spec_path
            ));
        }
        if report.rest_normalisations.len() > 40 {
            out.push_str(&format!(
                "_(+ {} more)_\n",
                report.rest_normalisations.len() - 40
            ));
        }
        out.push('\n');
    }

    out
}

/// Write the corrections markdown to disk next to the spec.
pub fn write_corrections(report: &FitReport, client: &str, path: &Path) -> Result<(), String> {
    let md = render_corrections_md(report, client);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent: {}", e))?;
    }
    std::fs::write(path, md).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(())
}

/// Extract ambiguous rows from the report and write a structured
/// worksheet (`ambiguities.md`). Each row has the path, method,
/// numbered candidates, and an empty `Resolution` column the operator
/// fills in. The operator's choices feed into
/// `dtcard/.forge/ambiguity-resolutions.toml` as a tiebreaker for
/// the next run, where the matcher reads the toml and binds the
/// chosen proc instead of flagging the row ambiguous.
pub fn write_ambiguities(
    report: &FitReport,
    client: &str,
    path: &Path,
) -> Result<(), String> {
    let mut rows: Vec<(String, String, Vec<String>)> = Vec::new();
    for log in &report.missing_ops_logged {
        if !log.reason.starts_with("AMBIGUOUS") {
            continue;
        }
        // Parse candidate list out of the reason string. Format:
        //   "AMBIGUOUS — N candidate `VERB` proc(s) match entity `X`: a, b, c (+ k more)"
        let candidates_part = log.reason.split_once(": ")
            .map(|(_, rest)| rest)
            .unwrap_or(&log.reason);
        // Strip "(+ k more)" suffix if present.
        let cleaned = candidates_part.split(" (+ ").next().unwrap_or(candidates_part);
        let candidates: Vec<String> = cleaned
            .split(", ")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let method = log.method.as_deref().unwrap_or("GET").to_uppercase();
        rows.push((log.path.clone(), method, candidates));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent: {}", e))?;
    }
    if rows.is_empty() {
        // Remove a stale file from a prior run.
        let _ = std::fs::remove_file(path);
        return Ok(());
    }

    let mut out = format!(
        "# Ambiguous proc bindings — {client}\n\n\
         {} endpoints have multiple candidate procs that the 6-gate matcher can't\n\
         distinguish. Pick one per row by writing the chosen proc into the\n\
         `Resolution` column, then copy the decisions into\n\
         `dtcard/.forge/ambiguity-resolutions.toml` so the next `forge docs`\n\
         run binds the chosen proc instead of flagging the row.\n\n\
         ## Resolution worksheet\n\n\
         | # | Path | Method | Candidates | Resolution (proc) |\n\
         | --- | --- | --- | --- | --- |\n",
        rows.len(),
    );
    for (i, (p, m, cands)) in rows.iter().enumerate() {
        let cands_md = cands.iter().enumerate()
            .map(|(j, c)| format!("{}. `{}`", j + 1, c))
            .collect::<Vec<_>>()
            .join("<br>");
        out.push_str(&format!(
            "| {} | `{}` | {} | {} |  |\n",
            i + 1, p, m, cands_md,
        ));
    }
    out.push_str("\n## Toml format reference\n\n");
    out.push_str(
        "Once you've chosen, add entries to\n\
         `dtcard/.forge/ambiguity-resolutions.toml` like this:\n\n\
         ```toml\n\
         [resolutions]\n\
         # path → method → fully-qualified proc\n\
         \"/account/{accountId}/transitions\" = { GET = \"accounthosting.p_txn_Get_Account_Transitions\" }\n\
         \"/cardholder/{cardholderId}/transitions\" = { GET = \"cardholder.p_txn_Get_Cardholder_Transitions\" }\n\
         ```\n\n\
         Re-run `said forge docs --client TXN --verify-against-sandbox` and\n\
         the resolved rows will move from the ambiguous bucket into bound.\n",
    );
    std::fs::write(path, out).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(())
}

/// Write `fixes.md` listing every path-param normalisation applied
/// during this run. The OpenAPI spec itself carries no rewrite
/// annotation — this file is the audit trail. Skipped (no file
/// written) when no normalisations fired.
pub fn write_fixes(report: &FitReport, client: &str, path: &Path) -> Result<(), String> {
    if report.standard_normalisations.is_empty() {
        // Remove a stale file from a prior run so the audit trail
        // reflects the current configuration. Safe to ignore errors —
        // the file may not exist.
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    let mut out = format!("# Standard normalisations — {}\n\n", client);
    out.push_str(
        "The OpenAPI standard at `.forge/openapi-standard.toml` defines\n\
         path-param casing and collection pluralisation rules. The following\n\
         rewrites were applied during this run. Source-of-truth values\n\
         (Dev Spec markdown, registry rows) are unchanged on disk; only the\n\
         emitted OpenAPI YAML is normalised.\n\n",
    );
    let mut groups: std::collections::BTreeMap<&str, Vec<&StandardNormalisation>> =
        std::collections::BTreeMap::new();
    for n in &report.standard_normalisations {
        groups.entry(n.reason.as_str()).or_default().push(n);
    }
    for (reason, items) in groups {
        out.push_str(&format!("## {}\n\n", section_heading_for_reason(reason)));
        out.push_str(&format!("Rule: `{}`\n\n", reason));
        out.push_str("| Source path | Emitted as |\n| --- | --- |\n");
        for n in items {
            out.push_str(&format!("| `{}` | `{}` |\n", n.from, n.to));
        }
        out.push('\n');
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent: {}", e))?;
    }
    std::fs::write(path, out).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(())
}

// ─────────────────────────── helpers ───────────────────────────

/// Derive a human-readable section heading from the reason key written
/// to `StandardNormalisation.reason`. Keeps `fixes.md` readable without
/// requiring the rule string to be regex-parsed in tests.
fn section_heading_for_reason(reason: &str) -> &'static str {
    if reason.starts_with("paths.collection_pluralisation") {
        "Collection pluralisation"
    } else if reason.starts_with("paths.param_casing") {
        "Path-param casing"
    } else {
        "Path normalisations"
    }
}

#[derive(Debug, Clone)]
struct ProcParam {
    name: String,
    data_type: String,
    has_default: bool,
    #[allow(dead_code)]
    ordinal: i32,
}

async fn fetch_proc_params(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
    proc_full_name: &str,
) -> Result<Vec<ProcParam>, String> {
    let (schema, name) = match proc_full_name.split_once('.') {
        Some((s, n)) => (s.to_string(), n.to_string()),
        None => ("dbo".to_string(), proc_full_name.to_string()),
    };
    let sql = format!(
        "SELECT p.name AS pname, t.name AS dtype, p.has_default_value AS hasdef, p.parameter_id AS ord \
         FROM sys.parameters p \
         INNER JOIN sys.objects o ON o.object_id = p.object_id \
         INNER JOIN sys.schemas s ON s.schema_id = o.schema_id \
         INNER JOIN sys.types   t ON t.user_type_id = p.user_type_id \
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
            let hasdef: Option<bool> = row.try_get(2).ok().flatten();
            let ord: Option<i32> = row.try_get(3).ok().flatten();
            if let (Some(n), Some(t)) = (pname, dtype) {
                params.push(ProcParam {
                    name: n.to_string(),
                    data_type: t.to_string(),
                    has_default: hasdef.unwrap_or(false),
                    ordinal: ord.unwrap_or(0),
                });
            }
        }
    }
    Ok(params)
}

/// Outcome of a proc lookup. Forces the caller to handle the
/// ambiguous case explicitly instead of silently picking a winner.
#[derive(Debug, Clone)]
pub enum ProcMatch {
    /// Single unambiguous match — auto-bind safe.
    Single(String),
    /// Multiple verb+entity matches AND/OR path-param-shape failures.
    /// Caller logs all candidates in corrections.md as a story
    /// candidate. Never silently pick.
    Ambiguous(Vec<String>),
    /// No proc passed gate 1 (verb agreement).
    None,
}

/// Find a proc match for a given entity token + HTTP method, applying
/// the gate hierarchy:
///
///   Gate 1 — verb agreement: proc name must contain an allowed verb
///            for the registry's HTTP method (Get_*, Create_*, etc.).
///   Gate 2 — single unambiguous candidate: if 2+ procs pass gate 1,
///            return Ambiguous with all candidates listed; do NOT
///            silently pick a winner.
///   Gate 3 — path-param signature must agree: each path-param in
///            the registry path must map to a proc param with a
///            matching name suffix (e.g. `/business/{businessId}` →
///            proc must have `@uBusinessId` or similar).
///
/// `path_params` is the list of path-param names from the registry
/// path, e.g. `["businessId"]` for `/business/{businessId}`. Empty
/// when the path has no parameters (gate 3 is a no-op then).
async fn find_proc_by_entity(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
    entity_token: &str,
    allowed_verbs: &[&str],
    path_params: &[String],
    full_path_segments: &[String],
    catalog: &crate::sql_catalog::SqlCatalog,
) -> Result<ProcMatch, String> {
    // Some registry paths use non-REST `/verbAndNoun` shape — e.g.
    // `/updatecardholder`, `/getallcardholderdetails`. Procs use
    // underscore-separated words (`p_txn_Update_Cardholder`), so a raw
    // substring search would miss them. Generate alternate search
    // tokens by stripping known leading verbs, then OR them into the
    // SQL filter so any proc containing any of the candidate substrings
    // surfaces. The Gate 1 verb filter still enforces correctness.
    let entity_lc = entity_token.to_lowercase();
    let mut search_tokens: Vec<String> = vec![entity_lc.clone()];
    for verb in ["getall", "get", "update", "set", "delete", "remove", "create", "add", "post", "put", "patch"] {
        if let Some(rest) = entity_lc.strip_prefix(verb) {
            if !rest.is_empty() && rest.len() >= 3 {
                search_tokens.push(rest.to_string());
            }
        }
    }
    // Compute singular fallbacks — used ONLY if the plural-form search
    // returns zero candidates. Including singulars unconditionally
    // would over-match (e.g. plural `cards` falling back to singular
    // `card` brings in every proc with "Card" in the name).
    let mut singular_fallbacks: Vec<String> = Vec::new();
    for tok in &search_tokens {
        if let Some(stem) = tok.strip_suffix("ies") {
            singular_fallbacks.push(format!("{}y", stem));
        } else if let Some(stem) = tok.strip_suffix("es") {
            if stem.len() >= 4 {
                singular_fallbacks.push(stem.to_string());
            }
        } else if let Some(stem) = tok.strip_suffix('s') {
            if stem.len() >= 4 {
                singular_fallbacks.push(stem.to_string());
            }
        }
    }
    search_tokens.sort();
    search_tokens.dedup();
    singular_fallbacks.sort();
    singular_fallbacks.dedup();
    // Match against the proc name with underscores stripped, so a
    // concatenated registry token like `cardholderdetails` finds
    // `p_txn_Get_Cardholder_Details` (squashed: `p_txngetcardholderdetails`).
    let build_sql = |toks: &[String]| -> String {
        let predicate = toks
            .iter()
            .map(|t| format!(
                "REPLACE(LOWER(o.name), '_', '') LIKE '%{}%'",
                t.replace('\'', "''")
            ))
            .collect::<Vec<_>>()
            .join(" OR ");
        format!(
            "SELECT s.name + '.' + o.name AS full_name, s.name AS schema_name, o.name AS proc_name \
             FROM sys.procedures o \
             INNER JOIN sys.schemas s ON s.schema_id = o.schema_id \
             WHERE {};",
            predicate
        )
    };

    async fn run_query(
        client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
        sql: &str,
    ) -> Result<Vec<(String, String, String)>, String> {
        let mut out: Vec<(String, String, String)> = Vec::new();
        let mut stream = client.simple_query(sql).await.map_err(|e| format!("find proc: {}", e))?;
        while let Some(item) = stream.try_next().await.map_err(|e| format!("stream: {}", e))? {
            if let QueryItem::Row(row) = item {
                let full: Option<&str> = row.try_get(0).ok().flatten();
                let schema: Option<&str> = row.try_get(1).ok().flatten();
                let proc_name: Option<&str> = row.try_get(2).ok().flatten();
                if let (Some(a), Some(b), Some(c)) = (full, schema, proc_name) {
                    out.push((a.to_string(), b.to_string(), c.to_string()));
                }
            }
        }
        Ok(out)
    }

    // First pass: plural/original tokens only. Only fall back to
    // singular forms if zero candidates surfaced — singulars over-match
    // when the plural form already had hits.
    let first_sql = build_sql(&search_tokens);
    let mut raw_candidates = run_query(client, &first_sql).await?;

    // GATE 1: verb agreement. Filter to only procs whose name
    // contains one of the allowed verb tokens. Candidates that
    // ALSO contain a different verb get penalised down later.
    let mismatched_verbs: Vec<&str> = ["get_", "create_", "update_", "delete_",
        "set_", "patch_", "remove_", "add_", "insert_"]
        .iter()
        .copied()
        .filter(|v| !allowed_verbs.contains(v))
        .collect();

    #[derive(Debug, Clone)]
    struct Scored {
        full: String,
        score: i32,
    }

    // Try Gate 1 with current candidates. If no proc passes the verb
    // filter and we have singular fallbacks, retry the SQL search
    // including singulars and re-run Gate 1. This catches cases like
    // POST `/cardholders` where the only `cardholders`-matching proc
    // is `Update_Cardholder_Status` (wrong verb) — the proc we want
    // is `Create_Cardholder` (singular).
    let mut tried_fallback = false;
    let mut gate1_passed: Vec<Scored> = Vec::new();
    'gate1: loop {
        gate1_passed.clear();
        for (full, schema, proc_name) in &raw_candidates {
        let lower = proc_name.to_lowercase();
        // Gate 1 must hit before any scoring happens.
        let has_allowed_verb = allowed_verbs.iter().any(|v| lower.contains(v));
        if !has_allowed_verb {
            continue;
        }
        let mut score: i32 = 100; // gate 1 baseline
        for mv in &mismatched_verbs {
            if lower.contains(mv) {
                score -= 50;
            }
        }
        // Entity precision: try each search token both as a bounded
        // (underscore-delimited) match in the proc name, AND as a
        // substring against the underscore-stripped form. The latter
        // catches concatenated registry tokens like `cardholderdetails`
        // mapping to `Cardholder_Details`.
        let lower_squashed = lower.replace('_', "");
        let mut entity_hit = false;
        for tok in &search_tokens {
            let bounded = format!("_{}_", tok);
            let bounded_end = format!("_{}", tok);
            let bounded_start = format!("{}_", tok);
            if lower.contains(&bounded)
                || lower.ends_with(&bounded_end)
                || lower.contains(&bounded_start)
                || lower_squashed.contains(tok.as_str())
            {
                score += 50;
                entity_hit = true;
                break;
            }
        }
        if !entity_hit {
            for tok in &search_tokens {
                let stem = if tok.ends_with('s') { &tok[..tok.len() - 1] } else { tok.as_str() };
                let stem_bounded = format!("_{}_", stem);
                let stem_end = format!("_{}", stem);
                if lower.contains(&stem_bounded)
                    || lower.ends_with(&stem_end)
                    || lower_squashed.contains(stem)
                {
                    score += 20;
                    break;
                }
            }
        }
        if lower.starts_with("p_txn_") {
            score += 10;
        }
        // Exact-tail boost: when the underscore-stripped proc name
        // ENDS with the original entity_lc (e.g. squashed
        // `p_txnupdatecardholder` ends with `updatecardholder`),
        // prefer it over candidates that contain the token in the
        // middle. Lets `/updatecardholder` PUT pick `Update_Cardholder`
        // over `Update_Cardholder_Status`.
        if lower_squashed.ends_with(entity_lc.as_str()) {
            score += 40;
        }
        if schema.eq_ignore_ascii_case(entity_token) {
            score += 5;
        }
        gate1_passed.push(Scored {
            full: full.clone(),
            score,
        });
        }
        // Gate 1 produced at least one verb-compatible candidate — done.
        if !gate1_passed.is_empty() {
            break 'gate1;
        }
        // No candidate has the right verb. Try fallback once: re-search
        // the database with singular forms included, then re-run Gate 1
        // against the expanded result set.
        if tried_fallback || singular_fallbacks.is_empty() {
            return Ok(ProcMatch::None);
        }
        tried_fallback = true;
        let mut combined = search_tokens.clone();
        combined.extend(singular_fallbacks.iter().cloned());
        combined.sort();
        combined.dedup();
        let fallback_sql = build_sql(&combined);
        raw_candidates = run_query(client, &fallback_sql).await?;
        search_tokens = combined;
        if raw_candidates.is_empty() {
            return Ok(ProcMatch::None);
        }
    }

    // GATE 3: path-param signature must agree. For each surviving
    // candidate, fetch its sys.parameters and check that EVERY
    // path-param the registry declares maps to a proc parameter
    // whose name contains that path-param's word (case-insensitive).
    //
    // Example: `/business/{businessId}` → registry path-params:
    //   ["businessId"]. Proc must have a parameter whose lowercased
    //   name contains "businessid" — e.g. @uBusinessId, @sBusinessId.
    //
    // No path-params → gate 3 is automatically pass.
    let mut gate3_passed: Vec<Scored> = Vec::new();
    for cand in &gate1_passed {
        if path_params.is_empty() {
            gate3_passed.push(cand.clone());
            continue;
        }
        let proc_params = match fetch_proc_params(client, &cand.full).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        let proc_param_names_lc: Vec<String> = proc_params
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        let all_params_present = path_params.iter().all(|pp| {
            let want = pp.to_lowercase().replace(['_', '-'], "");
            proc_param_names_lc.iter().any(|n| {
                let n_norm = n.replace(['_', '@'], "");
                n_norm.contains(&want)
            })
        });
        if all_params_present {
            gate3_passed.push(cand.clone());
        }
    }

    if gate3_passed.is_empty() {
        // gate 1 hits but no proc has the right path-param signature.
        // List the gate-1 survivors as candidates so a human sees
        // what the matcher *almost* picked.
        return Ok(ProcMatch::Ambiguous(
            gate1_passed.iter().map(|c| c.full.clone()).collect(),
        ));
    }

    // GATE 6 (early veto): when there's exactly ONE candidate after
    // gates 1+3, run schema-affinity + FK-walk against it. If the
    // proc has ZERO affinity to the URL path (its schema doesn't
    // appear anywhere in the path AND its referenced tables don't
    // either), the lone candidate is wrong — reject it entirely so
    // the registry path goes into the "story candidate" backlog
    // instead of being silently bound to a misnamed proc.
    //
    // Example: `/cards/{card_id}` GET with sole candidate
    // `cardholder.p_txn_Get_Cardholder_Cards` — proc schema is
    // `cardholder`, not `cards`, and its referenced tables live in
    // `cardholder.*`. Veto.
    if gate3_passed.len() == 1 {
        let only = &gate3_passed[0];
        if !proc_has_path_affinity(&only.full, full_path_segments, catalog) {
            return Ok(ProcMatch::Ambiguous(vec![only.full.clone()]));
        }
        return Ok(ProcMatch::Single(only.full.clone()));
    }

    // GATE 2: single unambiguous candidate. If multiple procs pass
    // gates 1 + 3, sort by score. When the top beats #2 by ≥30 points
    // it's a clear winner. When it's a near-tie, Gate 5 disambiguates
    // by path-suffix vs proc-name-suffix correlation.
    //
    // Future Gate 6 (FK relationship walk) would slot in HERE — when
    // Gates 1-5 all tie, look at the FK graph in the catalog: prefer
    // the proc whose primary table has no inbound FK from the other's
    // primary table (parent vs child resource discrimination).
    gate3_passed.sort_by_key(|c| std::cmp::Reverse(c.score));
    if gate3_passed.len() == 1 {
        return Ok(ProcMatch::Single(gate3_passed[0].full.clone()));
    }
    let top_score = gate3_passed[0].score;
    let runner_up = gate3_passed[1].score;
    if top_score - runner_up >= 30 {
        return Ok(ProcMatch::Single(gate3_passed[0].full.clone()));
    }

    // GATE 5: path-suffix correlation tiebreaker.
    //
    // The registry path tells us what resource shape we're looking at.
    // `/account/{accountId}` is the parent resource (no suffix).
    // `/account/{accountId}/balance` is a sub-resource (`balance` suffix).
    //
    // Each candidate proc name carries the same information in its
    // tokens. `Get_Account_By_Id` has no qualifying suffix. `Get_Account_
    // Balance_By_Account_Id` has `Balance` between the entity and the
    // `By_*` clause. We score candidates by how well their suffix
    // tokens correlate with the path's suffix segments:
    //
    //   path has NO suffix + proc name has NO qualifying suffix → +30
    //   path has SUFFIX X + proc name contains X token         → +30
    //   path has NO suffix + proc name HAS qualifying suffix   → -30
    //   path has SUFFIX X + proc name has DIFFERENT suffix     → -30
    //
    // After re-scoring, re-check the 30-point margin rule. If a clear
    // winner emerges, bind it. Otherwise stay Ambiguous.
    // Walk the full path segments to find what comes AFTER the entity
    // word. If the entity is the leaf, suffix is empty (parent
    // resource). Anything after = sub-resource — record those tokens.
    let path_suffix_tokens =
        path_suffix_after_entity_segs(entity_token, full_path_segments);
    for c in gate3_passed.iter_mut() {
        let proc_lower = c.full.to_lowercase();
        let proc_qualifier = proc_qualifier_tokens(&proc_lower, &entity_lc);
        let path_has_suffix = !path_suffix_tokens.is_empty();
        let proc_has_qualifier = !proc_qualifier.is_empty();
        match (path_has_suffix, proc_has_qualifier) {
            (false, false) => c.score += 30, // both bare → parent endpoint
            (false, true) => c.score -= 30,  // path bare, proc qualified → wrong proc
            (true, false) => c.score -= 30,  // path qualified, proc bare → wrong proc
            (true, true) => {
                let any_overlap = path_suffix_tokens
                    .iter()
                    .any(|p| proc_qualifier.iter().any(|q| q == p));
                if any_overlap {
                    c.score += 30;
                } else {
                    c.score -= 30;
                }
            }
        }
    }
    gate3_passed.sort_by_key(|c| std::cmp::Reverse(c.score));
    let top_score_g5 = gate3_passed[0].score;
    let runner_up_g5 = gate3_passed[1].score;
    if top_score_g5 - runner_up_g5 >= 30 {
        return Ok(ProcMatch::Single(gate3_passed[0].full.clone()));
    }

    // GATE 6: schema-affinity + FK-walk discrimination.
    //
    // When 2+ candidates still tie after Gate 5, we look at SQL
    // metadata the catalog already has:
    //
    //   6a. Schema-affinity: prefer the proc whose schema name
    //       matches one of the literal segments in the URL path.
    //       `business.p_txn_*` for `/business/...` → +25.
    //       `cardholder.p_txn_*` for `/business/...` → 0.
    //
    //   6b. FK-walk: look at each candidate proc's `referenced_tables`
    //       (the FROM/JOIN/UPDATE targets we extracted at ingest).
    //       A proc whose primary referenced table sits in the same
    //       schema as the URL's entity word is more likely the right
    //       backing. +20 per matching reference.
    //
    // Together these resolve the `/cards/{card_id}` vs
    // `cardholder.Get_Cardholder_Cards` case: schema affinity says
    // `cardholder.*` is wrong for a `/cards/...` URL.
    let path_literal_segments: Vec<String> = full_path_segments
        .iter()
        .filter(|s| s.as_str() != "{x}")
        .map(|s| s.to_lowercase())
        .collect();
    for c in gate3_passed.iter_mut() {
        let (proc_schema, _proc_name) = c.full
            .split_once('.')
            .map(|(s, n)| (s.to_lowercase(), n.to_lowercase()))
            .unwrap_or_else(|| ("dbo".to_string(), c.full.to_lowercase()));

        // 6a. Schema-affinity to URL path segments.
        let schema_in_path = path_literal_segments
            .iter()
            .any(|seg| schema_matches_segment(&proc_schema, seg));
        if schema_in_path {
            c.score += 25;
        }

        // 6b. FK-walk: pull this proc's referenced tables from the
        // catalog and check whether any of them live in a schema that
        // matches a path segment.
        let proc_obj = catalog
            .objects
            .iter()
            .find(|o| o.full_name().eq_ignore_ascii_case(&c.full));
        if let Some(obj) = proc_obj {
            let mut affinity_hits = 0usize;
            for tbl_ref in &obj.referenced_tables {
                let tbl_schema = tbl_ref
                    .split_once('.')
                    .map(|(s, _)| s.to_lowercase())
                    .unwrap_or_default();
                if tbl_schema.is_empty() {
                    continue;
                }
                if path_literal_segments
                    .iter()
                    .any(|seg| schema_matches_segment(&tbl_schema, seg))
                {
                    affinity_hits += 1;
                }
            }
            // Cap the FK contribution so a proc with many references
            // doesn't dominate by reference count alone — we want
            // SOME signal, not "longest-proc-wins."
            if affinity_hits > 0 {
                c.score += 20.min(affinity_hits as i32 * 5 + 5);
            }
        }
    }
    gate3_passed.sort_by_key(|c| std::cmp::Reverse(c.score));
    let top_score_g6 = gate3_passed[0].score;
    let runner_up_g6 = gate3_passed[1].score;
    if top_score_g6 - runner_up_g6 >= 30 {
        return Ok(ProcMatch::Single(gate3_passed[0].full.clone()));
    }

    Ok(ProcMatch::Ambiguous(
        gate3_passed.iter().map(|c| c.full.clone()).collect(),
    ))
}

/// Returns true when a proc has SOME affinity to the URL path —
/// either its schema name matches a literal segment, OR one of its
/// `referenced_tables` lives in a schema that matches a segment.
/// Used as the Gate 6 single-candidate veto.
fn proc_has_path_affinity(
    proc_full: &str,
    full_path_segments: &[String],
    catalog: &crate::sql_catalog::SqlCatalog,
) -> bool {
    let path_literal: Vec<String> = full_path_segments
        .iter()
        .filter(|s| s.as_str() != "{x}")
        .map(|s| s.to_lowercase())
        .collect();
    if path_literal.is_empty() {
        // Path is purely params (unusual) — can't reason about
        // affinity, so don't veto.
        return true;
    }
    let proc_schema = proc_full
        .split_once('.')
        .map(|(s, _)| s.to_lowercase())
        .unwrap_or_default();
    if !proc_schema.is_empty()
        && path_literal
            .iter()
            .any(|seg| schema_matches_segment(&proc_schema, seg))
    {
        return true;
    }
    let proc_obj = catalog
        .objects
        .iter()
        .find(|o| o.full_name().eq_ignore_ascii_case(proc_full));
    if let Some(obj) = proc_obj {
        for tbl_ref in &obj.referenced_tables {
            let tbl_schema = tbl_ref
                .split_once('.')
                .map(|(s, _)| s.to_lowercase())
                .unwrap_or_default();
            if !tbl_schema.is_empty()
                && path_literal
                    .iter()
                    .any(|seg| schema_matches_segment(&tbl_schema, seg))
            {
                return true;
            }
        }
    }
    false
}

/// Schema-name vs URL-segment match. Two SQL schema-naming
/// conventions need to align with English-y URL segments:
///   - `business` ↔ `business`        (exact)
///   - `business` ↔ `businesses`      (plural URL, singular schema)
///   - `accounthosting` ↔ `accounts`  (compound — schema is multi-word
///     concatenated)
/// Schema-name vs URL-segment match. English plural rules cover:
///   - same word                    `business` ↔ `business`
///   - `+s`                         `card` ↔ `cards`
///   - `+es`                        `business` ↔ `businesses`, `bin` ↔ `bines` (defensive)
///   - `+ies` from `y` ending       `category` ↔ `categories`
///   - compound schema prefix       `accounthosting` ↔ `account` / `accounts`
fn schema_matches_segment(schema: &str, segment: &str) -> bool {
    let s = schema.to_lowercase();
    let g = segment.to_lowercase();
    if s == g {
        return true;
    }
    // Plural URL ↔ singular schema. Try `+s`, `+es`, `+ies`.
    let g_singular_candidates: Vec<String> = {
        let mut out: Vec<String> = Vec::new();
        if g.ends_with("ies") && g.len() > 3 {
            out.push(format!("{}y", &g[..g.len() - 3]));
        }
        if g.ends_with("es") && g.len() > 2 {
            out.push(g[..g.len() - 2].to_string());
        }
        if g.ends_with('s') && g.len() > 1 {
            out.push(g[..g.len() - 1].to_string());
        }
        out
    };
    if g_singular_candidates.iter().any(|c| c == &s) {
        return true;
    }
    // Singular URL ↔ plural schema (rare).
    let s_singular_candidates: Vec<String> = {
        let mut out: Vec<String> = Vec::new();
        if s.ends_with("ies") && s.len() > 3 {
            out.push(format!("{}y", &s[..s.len() - 3]));
        }
        if s.ends_with("es") && s.len() > 2 {
            out.push(s[..s.len() - 2].to_string());
        }
        if s.ends_with('s') && s.len() > 1 {
            out.push(s[..s.len() - 1].to_string());
        }
        out
    };
    if s_singular_candidates.iter().any(|c| c == &g) {
        return true;
    }
    // Compound schema prefix. `accounthosting` starts with `account`,
    // so `/accounts` should match `accounthosting.*` procs.
    if s.starts_with(&g) && s.len() > g.len() {
        return true;
    }
    // And the singularised version of the URL segment if compound.
    for cand in &g_singular_candidates {
        if s.starts_with(cand) && s.len() > cand.len() {
            return true;
        }
    }
    false
}

/// Extract the path segments that come AFTER the entity word and
/// AREN'T path parameters. For `/account/{accountId}/balance`, entity
/// = "account", returns `["balance"]`. For `/account/{accountId}`,
/// returns `[]`. Used by Gate 5 to test "is the URL a parent or sub-
/// resource?".
///
/// `entity_token` is the last NON-param segment of the path (the
/// caller already determined this by walking the path in reverse).
/// `segs` is the full normalised segment list (path-params collapsed
/// to `{x}`). We find the entity's position and return everything
/// after it that isn't a path-param marker.
fn path_suffix_after_entity_segs(entity_token: &str, segs: &[String]) -> Vec<String> {
    let entity_lc = entity_token.to_lowercase();
    let pos = match segs.iter().rposition(|s| s.to_lowercase() == entity_lc) {
        Some(p) => p,
        None => return Vec::new(),
    };
    segs[pos + 1..]
        .iter()
        .filter(|s| s.as_str() != "{x}")
        .map(|s| s.to_lowercase())
        .collect()
}

/// Extract the "qualifier" tokens from a proc name — the words that
/// appear BETWEEN the entity word and the `_By_*` clause. For
/// `p_txn_API_Get_Account_Balance_By_Account_Id`, qualifier = ["balance"].
/// For `p_txn_API_Get_Account_By_Id`, qualifier = []. Used by Gate 5.
///
/// Operates on the proc name only (not the schema-qualified full
/// name) so we don't accidentally split on the entity word appearing
/// inside the SCHEMA (`accounthosting` contains `account`). Splits
/// on the FIRST word-boundary `_<entity>_` or trailing `_<entity>`
/// occurrence to find the qualifier zone.
fn proc_qualifier_tokens(proc_full: &str, entity_lc: &str) -> Vec<String> {
    // Drop the schema prefix so we never split on schema-name overlap.
    let proc_name_only = match proc_full.split_once('.') {
        Some((_, n)) => n.to_lowercase(),
        None => proc_full.to_lowercase(),
    };
    // Find the entity word at a word boundary (preceded by `_`,
    // followed by `_` or end of string). Falls back to start-of-name.
    let bounded = format!("_{}_", entity_lc);
    let bounded_end = format!("_{}", entity_lc);
    let after_offset = if let Some(pos) = proc_name_only.find(&bounded) {
        pos + bounded.len()
    } else if let Some(pos) = proc_name_only.find(&bounded_end) {
        // entity is at the end of the name → no qualifier.
        let _ = pos;
        return Vec::new();
    } else if proc_name_only.starts_with(&format!("{}_", entity_lc)) {
        entity_lc.len() + 1
    } else {
        return Vec::new();
    };
    let after = &proc_name_only[after_offset..];
    let by_pos = after.find("by_");
    let qualifier_zone = match by_pos {
        Some(pos) => &after[..pos],
        None => after,
    };
    qualifier_zone
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Gate 4 — runtime smoke EXEC against a proc to verify the binding.
///
/// Returns `Ok(())` when the proc accepted our parameter set and
/// returned either a success or a structured business-rule rejection
/// (proc parsed + ran). Returns `Err(reason)` for SQL parser errors,
/// missing dependent objects, or "wrong-shape" failures (proc
/// expected a parameter we didn't supply, etc.). Those signal a
/// proc that ISN'T actually the right backing for the registry path.
///
/// `bound_api_id` is `Some(uuid)` when the registry has a per-row
/// credential (TXN-shape `lookups.ars_Api_Rule_Settings`); the proc
/// receives that ID and bypasses the AAI gate so we exercise the
/// real proc body. `None` when the registry is documentation-only
/// (Vivere-shape) — the smoke EXEC runs without an API ID, the proc
/// either runs through (procs with no auth gate) or fails on first
/// validation. Either way, the classifier in `drain_smoke` decides
/// PASS vs FAIL based on the error class, not the response code.
async fn smoke_exec_proc(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
    proc_full_name: &str,
    bound_api_id: Option<uuid::Uuid>,
) -> Result<(), String> {
    let proc_params = fetch_proc_params(client, proc_full_name)
        .await
        .map_err(|e| format!("sys.parameters: {}", e))?;
    if proc_params.is_empty() {
        // Proc has no parameters at all — EXEC unconditionally.
        let sql = format!("EXEC {};", quote_proc(proc_full_name));
        return drain_smoke(client, &sql).await;
    }

    // Build the EXEC parameter list. Same heuristics as the contract
    // tester so we exercise the proc with realistic values.
    let request_uuid = uuid::Uuid::new_v4();
    let correlation_uuid = uuid::Uuid::new_v4();
    let body_json: String = "{}".to_string();
    let headers_json: String = "{}".to_string();

    let mut clauses: Vec<String> = Vec::new();
    for p in &proc_params {
        let name_lower = p.name.to_lowercase();
        let is_json = matches!(p.data_type.to_uppercase().as_str(), "NVARCHAR" | "VARCHAR")
            && (name_lower.contains("json")
                || name_lower.contains("request")
                || name_lower.contains("payload"));
        let is_request_id = name_lower.contains("requestid") || name_lower.ends_with("reqid");
        let is_correlation_id =
            name_lower.contains("correlationid") || name_lower.contains("corrid");
        let is_headers = name_lower.contains("header");
        let is_api_id = (name_lower == "@sapiid"
            || name_lower == "@uapiid"
            || name_lower.ends_with("apiid"))
            && p.data_type.to_uppercase() == "UNIQUEIDENTIFIER";

        if is_api_id {
            // Bind the registered ID when available; otherwise fall
            // back to the proc default (skip the param) when one
            // exists, or NIL UUID when not. Vivere procs typically
            // don't have an API ID param so this branch rarely fires
            // for that client.
            match bound_api_id {
                Some(id) => clauses.push(format!("{} = '{}'", p.name, id)),
                None => {
                    if !p.has_default {
                        clauses.push(format!(
                            "{} = '00000000-0000-0000-0000-000000000000'",
                            p.name
                        ));
                    }
                }
            }
        } else if is_request_id {
            clauses.push(format!("{} = '{}'", p.name, request_uuid));
        } else if is_correlation_id {
            clauses.push(format!("{} = '{}'", p.name, correlation_uuid));
        } else if is_headers {
            clauses.push(format!("{} = N'{}'", p.name, sql_escape(&headers_json)));
        } else if is_json {
            clauses.push(format!("{} = N'{}'", p.name, sql_escape(&body_json)));
        } else if p.has_default {
            // Let the proc use its default.
        } else if p.data_type.to_uppercase() == "UNIQUEIDENTIFIER" {
            clauses.push(format!(
                "{} = '00000000-0000-4000-8000-000000000000'",
                p.name
            ));
        } else {
            clauses.push(format!("{} = NULL", p.name));
        }
    }

    let sql = format!(
        "EXEC {} {};",
        quote_proc(proc_full_name),
        clauses.join(", ")
    );
    drain_smoke(client, &sql).await
}

fn quote_proc(full: &str) -> String {
    match full.split_once('.') {
        Some((s, n)) => format!("[{}].[{}]", s, n),
        None => format!("[{}]", full),
    }
}

fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Drain a smoke-EXEC stream. Three classes of outcome:
///
///   PASS:
///     - success (any rows or a single empty result-set)
///     - RAISERROR with `{"PrcCode":N,"PrcDesc":"..."}` payload
///       (proc ran, business-rule rejection)
///     - downstream-dependency failure (proc actually executed,
///       failed inside a transitively-called proc/table — e.g.
///       `dbo.p_dte_Audit_Backend` NULL-on-`sel_Created` bug). This
///       is evidence the proc IS reachable; binding is correct,
///       deployment has bugs.
///
///   FAIL (Gate 4 demotes to Ambiguous):
///     - SQL parser error in our generated EXEC
///     - Wrong-shape error: "Procedure expects parameter X" /
///       "Procedure or function 'X' has too many arguments"
///     - "Could not find stored procedure '<our-target>'"
///     - "Invalid object name '<our-target>'"
///
/// The distinction matters because `p_dte_Audit_Backend` is a known
/// dt-source bug that breaks every API proc — punishing a binding for
/// downstream bugs would zero out auto-binds even when they're right.
async fn drain_smoke(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
    sql: &str,
) -> Result<(), String> {
    let stream_result = client.simple_query(sql).await;
    let mut stream = match stream_result {
        Ok(s) => s,
        Err(e) => return classify_smoke_error(&e.to_string(), sql),
    };
    loop {
        match stream.try_next().await {
            Ok(Some(_item)) => continue,
            Ok(None) => return Ok(()),
            Err(e) => return classify_smoke_error(&e.to_string(), sql),
        }
    }
}

/// Decide whether a smoke-EXEC error means "wrong binding"
/// (Gate 4 fail) or "right binding, downstream broken" (Gate 4 pass).
fn classify_smoke_error(msg: &str, sql: &str) -> Result<(), String> {
    let lower = msg.to_lowercase();
    // PASS — proc-validation rejection in business-rule JSON.
    if msg.contains("\"PrcCode\"") {
        return Ok(());
    }
    // FAIL — wrong proc-signature errors that imply the binding
    // itself is structurally wrong.
    let wrong_signature_markers = [
        "expects parameter",                          // missing required param
        "expects the parameter",                      // variant
        "has too many arguments",                     // surplus args
        "incorrect syntax",                           // SQL parser
        "must be the first statement",                // GO ordering
    ];
    for marker in wrong_signature_markers {
        if lower.contains(marker) {
            return Err(format!("wrong proc signature: {}", trim_msg(msg)));
        }
    }
    // FAIL — the proc we tried to call doesn't exist.
    if lower.contains("could not find stored procedure")
        || lower.contains("invalid object name")
    {
        // BUT — only treat this as wrong-binding when the missing
        // object is the proc we tried to EXEC. If the missing
        // object is a TRANSITIVELY-called proc, that's a deployment
        // gap (Layer 5 territory), not evidence the binding is wrong.
        let executing_clue = format!("executing {}", target_proc_from_sql(sql).to_lowercase());
        if lower.contains(&executing_clue) || !lower.contains("executing") {
            return Err(format!("target proc unreachable: {}", trim_msg(msg)));
        }
        // Downstream dep missing → PASS (binding ok, deploy broken).
        return Ok(());
    }
    // Downstream-table NULL/constraint/conversion failures → PASS.
    // The proc executed and got far enough to violate something else.
    let downstream_markers = [
        "cannot insert the value null",
        "cannot insert null",
        "violation of",
        "conversion failed",
        "the conversion of",
        "string or binary data would be truncated",
        "operand data type",
        "arithmetic overflow",
        "subquery returned more than 1 value",
        "divide by zero",
        "deadlock",
    ];
    for marker in downstream_markers {
        if lower.contains(marker) {
            return Ok(());
        }
    }
    // Anything else — be conservative, fail Gate 4.
    Err(format!("EXEC failed: {}", trim_msg(msg)))
}

/// Pull the EXEC target proc name out of our generated SQL string.
/// We only call this for diagnostics, so best-effort is fine.
fn target_proc_from_sql(sql: &str) -> String {
    let upper = sql.to_uppercase();
    if let Some(pos) = upper.find("EXEC ") {
        let after = &sql[pos + 5..];
        let trimmed = after.trim_start();
        let mut out = String::new();
        for ch in trimmed.chars() {
            if ch.is_ascii_alphanumeric()
                || ch == '_'
                || ch == '.'
                || ch == '['
                || ch == ']'
            {
                out.push(ch);
            } else {
                break;
            }
        }
        return out.replace(['[', ']'], "");
    }
    String::new()
}

fn trim_msg(s: &str) -> String {
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.len() > 200 {
        format!("{}…", &one_line[..200])
    } else {
        one_line
    }
}

/// Extract `<schema>.<name>` from a description like
/// "Implemented by `cardholder.p_txn_Update_Cardholder`."
fn extract_proc_from_backref(desc: &str) -> String {
    if let Some(start) = desc.find('`') {
        let rest = &desc[start + 1..];
        if let Some(end) = rest.find('`') {
            let candidate = &rest[..end];
            if candidate.contains('.') {
                return candidate.to_string();
            }
        }
    }
    String::new()
}

/// Add a minimal op to the spec for a registry path the generator missed.
/// Uses the same shape as `build_internal_item` — sparse but valid.
///
/// Public so integration tests can verify the path-normalisation
/// pipeline (param-casing + pluralisation) on registry rows without
/// standing up a sandbox.
pub fn add_op_to_spec(
    spec: &mut serde_yaml::Value,
    entry: &RegistryEntry,
    proc_full: &str,
    standard: &crate::OpenApiStandard,
    normalisations: &mut Vec<StandardNormalisation>,
) {
    let paths_node = match spec.get_mut("paths").and_then(|v| v.as_mapping_mut()) {
        Some(m) => m,
        None => return,
    };
    let method = entry
        .method
        .as_deref()
        .unwrap_or("get")
        .to_lowercase();

    // Normalise snake_case path params to camelCase Id (BRU/OpenAPI
    // standard). `/cards/{card_id}` → `/cards/{cardId}`. Pre-existing
    // TXN seed rows ride this normalisation pass too — we don't write
    // back to the database, just present the spec in the right shape.
    // Gated by the standard: `Preserve` keeps the registry path as-is.
    let casing_normalised = match standard.paths.param_casing {
        crate::openapi_standard::ParamCasing::CamelCaseId => {
            let n = normalise_path_params_to_camel(&entry.path);
            if n != entry.path && standard.paths.log_normalisations {
                normalisations.push(StandardNormalisation {
                    from: entry.path.clone(),
                    to: n.clone(),
                    reason: "paths.param_casing = camel_case_id".into(),
                });
            }
            n
        }
        crate::openapi_standard::ParamCasing::Preserve =>
            entry.path.clone(),
    };

    // Apply collection pluralisation to registry rows too. Without
    // this, registry seed paths like `/businesses/{businessId}/transitions`
    // pass through verbatim while Dev Spec entries get singularised —
    // producing two divergent path entries for the same logical
    // endpoint. Same rule, same `pluralise_path` impl.
    let (normalised_path, plural_log) = crate::dev_spec::parser::pluralise_path(
        &method,
        &casing_normalised,
        standard,
    );
    if let Some(reason) = plural_log {
        if standard.paths.log_normalisations {
            normalisations.push(StandardNormalisation {
                from: casing_normalised.clone(),
                to: normalised_path.clone(),
                reason,
            });
        }
    }

    // Sanitise the proc-name fragment that goes into operationId.
    // Some bindings are markers like `ambiguous (see corrections.md)`
    // not real proc names — strip everything outside [A-Za-z0-9_] so
    // operationId stays a valid identifier.
    let op_id_proc = proc_full
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>();

    let mut op = serde_yaml::Mapping::new();
    op.insert(
        serde_yaml::Value::String("summary".into()),
        serde_yaml::Value::String(format!("Auto-added from registry — backed by `{}`", proc_full)),
    );
    op.insert(
        serde_yaml::Value::String("description".into()),
        serde_yaml::Value::String(format!(
            "Implemented by `{}`. Endpoint registered in `{}` and bound to this proc by the 6-gate matcher (verb + entity + path-param + smoke + suffix + schema-affinity).",
            proc_full,
            entry.source_table
        )),
    );
    op.insert(
        serde_yaml::Value::String("operationId".into()),
        serde_yaml::Value::String(format!("{}_{}", method, op_id_proc)),
    );
    op.insert(
        serde_yaml::Value::String("x-source".into()),
        serde_yaml::Value::String("registry-mapped".into()),
    );

    // Every `{name}` in the path must have a matching `parameters`
    // entry per OpenAPI 3.x. Without it, validators reject the spec
    // and tools (Swagger UI, code generators, BRU import) silently
    // treat the placeholder as a literal URL segment.
    let path_params = extract_path_param_names(&normalised_path);
    if !path_params.is_empty() {
        let mut params: Vec<serde_yaml::Value> = Vec::new();
        for pname in &path_params {
            let mut p = serde_yaml::Mapping::new();
            p.insert(serde_yaml::Value::String("name".into()),
                serde_yaml::Value::String(pname.clone()));
            p.insert(serde_yaml::Value::String("in".into()),
                serde_yaml::Value::String("path".into()));
            p.insert(serde_yaml::Value::String("required".into()),
                serde_yaml::Value::Bool(true));
            p.insert(serde_yaml::Value::String("description".into()),
                serde_yaml::Value::String(format!("Path parameter: {}", pname)));
            let mut schema = serde_yaml::Mapping::new();
            schema.insert(serde_yaml::Value::String("type".into()),
                serde_yaml::Value::String("string".into()));
            // `*Id` path params are UUIDs by convention.
            if pname.ends_with("Id") || pname.ends_with("_id") {
                schema.insert(serde_yaml::Value::String("format".into()),
                    serde_yaml::Value::String("uuid".into()));
            }
            p.insert(serde_yaml::Value::String("schema".into()),
                serde_yaml::Value::Mapping(schema));
            params.push(serde_yaml::Value::Mapping(p));
        }
        op.insert(serde_yaml::Value::String("parameters".into()),
            serde_yaml::Value::Sequence(params));
    }

    // Merge the new method into the existing path entry rather than
    // overwriting. When the path already has e.g. PUT defined, adding
    // POST should produce an entry with both — not wipe the PUT.
    let path_key = serde_yaml::Value::String(normalised_path);
    let method_key = serde_yaml::Value::String(method);
    if let Some(existing) = paths_node.get_mut(&path_key) {
        if let Some(map) = existing.as_mapping_mut() {
            map.insert(method_key, serde_yaml::Value::Mapping(op));
            return;
        }
    }
    let mut method_map = serde_yaml::Mapping::new();
    method_map.insert(method_key, serde_yaml::Value::Mapping(op));
    paths_node.insert(path_key, serde_yaml::Value::Mapping(method_map));
}

/// Convert snake_case `{x_y_z}` placeholders to camelCase `{xYZ}`.
/// `/cards/{card_id}` → `/cards/{cardId}`.
/// `/businesses/{business_id}/transitions/{transition_id}` →
///   `/businesses/{businessId}/transitions/{transitionId}`.
/// Path segments outside `{...}` are untouched.
/// Operator-supplied tiebreakers for ambiguous proc bindings.
/// Loaded from `<workspace>/.forge/ambiguity-resolutions.toml`. When
/// the matcher would otherwise flag a (path, method) ambiguous, it
/// first checks whether the operator has chosen a specific proc here.
/// If so, that proc is used directly — the row moves into the bound
/// bucket on the next run.
///
/// File shape:
///
/// ```toml
/// [resolutions]
/// "/account/{accountId}/transitions" = { GET = "accounthosting.p_txn_Get_Account_Transitions" }
/// ```
#[derive(Debug, Clone, Default)]
pub struct AmbiguityResolutions {
    by_key: std::collections::BTreeMap<(String, String), String>,
}

impl AmbiguityResolutions {
    pub fn empty() -> Self { Self::default() }

    /// Read `<workspace>/.forge/ambiguity-resolutions.toml`.
    /// Returns an empty map when the file is missing — that's the
    /// default state before the operator has resolved anything.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".forge").join("ambiguity-resolutions.toml");
        if !path.exists() {
            return Self::default();
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        let parsed: toml::Table = match toml::from_str(&text) {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };
        let mut by_key = std::collections::BTreeMap::new();
        let block = parsed.get("resolutions").and_then(|v| v.as_table());
        if let Some(b) = block {
            for (path_key, value) in b.iter() {
                if let Some(by_method) = value.as_table() {
                    for (method, proc_name) in by_method.iter() {
                        if let Some(p) = proc_name.as_str() {
                            by_key.insert(
                                (path_key.clone(), method.to_uppercase()),
                                p.to_string(),
                            );
                        }
                    }
                }
            }
        }
        Self { by_key }
    }

    pub fn lookup(&self, path: &str, method: &str) -> Option<&str> {
        self.by_key
            .get(&(path.to_string(), method.to_uppercase()))
            .map(|s| s.as_str())
    }

    pub fn len(&self) -> usize { self.by_key.len() }
}

/// Apply the workspace standard's path normalisations (param casing
/// + collection pluralisation) to a registry path. Mirrors what
/// `add_op_to_spec` does before inserting the path key, so callers
/// upstream of `add_op_to_spec` (entity-token derivation, diagnostic
/// logging) can use the same final form.
fn post_normalisation_path(
    raw: &str,
    method: &str,
    standard: &crate::OpenApiStandard,
) -> String {
    let casing_normalised = match standard.paths.param_casing {
        crate::openapi_standard::ParamCasing::CamelCaseId =>
            normalise_path_params_to_camel(raw),
        crate::openapi_standard::ParamCasing::Preserve => raw.to_string(),
    };
    let (plural_normalised, _) = crate::dev_spec::parser::pluralise_path(
        method, &casing_normalised, standard,
    );
    plural_normalised
}

fn normalise_path_params_to_camel(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for seg in path.split('/') {
        if seg.is_empty() {
            out.push('/');
            continue;
        }
        if !out.is_empty() && !out.ends_with('/') {
            out.push('/');
        } else if out.is_empty() {
            // Leading slash on the path.
            out.push('/');
        }
        if let Some(inner) = seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            // Convert snake_case → camelCase.
            let mut camel = String::new();
            let mut upper_next = false;
            for c in inner.chars() {
                if c == '_' {
                    upper_next = true;
                } else if upper_next {
                    camel.extend(c.to_uppercase());
                    upper_next = false;
                } else {
                    camel.push(c);
                }
            }
            out.push('{');
            out.push_str(&camel);
            out.push('}');
        } else {
            out.push_str(seg);
        }
    }
    // Restore trailing slash policy: input doesn't end with /, neither does output.
    if !path.ends_with('/') && out.ends_with('/') && out.len() > 1 {
        out.pop();
    }
    out
}

fn extract_path_param_names(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|seg| {
            seg.strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .map(|s| s.to_string())
        })
        .collect()
}

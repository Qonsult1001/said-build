//! Reads `bundles/<Entity>/bundle.toml`. Bundles are the sole source of
//! truth for endpoints. (Profile-level `manifest.yml` was retired
//! 2026-05-13 in favour of bundles-only — `load_profile_manifest` is
//! kept as a no-op stub for backwards compatibility.)
//!
//! Same field semantics as the Python tool — the data layer doesn't change.
//! See `dtcard/.forge/proc-framework/README.md` for the canonical contract.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A bundle declaration — one folder per business entity.
#[derive(Debug, Clone, Deserialize)]
pub struct Bundle {
    pub bundle: String,
    pub version: Option<String>,
    pub schema: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub endpoints: Vec<EndpointRow>,
    /// Per-bundle prc_Code allocation (single segment, legacy shape).
    /// Audited by sql_semantic_audit. New bundles should use the
    /// multi-segment form `prc_code_ranges` below.
    #[serde(default, rename = "prc_code_range")]
    pub prc_code_range: Option<PrcCodeRange>,
    /// Per-bundle prc_Code allocation (multi-segment, current shape).
    /// A bundle's codes live inside one or more 100-wide blocks; see
    /// standards/prc-code-conventions.md for the Option-A advisory
    /// semantics. The auditor accepts a code if it falls in ANY segment
    /// OR in the shared infra set.
    #[serde(default, rename = "prc_code_ranges")]
    pub prc_code_ranges: Vec<PrcCodeRange>,
    /// Tables this bundle owns. Audited as the allow-list for proc
    /// table-touch semantics. Backed by the TOML `[tables]` table:
    /// the renderer parses `tables.files` (a list of `tables/x.sql`
    /// paths). Auditor only needs the file stems → table names.
    #[serde(default, rename = "tables")]
    pub tables_block: Option<TablesBlock>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrcCodeRange {
    pub start: i64,
    pub end: i64,
}

impl PrcCodeRange {
    pub fn contains(&self, code: i64) -> bool {
        code >= self.start && code <= self.end
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TablesBlock {
    #[serde(default)]
    pub files: Vec<String>,
}

impl Bundle {
    /// Convenience accessor — same shape `sql_semantic_audit.rs` and
    /// future renderers want: a flat list of `tables/x.sql` strings.
    pub fn table_files(&self) -> &[String] {
        match &self.tables_block {
            Some(t) => &t.files,
            None => &[],
        }
    }

    /// Unified prc_Code segments — merges the legacy single `prc_code_range`
    /// with the new multi-segment `prc_code_ranges` array. Auditor and
    /// future tooling read through this accessor so both shapes work
    /// transparently.
    pub fn prc_code_segments(&self) -> Vec<PrcCodeRange> {
        let mut out: Vec<PrcCodeRange> = self.prc_code_ranges.clone();
        if let Some(single) = &self.prc_code_range {
            out.push(single.clone());
        }
        out
    }
}

/// One endpoint inside a bundle (or a top-level profile manifest row).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EndpointRow {
    pub id: String,
    pub shape: String,
    /// Optional in bundle rows (inherits from bundle); required at render time.
    #[serde(default)]
    pub schema: Option<String>,
    pub entity: String,
    pub verb: String,
    #[serde(default)]
    pub entity_plural: Option<String>,
    #[serde(default, alias = "http_method")]
    pub method: Option<String>,
    pub path: String,
    pub api_id: String,
    pub primary_table: String,
    pub action_type: String,
    #[serde(default)]
    pub route_params: Vec<RouteParam>,
    /// Extra non-canonical parameters that the source proc declares but
    /// the canonical shape's `parameter_signature` doesn't include. Used
    /// for procs whose authoring predates the framework standard (e.g.
    /// Business.GetBusinessTransitions has `@dFromUtc DATETIME2(7)` and
    /// `@dToUtc DATETIME2(7)` date-range filter parameters). Without
    /// listing these here, the renderer drops them and the proc body
    /// fails to compile.
    ///
    /// Each entry may carry a `default` value (e.g. `NULL` or a literal)
    /// — the renderer emits `name ty = default` when present, `name ty`
    /// when absent.
    #[serde(default)]
    pub param_extras: Vec<ParamExtra>,
    #[serde(default)]
    pub request_dto: Option<String>,
    #[serde(default)]
    pub response_dto: Option<String>,
    #[serde(default)]
    pub has_validation: bool,
    #[serde(default)]
    pub has_child_tables: bool,
    /// Optional override when the deployed name doesn't follow the
    /// `p_txn_API_{verb}_{entity}` convention. Tells renderer + auditor
    /// to use this name verbatim.
    #[serde(default)]
    pub proc_name_override: Option<String>,
    /// Optional override for the C# Query class stem when the deployed
    /// file name doesn't match the row id after the dot. Default
    /// derivation: `Account.UpdateAccount` → `UpdateAccount` →
    /// `UpdateAccountQuery.cs`. Override when reality differs — e.g.
    /// `UpdateAccountByIdQuery.cs` is deployed for `Account.UpdateAccount`.
    /// Renderer uses this to pick the output file name and the C# class
    /// name; auditor uses it to find the deployed file for diffing.
    #[serde(default)]
    pub cs_class_override: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub create_date: Option<String>,
    #[serde(default)]
    pub change_log_rows: Vec<String>,
    /// Per-profile persistence strategy. The renderer selects rows whose
    /// strategy is `thick-sp` for the requested profile; `retired` rows
    /// produce no SP.
    #[serde(default)]
    pub persistence_by_profile: Vec<PersistenceEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RouteParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// Same shape as RouteParam but with an optional `default` literal.
/// Used by `param_extras` for procs whose source carries extra params
/// (e.g. `@dFromUtc DATETIME2(7) = NULL`) that the canonical shape
/// signature doesn't include.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParamExtra {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersistenceEntry {
    pub profile: String,
    pub strategy: String,
}

/// Top-level profile manifest (the YAML file). Carries endpoints not yet
/// bundled. Once an endpoint moves into a bundle, its row here is deleted.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileManifest {
    #[serde(default)]
    pub endpoints: Vec<EndpointRow>,
}

/// Parsed `profile.toml`. Only the fields the Rust renderer/auditor use
/// today are pulled out; unknown keys are ignored so the file can grow
/// without breaking the reader.
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub profile: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub reference_paths: ProfilePaths,
    /// Build-root paths — where the SSDT `.sqlproj` and ASP.NET `.csproj`
    /// live and auto-glob every bundle's output. Renderers always write
    /// here so the projects compile every bundle together.
    #[serde(default)]
    pub generated_paths: ProfilePaths,
    /// Per-bundle authoritative-source paths under `bundles/<Bundle>/`.
    /// Renderers ALSO write to these so each bundle is a self-contained
    /// handover unit. Optional — when the block is absent or a key is
    /// empty, renderers skip the per-bundle write and only emit to
    /// `generated_paths` (Phase-1 backward compatibility).
    #[serde(default)]
    pub bundle_source_paths: BundleSourcePaths,
    /// Tables that ship with every SSDT deliverable regardless of which
    /// bundle is rendered — framework plumbing (audit/registry/validation)
    /// + shared vocabulary lookups (currency, status, product). The
    /// render-sql-deploy-set command copies these in addition to each
    /// bundle's per-bundle `[tables]` declarations.
    #[serde(default)]
    pub shared_tables: SharedTables,
    /// Constants every emitted Bruno fixture inherits — the sandbox URL,
    /// APIM subscription key, x-api-version header. Read by
    /// `forge render-bruno`. Profile-level (not per-bundle) so changing
    /// the sandbox URL once updates every bundle's fixtures on next render.
    #[serde(default)]
    pub bruno_defaults: BrunoDefaults,
}

/// Constants every emitted Bruno fixture inherits. See `profile.toml`
/// `[bruno_defaults]` block.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BrunoDefaults {
    #[serde(default)]
    pub local_base_url: Option<String>,
    #[serde(default)]
    pub apim_subscription_key: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
}

/// Profile-level shared-table declarations. Both lists are workspace-
/// relative paths under `1-ground-truth/`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SharedTables {
    /// Framework plumbing — audit pipeline, registry, validation logs.
    /// Every proc renders against these regardless of business domain.
    #[serde(default)]
    pub plumbing: Vec<String>,
    /// Shared vocabulary — lookup tables referenced across multiple
    /// bundles (currency, status, product). Not owned by any single
    /// business bundle.
    #[serde(default)]
    pub vocabulary: Vec<String>,
    /// Cross-bundle structural tables — owned by some other bundle but
    /// referenced via FK / trigger from this one's procs. Ships with
    /// the SSDT deliverable until the owning bundle is part of the
    /// standard render set. Identical content; never drifts.
    #[serde(default)]
    pub cross_bundle: Vec<String>,
}

/// Path templates inside `profile.toml`. `{schema}` is substituted at
/// resolve-time by the caller. Workspace-relative — the renderer joins
/// these against the workspace root, not the framework root.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfilePaths {
    #[serde(default)]
    pub sql_proc_root: Option<String>,
    #[serde(default)]
    pub sql_project_root: Option<String>,
    #[serde(default)]
    pub cs_query_root: Option<String>,
    #[serde(default)]
    pub cs_request_dto_root: Option<String>,
    #[serde(default)]
    pub cs_response_dto_root: Option<String>,
    #[serde(default)]
    pub cs_controller_root: Option<String>,
    #[serde(default)]
    pub cs_project_root: Option<String>,
    #[serde(default)]
    pub bruno_root: Option<String>,
    #[serde(default)]
    pub openapi_root: Option<String>,
}

/// Per-bundle authoritative-source path templates from
/// `[bundle_source_paths]` in `profile.toml`. Each template contains a
/// `{bundle}` placeholder substituted by `resolve()` at call time.
///
/// Distinct shape from `ProfilePaths` because the per-bundle layout adds
/// extras the build-root path block doesn't need:
/// - `sql_tables_root` — bundle-owned table DDL goes here (separate from
///   procs so `bundles/Account/sql/Tables/` is a clean folder).
/// - `sql_seeds_root` — bundle-emitted seed rows that are merged into the
///   shared `lookups/Data/` seed files at the build root.
/// - `cs_domain_root` — per-bundle Command/Query interfaces + concrete
///   classes (`Domain/Account/Command/AccountCommands.cs`, etc.).
///
/// Every field is `Option` so a profile can opt in incrementally — empty
/// fields mean "don't write here, only `generated_paths`".
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BundleSourcePaths {
    #[serde(default)]
    pub sql_proc_root: Option<String>,
    #[serde(default)]
    pub sql_tables_root: Option<String>,
    #[serde(default)]
    pub sql_seeds_root: Option<String>,
    #[serde(default)]
    pub cs_query_root: Option<String>,
    #[serde(default)]
    pub cs_request_dto_root: Option<String>,
    #[serde(default)]
    pub cs_response_dto_root: Option<String>,
    #[serde(default)]
    pub cs_controller_root: Option<String>,
    #[serde(default)]
    pub cs_domain_root: Option<String>,
    #[serde(default)]
    pub bruno_root: Option<String>,
    #[serde(default)]
    pub openapi_root: Option<String>,
}

impl BundleSourcePaths {
    /// `true` when at least one path is set — used by renderers to gate
    /// the per-bundle write entirely. A profile that hasn't migrated to
    /// Phase 2 will have an empty `[bundle_source_paths]` block (all
    /// `None`) and renderers will skip the bundle-source write.
    pub fn is_configured(&self) -> bool {
        self.sql_proc_root.is_some()
            || self.sql_tables_root.is_some()
            || self.sql_seeds_root.is_some()
            || self.cs_query_root.is_some()
            || self.cs_request_dto_root.is_some()
            || self.cs_response_dto_root.is_some()
            || self.cs_controller_root.is_some()
            || self.cs_domain_root.is_some()
            || self.bruno_root.is_some()
            || self.openapi_root.is_some()
    }

    /// Substitute the `{bundle}` placeholder in a single path template.
    /// Returns `None` when the template is `None` (caller's signal to
    /// skip the per-bundle write for that artefact type).
    pub fn resolve(template: Option<&str>, bundle: &str) -> Option<String> {
        template.map(|t| t.replace("{bundle}", bundle))
    }
}

/// Load `profiles/<name>/profile.toml`.
pub fn load_profile(framework_root: &Path, profile: &str) -> Result<Profile, String> {
    let path = framework_root.join("profiles").join(profile).join("profile.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    toml::from_str(&text)
        .map_err(|e| format!("parse {}: {}", path.display(), e))
}

impl EndpointRow {
    /// Resolve the deployed proc name. Override takes priority over the
    /// `p_txn_API_{verb}_{entity}` convention.
    pub fn proc_name(&self) -> String {
        if let Some(o) = &self.proc_name_override {
            return o.clone();
        }
        format!("p_txn_API_{}_{}", self.verb, self.entity)
    }

    /// Resolve the C# Query class stem (the part before `Query.cs`).
    /// Override takes priority over the default derivation, which takes
    /// the part of `row.id` after the dot — e.g. `Account.CreateAccount`
    /// → `CreateAccount`. Falls back to the full id when no dot is
    /// present (defensive — every bundled endpoint id should have one).
    ///
    /// **Scope**: this stem applies to the SQL Query adapter class
    /// (`Data/Repositories/SqlQueries/{Bundle}/{Stem}Query.cs`) — that's
    /// where the deployed convention sometimes diverges
    /// (`UpdateAccountByIdQuery.cs` for `Account.UpdateAccount`).
    /// Domain method names and Controller method names follow the
    /// row-id stem **without** override, so they stay aligned with the
    /// public API verb (`UpdateAccountAsync`, not `UpdateAccountByIdAsync`).
    /// Use `bare_class_stem()` when you want the row-id stem regardless
    /// of override.
    pub fn cs_class_stem(&self) -> String {
        if let Some(o) = &self.cs_class_override {
            return o.clone();
        }
        self.bare_class_stem()
    }

    /// Row-id stem WITHOUT honouring `cs_class_override`. Used for
    /// Domain method names, Controller method names, and anywhere the
    /// public API verb-name is wanted (which mirrors the row id, not
    /// the SQL-adapter file-naming convention).
    pub fn bare_class_stem(&self) -> String {
        match self.id.split_once('.') {
            Some((_, stem)) => stem.to_string(),
            None => self.id.clone(),
        }
    }

    /// C# bundle folder under `Data/Repositories/SqlQueries/` — the part
    /// of `row.id` before the dot (e.g. `Account.CreateAccount` →
    /// `Account`). Empty string when there's no dot.
    pub fn cs_bundle_folder(&self) -> String {
        match self.id.split_once('.') {
            Some((bundle, _)) => bundle.to_string(),
            None => String::new(),
        }
    }

    /// Deployed file path under the profile's sql_proc_root.
    pub fn deployed_path(&self, deployed_root: &Path) -> PathBuf {
        let schema = self.schema.as_deref().unwrap_or("");
        deployed_root
            .join(schema)
            .join("Stored Procedures")
            .join(format!("{}.sql", self.proc_name()))
    }

    /// True if this row participates in the given profile with strategy =
    /// "thick-sp". Used by both renderer and auditor to filter rows.
    pub fn is_thick_sp_in(&self, profile: &str) -> bool {
        self.persistence_by_profile
            .iter()
            .any(|p| p.profile == profile && p.strategy == "thick-sp")
    }

    /// True if this row is marked retired for the given profile (no SP).
    pub fn is_retired_in(&self, profile: &str) -> bool {
        self.persistence_by_profile
            .iter()
            .any(|p| p.profile == profile && p.strategy == "retired")
    }
}

/// Load a bundle from `bundles/<name>/bundle.toml`.
pub fn load_bundle(framework_root: &Path, bundle_name: &str) -> Result<Bundle, String> {
    let path = framework_root.join("bundles").join(bundle_name).join("bundle.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let mut bundle: Bundle = toml::from_str(&text)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    // Endpoints inherit the bundle's schema if they don't carry their own.
    for ep in &mut bundle.endpoints {
        if ep.schema.is_none() {
            ep.schema = Some(bundle.schema.clone());
        }
    }
    Ok(bundle)
}

/// Retired 2026-05-13: profile-level `manifest.yml` is no longer the
/// source of truth — bundles own every endpoint. Kept as a no-op stub
/// so `collect_endpoints` doesn't need a conditional, and so a stale
/// manifest.yml file (if one reappears) is silently ignored instead of
/// quietly shadowing bundle endpoints.
pub fn load_profile_manifest(_framework_root: &Path, _profile: &str) -> Result<ProfileManifest, String> {
    Ok(ProfileManifest { endpoints: Vec::new() })
}

/// Collect every endpoint a profile sees — manifest rows + all bundle rows
/// that have `thick-sp` for this profile. Used by `audit` when walking the
/// whole profile.
pub fn collect_endpoints(framework_root: &Path, profile: &str) -> Result<Vec<EndpointRow>, String> {
    let mut rows = Vec::new();

    // Profile-level manifest first (transient — rows here haven't moved into
    // a bundle yet).
    let manifest = load_profile_manifest(framework_root, profile)?;
    rows.extend(manifest.endpoints);

    // Then every bundle.
    let bundles_dir = framework_root.join("bundles");
    if !bundles_dir.exists() {
        return Ok(rows);
    }
    let mut bundle_dirs: Vec<PathBuf> = std::fs::read_dir(&bundles_dir)
        .map_err(|e| format!("read {}: {}", bundles_dir.display(), e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("bundle.toml").exists())
        .collect();
    bundle_dirs.sort();
    for d in bundle_dirs {
        let name = d.file_name().unwrap().to_string_lossy().to_string();
        match load_bundle(framework_root, &name) {
            Ok(bundle) => {
                for ep in bundle.endpoints {
                    // Skip rows the profile doesn't model OR rows marked retired.
                    if ep.is_thick_sp_in(profile) {
                        rows.push(ep);
                    }
                    // `retired` rows and rows without an entry for this profile
                    // are silently skipped — they're not defects.
                }
            }
            Err(e) => {
                eprintln!("warning: bundle {}: {}", name, e);
            }
        }
    }

    Ok(rows)
}

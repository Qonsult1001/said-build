//! Mirror just-rendered artefacts into the per-bundle source tree.
//!
//! Phase 2 dual-write helper. Every `render-*` command writes its output
//! to `_project/sql/` or `_project/cs/` (the build-root for SSDT / .csproj).
//! When the profile's `[bundle_source_paths]` block is configured, this
//! helper copies the same file into `bundles/<Bundle>/{sql,cs,bruno}/...`
//! so each bundle is a self-contained handover unit.
//!
//! Design notes:
//!   - The renderer already wrote to the build-root. This helper just
//!     copies. Same bytes; no second-source-of-truth.
//!   - When the bundle-source path is `None`, the helper is a no-op —
//!     keeps profiles forward-compatible.
//!   - Mkdir's the parent directory automatically.
//!   - Returns the bundle-source path written, or `None` when skipped,
//!     so callers can log it alongside their normal "wrote X" output.

use std::path::{Path, PathBuf};

use crate::proc_framework::manifest::BundleSourcePaths;

/// Artefact kinds the bundle mirror knows about. Each maps to one path
/// template in `BundleSourcePaths`.
#[derive(Debug, Clone)]
pub enum ArtefactKind {
    /// SQL stored procedure (`.sql` under `bundles/<B>/sql/Stored Procedures/`).
    SqlProc,
    /// SQL table DDL (`.sql` under `bundles/<B>/sql/Tables/`).
    SqlTable,
    /// SQL seed script (`.sql` under `bundles/<B>/sql/seeds/`).
    SqlSeed,
    /// C# EF Query class (`.cs` under `bundles/<B>/cs/Data/Repositories/SqlQueries/<B>/`).
    CsQuery,
    /// C# Request DTO (`.cs` under `bundles/<B>/cs/Models/Request/<B>/`).
    CsRequestDto,
    /// C# Response DTO (`.cs` under `bundles/<B>/cs/Models/Response/<B>/`).
    CsResponseDto,
    /// C# Controller (`.cs` under `bundles/<B>/cs/Controllers/V1/`).
    CsController,
    /// C# Domain Command/Query (`.cs` under `bundles/<B>/cs/Domain/<B>/...`).
    /// `subpath` is the relative path *inside* the domain root — e.g.
    /// `Command/AccountCommands.cs`.
    CsDomain { subpath: String },
    /// Bruno fixture (`.bru` under `bundles/<B>/bruno/`).
    Bruno,
    /// OpenAPI per-bundle slice (`.yaml` under `bundles/<B>/openapi/`).
    Openapi,
}

/// Resolve the bundle-source target path for one artefact. Returns
/// `None` when the profile's `[bundle_source_paths]` block doesn't have
/// a template for that artefact kind — caller skips the mirror.
///
/// `workspace_root` is the path the renderer joins all `profile.toml`
/// path templates against (typically the workspace root, e.g. the
/// repo's working dir).
///
/// `filename` is the basename to write (e.g. `CreateAccountRequest.cs`).
pub fn target_for(
    paths: &BundleSourcePaths,
    workspace_root: &Path,
    bundle: &str,
    kind: ArtefactKind,
    filename: &str,
) -> Option<PathBuf> {
    let tpl = match &kind {
        ArtefactKind::SqlProc => paths.sql_proc_root.as_deref(),
        ArtefactKind::SqlTable => paths.sql_tables_root.as_deref(),
        ArtefactKind::SqlSeed => paths.sql_seeds_root.as_deref(),
        ArtefactKind::CsQuery => paths.cs_query_root.as_deref(),
        ArtefactKind::CsRequestDto => paths.cs_request_dto_root.as_deref(),
        ArtefactKind::CsResponseDto => paths.cs_response_dto_root.as_deref(),
        ArtefactKind::CsController => paths.cs_controller_root.as_deref(),
        ArtefactKind::CsDomain { .. } => paths.cs_domain_root.as_deref(),
        ArtefactKind::Bruno => paths.bruno_root.as_deref(),
        ArtefactKind::Openapi => paths.openapi_root.as_deref(),
    }?;
    let resolved = tpl.replace("{bundle}", bundle);
    let mut p = workspace_root.join(resolved);
    if let ArtefactKind::CsDomain { subpath } = &kind {
        p = p.join(subpath);
    }
    Some(p.join(filename))
}

/// Copy `source` to its bundle-source mirror location. Returns
/// `Ok(Some(target))` when a copy happened, `Ok(None)` when the bundle
/// isn't configured for this artefact (no-op).
pub fn mirror(
    paths: &BundleSourcePaths,
    workspace_root: &Path,
    bundle: &str,
    kind: ArtefactKind,
    source: &Path,
) -> Result<Option<PathBuf>, String> {
    let filename = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("no filename in {}", source.display()))?;
    let Some(target) = target_for(paths, workspace_root, bundle, kind, filename) else {
        return Ok(None);
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::copy(source, &target)
        .map_err(|e| format!("copy {} -> {}: {}", source.display(), target.display(), e))?;
    Ok(Some(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_uses_bundle_placeholder() {
        let paths = BundleSourcePaths {
            cs_request_dto_root: Some("out/bundles/{bundle}/cs/Models/Request/{bundle}".into()),
            ..Default::default()
        };
        let target = target_for(
            &paths,
            Path::new("/work"),
            "Account",
            ArtefactKind::CsRequestDto,
            "CreateAccountRequest.cs",
        )
        .unwrap();
        let s = target.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("out/bundles/Account/cs/Models/Request/Account/CreateAccountRequest.cs"));
    }

    #[test]
    fn empty_paths_block_returns_none() {
        let paths = BundleSourcePaths::default();
        let target = target_for(
            &paths,
            Path::new("/work"),
            "Account",
            ArtefactKind::CsRequestDto,
            "X.cs",
        );
        assert!(target.is_none());
    }

    #[test]
    fn cs_domain_appends_subpath() {
        let paths = BundleSourcePaths {
            cs_domain_root: Some("out/bundles/{bundle}/cs/Domain/{bundle}".into()),
            ..Default::default()
        };
        let target = target_for(
            &paths,
            Path::new("/work"),
            "Account",
            ArtefactKind::CsDomain {
                subpath: "Command".into(),
            },
            "AccountCommands.cs",
        )
        .unwrap();
        let s = target.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("out/bundles/Account/cs/Domain/Account/Command/AccountCommands.cs"));
    }
}

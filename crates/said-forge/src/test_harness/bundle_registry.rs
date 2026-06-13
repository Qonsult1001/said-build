//! Bundle registry — the new source of truth for "what endpoints does the
//! harness walk?". Replaces the legacy `api-specification.generated.yml`
//! enumeration with a walk of every `bundles/<Name>/bundle.toml` under
//! `dtcard/.forge/proc-framework/bundles/`.
//!
//! Each `[[endpoints]]` row contributes one `BundleEndpoint`; the registry
//! also pre-computes a `(METHOD, normalised path)` → bundle lookup so the
//! harness can classify an `OperationResult` back to its owning bundle
//! after lifecycle.rs has run.
//!
//! Path normalisation uses the same logic as `lifecycle::normalise_path`
//! — replacing `{x}` placeholders with `{ID}` and stripping trailing
//! slashes. Same function, both sides of the lookup, or paths won't
//! match.

use std::collections::BTreeMap;
use std::path::Path;

use crate::test_harness::lifecycle;

/// One endpoint row from a bundle.toml file.
#[derive(Debug, Clone)]
pub struct BundleEndpoint {
    /// Bundle directory name (e.g. "Alert", "Account").
    pub bundle: String,
    /// Logical entity name (e.g. "Alerts", "Account").
    pub entity: String,
    /// Uppercase HTTP method (e.g. "GET", "POST").
    pub method: String,
    /// Raw path as authored (e.g. "/alerts", "/account/{accountId}").
    pub path: String,
    /// Endpoint id (e.g. "Alert.GetAllAlerts").
    pub id: String,
}

/// Loaded registry — list of endpoints + reverse lookup keyed by
/// `(METHOD, normalised path)`.
#[derive(Debug, Clone, Default)]
pub struct BundleRegistry {
    pub endpoints: Vec<BundleEndpoint>,
    /// `(METHOD upper, normalised path)` → bundle dir name. Normalised
    /// path replaces `{x}` placeholders with `{ID}` (matches what
    /// `lifecycle::normalise_path` produces for fixture URLs).
    pub path_to_bundle: BTreeMap<(String, String), String>,
}

/// Walk `framework_root.join("bundles")` for any subdir containing
/// a `bundle.toml`; parse the `[[endpoints]]` rows.
///
/// When `only_bundle` is set, only the bundle whose subdir name matches
/// (case-sensitive) is loaded.
pub fn load_registry(
    framework_root: &Path,
    only_bundle: Option<&str>,
) -> Result<BundleRegistry, String> {
    let bundles_dir = framework_root.join("bundles");
    if !bundles_dir.is_dir() {
        return Err(format!(
            "bundles directory missing: {}",
            bundles_dir.display()
        ));
    }

    let mut registry = BundleRegistry::default();

    let entries = std::fs::read_dir(&bundles_dir)
        .map_err(|e| format!("read_dir {}: {}", bundles_dir.display(), e))?;
    let mut subdirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        if let Some(only) = only_bundle {
            if name != only {
                continue;
            }
        }
        if path.join("bundle.toml").is_file() {
            subdirs.push(path);
        }
    }
    subdirs.sort();

    for subdir in subdirs {
        let bundle_name = subdir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let toml_path = subdir.join("bundle.toml");
        let text = std::fs::read_to_string(&toml_path)
            .map_err(|e| format!("read {}: {}", toml_path.display(), e))?;
        let parsed: toml::Value = toml::from_str(&text)
            .map_err(|e| format!("parse {}: {}", toml_path.display(), e))?;
        let Some(endpoints) = parsed
            .get("endpoints")
            .and_then(|v| v.as_array())
        else {
            // Bundle with no endpoints — nothing to register.
            continue;
        };
        for ep in endpoints {
            let Some(table) = ep.as_table() else { continue; };
            let method = table
                .get("http_method")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_uppercase();
            let path = table
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let entity = table
                .get("entity")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let id = table
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if method.is_empty() || path.is_empty() {
                continue;
            }
            // Use the same normaliser the lifecycle uses on fixture URLs.
            let (_, normalised) = lifecycle::normalise_path(&path);
            registry.path_to_bundle.insert(
                (method.clone(), normalised),
                bundle_name.clone(),
            );
            registry.endpoints.push(BundleEndpoint {
                bundle: bundle_name.clone(),
                entity,
                method,
                path,
                id,
            });
        }
    }

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `CARGO_MANIFEST_DIR` for said-forge points at `crates/said-forge`;
    /// the workspace root is two levels up.
    fn framework_root() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest_dir)
            .join("..")
            .join("..")
            .join("dtcard")
            .join(".forge")
            .join("proc-framework")
    }

    #[test]
    fn loads_alert_and_account() {
        let root = framework_root();
        let reg = load_registry(&root, None).expect("load_registry");
        let alert_count = reg
            .endpoints
            .iter()
            .filter(|e| e.bundle == "Alert")
            .count();
        let account_count = reg
            .endpoints
            .iter()
            .filter(|e| e.bundle == "Account")
            .count();
        assert_eq!(alert_count, 1, "Alert should expose 1 endpoint");
        assert_eq!(account_count, 9, "Account should expose 9 endpoints");
        let key = ("GET".to_string(), "/alerts".to_string());
        assert_eq!(
            reg.path_to_bundle.get(&key).map(|s| s.as_str()),
            Some("Alert"),
            "GET /alerts should route to Alert bundle"
        );
    }

    #[test]
    fn only_bundle_filters() {
        let root = framework_root();
        let reg = load_registry(&root, Some("Alert")).expect("load_registry");
        assert!(!reg.endpoints.is_empty(), "Alert filter yields endpoints");
        assert!(
            reg.endpoints.iter().all(|e| e.bundle == "Alert"),
            "only Alert endpoints when filtered",
        );
    }
}

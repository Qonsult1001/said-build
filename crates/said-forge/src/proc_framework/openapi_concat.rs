//! Concatenate every bundle's `openapi.yaml` slice into one project-wide
//! OpenAPI 3.0.3 spec.
//!
//! Reads:
//!   - All `bundles/<X>/openapi/openapi.yaml` under the framework
//!   - The base `info` block from any one slice (they're all equivalent)
//!
//! Emits:
//!   - `<client_root>/api-specification.generated.yml`
//!   - One project spec with unified `paths:` + `components.schemas:`.
//!     Duplicate path declarations across bundles are reported as errors
//!     (each path must belong to one bundle). `ApiError` is the only
//!     schema expected to appear in every slice — it's deduped silently.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Value};

#[derive(Debug, Clone)]
pub struct ConcatReport {
    /// Path the merged spec was written to.
    pub output_path: PathBuf,
    /// Slices read in.
    pub slice_paths: Vec<PathBuf>,
    /// Total endpoint count across all paths.
    pub endpoint_count: usize,
    /// Total schemas merged (after dedupe).
    pub schema_count: usize,
}

pub fn concat_bundles(
    deliverable_bundles_dir: &Path,
    client_label: &str,
    project_title: &str,
    project_version: &str,
    output_path: &Path,
) -> Result<ConcatReport, String> {
    if !deliverable_bundles_dir.exists() {
        return Err(format!(
            "no deliverable bundles dir at {} — run render-openapi first",
            deliverable_bundles_dir.display()
        ));
    }

    let mut slice_paths: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(deliverable_bundles_dir)
        .map_err(|e| format!("readdir {}: {}", deliverable_bundles_dir.display(), e))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.path().is_dir() {
            continue;
        }
        let candidate = entry.path().join("openapi").join("openapi.yaml");
        if candidate.exists() {
            slice_paths.push(candidate);
        }
    }
    slice_paths.sort();

    if slice_paths.is_empty() {
        return Err(format!(
            "no openapi.yaml slices under {}/*/openapi/ — run `forge render-openapi` first",
            deliverable_bundles_dir.display()
        ));
    }

    // ---- merge ----------------------------------------------------------
    let mut merged_paths = Mapping::new();
    let mut merged_schemas = Mapping::new();
    let mut endpoint_count: usize = 0;

    for slice_path in &slice_paths {
        let text = std::fs::read_to_string(slice_path)
            .map_err(|e| format!("read {}: {}", slice_path.display(), e))?;
        let doc: Value = serde_yaml::from_str(&text)
            .map_err(|e| format!("parse {}: {}", slice_path.display(), e))?;

        // paths
        if let Some(paths) = doc.get("paths").and_then(|v| v.as_mapping()) {
            for (k, v) in paths {
                let key_str = k.as_str().unwrap_or("<non-string-path>").to_string();
                if merged_paths.contains_key(k) {
                    return Err(format!(
                        "duplicate path `{}` declared in two bundle slices — \
                         each path must belong to one bundle",
                        key_str
                    ));
                }
                if let Some(verbs) = v.as_mapping() {
                    endpoint_count += verbs.len();
                }
                merged_paths.insert(k.clone(), v.clone());
            }
        }

        // components.schemas
        if let Some(schemas) = doc
            .get("components")
            .and_then(|c| c.get("schemas"))
            .and_then(|s| s.as_mapping())
        {
            for (k, v) in schemas {
                // ApiError appears in every slice — dedupe silently. For
                // anything else, a duplicate is also a real-world possibility
                // (e.g. multiple bundles referencing a shared envelope DTO);
                // first-wins for now, future audit might surface drift.
                merged_schemas.entry(k.clone()).or_insert(v.clone());
            }
        }
    }

    // ---- emit -----------------------------------------------------------
    let mut root = Mapping::new();
    root.insert(Value::from("openapi"), Value::from("3.0.3"));

    let mut info = Mapping::new();
    info.insert(Value::from("title"), Value::from(project_title));
    info.insert(Value::from("version"), Value::from(project_version));
    info.insert(
        Value::from("description"),
        Value::from(format!(
            "Project-wide OpenAPI spec for {}. Concatenated from \
             {} per-bundle slices under bundles/*/openapi/ by `forge bundle-docs`. \
             Each path belongs to exactly one bundle; schemas are deduped.",
            client_label,
            slice_paths.len()
        )),
    );
    root.insert(Value::from("info"), Value::Mapping(info));

    root.insert(Value::from("paths"), Value::Mapping(merged_paths.clone()));

    let mut components = Mapping::new();
    components.insert(
        Value::from("schemas"),
        Value::Mapping(merged_schemas.clone()),
    );
    root.insert(Value::from("components"), Value::Mapping(components));

    let yaml = serde_yaml::to_string(&Value::Mapping(root))
        .map_err(|e| format!("serialise merged spec: {}", e))?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(output_path, yaml.as_bytes())
        .map_err(|e| format!("write {}: {}", output_path.display(), e))?;

    Ok(ConcatReport {
        output_path: output_path.to_path_buf(),
        slice_paths,
        endpoint_count,
        schema_count: merged_schemas.len(),
    })
}

#[allow(dead_code)]
fn _ensure_btreemap_import_kept(_m: &BTreeMap<String, String>) {
    // The stdlib BTreeMap may be unused now but stays imported for future
    // ordering work (sorting schemas alphabetically before serialisation).
}

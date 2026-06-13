//! Reads compose-list shape files at
//! `profiles/<X>/sql/_shapes/{command,query-by-id,query-list}.toml`.
//!
//! A shape declares the parameter signature + ordered region list. Each
//! region is either:
//!   - { mode = "Fully", fragment = "<file>", vars = {...}, if = "<flag>" }
//!   - { mode = "Ignore", slot = "<name>", step_number = N, comment = "..." }
//!   - { mode = "Open" | "Close", kind = "create-procedure" | "end-procedure" }
//!   - { mode = "Open", raw = "<literal SQL line>" }

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Shape {
    pub shape: String,
    pub axis: String,
    /// SQL shapes require a parameter_signature (the renderer's
    /// `CREATE PROCEDURE` parameter list). C# shapes use Open-region
    /// emitters for their method signature, so this is optional.
    #[serde(default)]
    pub parameter_signature: String,
    #[serde(default)]
    pub regions: Vec<ShapeRegion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShapeRegion {
    pub mode: String,
    /// For Fully: filename in `sql/_shared/`. For Ignore: slot name.
    /// For Open/Close: not used (`kind` or `raw` is used instead).
    #[serde(default)]
    pub fragment: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    /// For Open/Close: the structural plumbing kind
    /// (`"create-procedure"`, `"end-procedure"`) or `raw` literal.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub raw: Option<String>,
    /// Per-region variable substitutions, e.g. `{ step_number = 3 }`.
    #[serde(default)]
    pub vars: BTreeMap<String, toml::Value>,
    /// Step number passed into Ignore stubs for the TODO comment.
    #[serde(default)]
    pub step_number: Option<i64>,
    /// One-line description shown in the empty Ignore stub.
    #[serde(default)]
    pub comment: Option<String>,
    /// Conditional flag — region emitted only if the endpoint row has this
    /// flag set to true (e.g. `if = "has_validation"`).
    #[serde(default, rename = "if")]
    pub if_flag: Option<String>,
}

pub fn load_shape(framework_root: &Path, profile: &str, shape_name: &str) -> Result<Shape, String> {
    let path = framework_root
        .join("profiles")
        .join(profile)
        .join("sql")
        .join("_shapes")
        .join(format!("{}.toml", shape_name));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    toml::from_str(&text).map_err(|e| format!("parse {}: {}", path.display(), e))
}

pub fn load_fragment(framework_root: &Path, profile: &str, fragment: &str) -> Result<String, String> {
    let path = framework_root
        .join("profiles")
        .join(profile)
        .join("sql")
        .join("_shared")
        .join(fragment);
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))
}

/// C# shape loader. Same structure as `load_shape` but reads from
/// `profiles/<X>/cs/_shapes/` — keeps the SQL and C# pipelines from
/// stepping on each other's fragments.
pub fn load_shape_cs(framework_root: &Path, profile: &str, shape_name: &str) -> Result<Shape, String> {
    let path = framework_root
        .join("profiles")
        .join(profile)
        .join("cs")
        .join("_shapes")
        .join(format!("{}.toml", shape_name));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    toml::from_str(&text).map_err(|e| format!("parse {}: {}", path.display(), e))
}

/// C# fragment loader from `profiles/<X>/cs/_shared/`.
pub fn load_fragment_cs(framework_root: &Path, profile: &str, fragment: &str) -> Result<String, String> {
    let path = framework_root
        .join("profiles")
        .join(profile)
        .join("cs")
        .join("_shared")
        .join(fragment);
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))
}

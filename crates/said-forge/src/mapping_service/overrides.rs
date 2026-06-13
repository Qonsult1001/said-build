//! Per-project mapping overrides loaded from `.forge/mapping.toml`.
//!
//! Shape:
//!
//! ```toml
//! [op."post-cardholders"]
//! tables = ["cardholder.cpf_Client_Profile", "cardholder.add_Address_Details"]
//! procs = [
//!   { name = "cardholder.p_txn_Update_Cardholder", role = "primary" },
//! ]
//!
//! [op."post-cardholders".columns]
//! last_name = "cardholder.cpf_Client_Profile.cpf_Last_Name"
//! nationality = "cardholder.add_Address_Details.col_Code"
//! ```
//!
//! Overrides are the highest-confidence tier (`Confidence::Explicit`).
//! MappingService routes every resolver through the override table
//! first; if an op has an entry, the entry wins and the heuristic is
//! skipped. That makes this file the one place a developer edits when
//! a heuristic gets it wrong — no code change required.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{ForgeError, ForgeResult};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MappingOverrides {
    #[serde(default, rename = "op")]
    ops: BTreeMap<String, OverrideOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OverrideOp {
    #[serde(default)]
    pub tables: Vec<String>,
    #[serde(default)]
    pub procs: Vec<OverrideProc>,
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideProc {
    pub name: String,
    #[serde(default)]
    pub role: String,
}

/// Alias for readability — a caller that just wants table-level overrides
/// doesn't care about the whole op block.
pub type OverrideTable = String;

impl MappingOverrides {
    pub fn for_op(&self, slug: &str) -> Option<&OverrideOp> {
        self.ops.get(slug)
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Load from a `.forge/mapping.toml` path. Missing file → empty
    /// overrides (no error — overrides are optional).
    pub fn load(path: &Path) -> ForgeResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| ForgeError::Io {
            path: path.display().to_string(),
            cause: e,
        })?;
        let parsed: Self = toml::from_str(&raw).map_err(|e| {
            ForgeError::Config(format!(
                "mapping.toml parse at {}: {}",
                path.display(),
                e
            ))
        })?;
        Ok(parsed)
    }

    /// Insert a table mapping for tests / programmatic use.
    pub fn insert_op_table(&mut self, slug: &str, table: String) {
        self.ops.entry(slug.into()).or_default().tables.push(table);
    }

    /// Insert a proc mapping for tests / programmatic use.
    pub fn insert_op_proc(&mut self, slug: &str, proc_name: String) {
        self.ops.entry(slug.into()).or_default().procs.push(OverrideProc {
            name: proc_name,
            role: "primary".into(),
        });
    }

    /// Insert a column mapping for tests / programmatic use.
    pub fn insert_op_column(&mut self, slug: &str, field: &str, target: String) {
        self.ops
            .entry(slug.into())
            .or_default()
            .columns
            .insert(field.into(), target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_file_yields_empty_overrides() {
        let p = Path::new("does-not-exist-mapping.toml");
        let o = MappingOverrides::load(p).unwrap();
        assert!(o.is_empty());
    }

    #[test]
    fn loads_op_tables_procs_and_columns_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mapping.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[op."post-cardholders"]
tables = ["cardholder.cpf_Client_Profile"]
procs = [ {{ name = "cardholder.p_txn_Update_Cardholder", role = "primary" }} ]

[op."post-cardholders".columns]
last_name = "cardholder.cpf_Client_Profile.cpf_Last_Name"
"#
        )
        .unwrap();
        let o = MappingOverrides::load(&path).unwrap();
        let op = o.for_op("post-cardholders").unwrap();
        assert_eq!(op.tables, vec!["cardholder.cpf_Client_Profile".to_string()]);
        assert_eq!(op.procs.len(), 1);
        assert_eq!(op.procs[0].name, "cardholder.p_txn_Update_Cardholder");
        assert_eq!(
            op.columns.get("last_name").map(String::as_str),
            Some("cardholder.cpf_Client_Profile.cpf_Last_Name")
        );
    }

    #[test]
    fn programmatic_insert_helpers_stack_entries() {
        let mut o = MappingOverrides::default();
        o.insert_op_table("x", "t1".into());
        o.insert_op_table("x", "t2".into());
        o.insert_op_proc("x", "p1".into());
        o.insert_op_column("x", "f1", "t1.c1".into());
        let op = o.for_op("x").unwrap();
        assert_eq!(op.tables.len(), 2);
        assert_eq!(op.procs.len(), 1);
        assert_eq!(op.columns.len(), 1);
    }

    #[test]
    fn malformed_toml_returns_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mapping.toml");
        std::fs::write(&path, "this is not toml [[[").unwrap();
        let err = MappingOverrides::load(&path).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.to_lowercase().contains("mapping.toml") || msg.contains("Config"));
    }
}

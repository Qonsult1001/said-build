//! `.forge/business.toml` — project-level business metadata used by
//! the story document emitter.
//!
//! Shape:
//!
//! ```toml
//! prepared_by = "Andruska Cronje"
//! version = "V1.0"
//! sprint = "Sprint 4"
//! sprint_start = "2024-12-12"
//! sprint_end   = "2025-12-12"
//! epic = "Cardholder Management"
//!
//! [[stakeholders]]
//! name = "Andruska Cronje"
//! position = "Product Owner"
//! department = "DT Design Team"
//!
//! [[stakeholders]]
//! name = "Jaco van Wyk"
//! position = "Scrum Master"
//! department = "DT Development Team"
//! ```
//!
//! Missing file → sensible defaults (placeholders the user replaces).
//! The loader never errors on absent file — only on malformed TOML.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::{ForgeError, ForgeResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessConfig {
    #[serde(default = "default_prepared_by")]
    pub prepared_by: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub sprint: String,
    #[serde(default)]
    pub sprint_start: String,
    #[serde(default)]
    pub sprint_end: String,
    #[serde(default = "default_epic")]
    pub epic: String,
    #[serde(default)]
    pub stakeholders: Vec<Stakeholder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stakeholder {
    pub name: String,
    pub position: String,
    #[serde(default)]
    pub department: String,
}

fn default_prepared_by() -> String {
    "TBD — set `prepared_by` in .forge/business.toml".into()
}

fn default_version() -> String {
    "V0.1".into()
}

fn default_epic() -> String {
    "TBD — set `epic` in .forge/business.toml".into()
}

impl Default for BusinessConfig {
    fn default() -> Self {
        Self {
            prepared_by: default_prepared_by(),
            version: default_version(),
            sprint: String::new(),
            sprint_start: String::new(),
            sprint_end: String::new(),
            epic: default_epic(),
            stakeholders: Vec::new(),
        }
    }
}

impl BusinessConfig {
    /// Load from `<workspace>/.forge/business.toml`. Missing file →
    /// Default placeholders so forge can run on a fresh workspace.
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
                "business.toml parse at {}: {}",
                path.display(),
                e
            ))
        })?;
        Ok(parsed)
    }

    /// True when the config still carries default placeholder values.
    /// Document emitters surface a review prompt when this is true so
    /// the user sees a clear "set these up" signal.
    pub fn is_placeholder(&self) -> bool {
        self.prepared_by.starts_with("TBD") || self.epic.starts_with("TBD")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_file_returns_default_with_placeholder_values() {
        let p = Path::new("does-not-exist-business.toml");
        let c = BusinessConfig::load(p).unwrap();
        assert!(c.is_placeholder());
        assert_eq!(c.stakeholders.len(), 0);
    }

    #[test]
    fn loads_full_config_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("business.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
prepared_by = "Andruska Cronje"
version = "V1.0"
sprint = "Sprint 4"
sprint_start = "2024-12-12"
sprint_end   = "2025-12-12"
epic = "Cardholder Management"

[[stakeholders]]
name = "Andruska Cronje"
position = "Product Owner"
department = "DT Design Team"

[[stakeholders]]
name = "Jaco van Wyk"
position = "Scrum Master"
department = "DT Development Team"
"#
        )
        .unwrap();
        let c = BusinessConfig::load(&path).unwrap();
        assert_eq!(c.prepared_by, "Andruska Cronje");
        assert_eq!(c.epic, "Cardholder Management");
        assert_eq!(c.stakeholders.len(), 2);
        assert_eq!(c.stakeholders[0].position, "Product Owner");
        assert!(!c.is_placeholder());
    }

    #[test]
    fn malformed_toml_returns_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("business.toml");
        std::fs::write(&path, "not valid toml [[[").unwrap();
        let err = BusinessConfig::load(&path).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.to_lowercase().contains("business.toml"));
    }

    #[test]
    fn stakeholder_without_department_parses_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("business.toml");
        std::fs::write(
            &path,
            r#"
prepared_by = "X"
epic = "E"
[[stakeholders]]
name = "Y"
position = "Tester"
"#,
        )
        .unwrap();
        let c = BusinessConfig::load(&path).unwrap();
        assert_eq!(c.stakeholders.len(), 1);
        assert_eq!(c.stakeholders[0].department, "");
    }
}

//! `.forge/config.toml` — the onboarding config written by `said forge plan`.
//!
//! Distinct from the existing `config::ForgeConfig` (which configures the
//! LLM and retrieval settings). This one lives **per-workspace** at
//! `<project>/.forge/config.toml` and answers five questions:
//!
//! - which file in `4-expectations/` is the directive?
//! - when primary and secondary directives disagree, who wins?
//! - how do we treat PDFs in `3-requirements/`?
//! - how do we treat code + examples in `2-progress/`?
//! - how do we treat any `.xlsx`/`.xlsm` files?
//! - should `forge run` emit the gap report before, after, or not at all?
//!
//! The `plan_complete` flag gates `forge sync` — sync refuses to run until
//! `forge plan` has been answered.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::{ForgeError, ForgeResult};

/// Top-level workspace config. Written to `.forge/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub version: u32,
    #[serde(default)]
    pub plan_complete: bool,
    #[serde(default)]
    pub directive: DirectiveConfig,
    #[serde(default = "default_folders")]
    pub folders: BTreeMap<String, FolderConfig>,
    #[serde(default)]
    pub xlsx: XlsxConfig,
    #[serde(default)]
    pub run: RunConfig,
    /// Per-file authority overrides. Keys are workspace-relative paths using
    /// forward slashes (e.g. `3-requirements/BIN Operating Models.pdf`).
    #[serde(default)]
    pub files: BTreeMap<String, FileOverride>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            version: 1,
            plan_complete: false,
            directive: DirectiveConfig::default(),
            folders: default_folders(),
            xlsx: XlsxConfig::default(),
            run: RunConfig::default(),
            files: BTreeMap::new(),
        }
    }
}

fn default_folders() -> BTreeMap<String, FolderConfig> {
    let mut m = BTreeMap::new();
    m.insert(
        "ground-truth".into(),
        FolderConfig {
            default_authority: "law".into(),
            flag_conflicts_with_ground_truth: false,
        },
    );
    m.insert(
        "progress".into(),
        FolderConfig {
            default_authority: "existing".into(),
            flag_conflicts_with_ground_truth: true,
        },
    );
    m.insert(
        "requirements".into(),
        FolderConfig {
            default_authority: "requested".into(),
            flag_conflicts_with_ground_truth: false,
        },
    );
    m.insert(
        "expectations".into(),
        FolderConfig {
            default_authority: "agreed".into(),
            flag_conflicts_with_ground_truth: false,
        },
    );
    m
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectiveConfig {
    #[serde(default)]
    pub mode: DirectiveMode,
    pub primary: Option<String>,
    pub secondary: Option<String>,
    /// Ordered authority labels — first wins on conflict. Empty = flag-all.
    #[serde(default)]
    pub authority_order: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirectiveMode {
    #[default]
    Unset,
    OpenapiOnly,
    DevPlanningOnly,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FolderConfig {
    pub default_authority: String,
    #[serde(default)]
    pub flag_conflicts_with_ground_truth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct XlsxConfig {
    #[serde(default)]
    pub mode: XlsxMode,
    #[serde(default = "default_sheet_skip")]
    pub sheet_skip_patterns: Vec<String>,
}

impl Default for XlsxConfig {
    fn default() -> Self {
        Self {
            mode: XlsxMode::default(),
            sheet_skip_patterns: default_sheet_skip(),
        }
    }
}

fn default_sheet_skip() -> Vec<String> {
    vec!["_drafts".into(), "_backup".into(), "~temp".into()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum XlsxMode {
    #[default]
    Unset,
    StoryPerRow,
    GroundingOnly,
    Preview,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunConfig {
    #[serde(default)]
    pub gap_report_phase: GapPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GapPhase {
    #[default]
    OneShot,
    GapFirst,
    StoriesOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileOverride {
    pub authority: String,
}

impl WorkspaceConfig {
    /// Default path: `<root>/.forge/config.toml`.
    pub fn default_path_for(root: &Path) -> std::path::PathBuf {
        root.join(".forge").join("config.toml")
    }

    pub fn load(path: &Path) -> ForgeResult<Self> {
        let s = std::fs::read_to_string(path).map_err(|e| ForgeError::Io {
            path: path.display().to_string(),
            cause: e,
        })?;
        toml::from_str(&s).map_err(|e| ForgeError::Parse {
            path: path.display().to_string(),
            message: format!("toml parse: {}", e),
        })
    }

    pub fn save(&self, path: &Path) -> ForgeResult<()> {
        let s = toml::to_string_pretty(self).map_err(|e| {
            ForgeError::Validation(format!("config serialise: {}", e))
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ForgeError::Io {
                path: parent.display().to_string(),
                cause: e,
            })?;
        }
        std::fs::write(path, s).map_err(|e| ForgeError::Io {
            path: path.display().to_string(),
            cause: e,
        })
    }

    /// Apply one plan-phase answer by question id + chosen option key. See
    /// `plan_cli.rs` for the canonical list of question ids.
    pub fn apply_answer(&mut self, question_id: &str, answer_key: &str) -> ForgeResult<()> {
        apply_answer(self, question_id, answer_key)
    }
}

pub fn apply_answer(
    cfg: &mut WorkspaceConfig,
    question_id: &str,
    answer_key: &str,
) -> ForgeResult<()> {
    match question_id {
        "q1_directive" => {
            cfg.directive.mode = match answer_key {
                "openapi-only" => DirectiveMode::OpenapiOnly,
                "dev-planning-only" => DirectiveMode::DevPlanningOnly,
                "both" => DirectiveMode::Both,
                "none" => DirectiveMode::Unset,
                other => {
                    return Err(ForgeError::Validation(format!(
                        "unknown q1 answer: {}",
                        other
                    )))
                }
            };
        }
        "q2_expectations_authority" => {
            cfg.directive.authority_order = match answer_key {
                "dev-planning-wins" => vec!["agreed".into(), "wishlist".into()],
                "openapi-wins" => vec!["wishlist".into(), "agreed".into()],
                "flag-all" => Vec::new(),
                other => {
                    return Err(ForgeError::Validation(format!(
                        "unknown q2 answer: {}",
                        other
                    )))
                }
            };
        }
        "q3_requirements_authority" => {
            if let Some(f) = cfg.folders.get_mut("requirements") {
                // Map UI keys to authority labels: [1] hard → requested,
                // [2] soft → soft, [3] mixed → requested (overrides live
                // in cfg.files).
                f.default_authority = match answer_key {
                    "hard" => "requested".into(),
                    "soft" => "soft".into(),
                    "mixed" => "requested".into(),
                    other => {
                        return Err(ForgeError::Validation(format!(
                            "unknown q3 answer: {}",
                            other
                        )))
                    }
                };
            }
        }
        "q4_progress_authority" => {
            if let Some(f) = cfg.folders.get_mut("progress") {
                match answer_key {
                    "reference-only" => {
                        f.default_authority = "existing".into();
                        f.flag_conflicts_with_ground_truth = false;
                    }
                    "authoritative" => {
                        f.default_authority = "existing".into();
                        f.flag_conflicts_with_ground_truth = true;
                    }
                    "split" => {
                        f.default_authority = "existing".into();
                        f.flag_conflicts_with_ground_truth = true;
                    }
                    other => {
                        return Err(ForgeError::Validation(format!(
                            "unknown q4 answer: {}",
                            other
                        )))
                    }
                }
            }
        }
        "q5_xlsx_handling" | "q5_xlsx_skip" => {
            cfg.xlsx.mode = match answer_key {
                "story-per-row" => XlsxMode::StoryPerRow,
                "grounding-only" => XlsxMode::GroundingOnly,
                "preview" => XlsxMode::Preview,
                "skip" => XlsxMode::Unset,
                other => {
                    return Err(ForgeError::Validation(format!(
                        "unknown q5 answer: {}",
                        other
                    )))
                }
            };
        }
        "q6_gap_phase" => {
            cfg.run.gap_report_phase = match answer_key {
                "gap-first" => GapPhase::GapFirst,
                "one-shot" => GapPhase::OneShot,
                "stories-only" => GapPhase::StoriesOnly,
                other => {
                    return Err(ForgeError::Validation(format!(
                        "unknown q6 answer: {}",
                        other
                    )))
                }
            };
        }
        other => {
            return Err(ForgeError::Validation(format!(
                "unknown question id: {}",
                other
            )))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_four_folders_and_version_1() {
        let cfg = WorkspaceConfig::default();
        assert_eq!(cfg.version, 1);
        assert!(!cfg.plan_complete);
        assert!(cfg.folders.contains_key("ground-truth"));
        assert!(cfg.folders.contains_key("progress"));
        assert!(cfg.folders.contains_key("requirements"));
        assert!(cfg.folders.contains_key("expectations"));
    }

    #[test]
    fn round_trips_through_toml_losslessly() {
        let mut cfg = WorkspaceConfig::default();
        cfg.plan_complete = true;
        cfg.directive.mode = DirectiveMode::Both;
        cfg.directive.primary = Some("4-expectations/api.yaml".into());
        cfg.directive.secondary = Some("4-expectations/Dev Planning/api-spec.yaml".into());
        cfg.directive.authority_order = vec!["agreed".into(), "wishlist".into()];
        cfg.xlsx.mode = XlsxMode::StoryPerRow;
        cfg.run.gap_report_phase = GapPhase::GapFirst;
        cfg.files.insert(
            "3-requirements/BIN.pdf".into(),
            FileOverride {
                authority: "soft".into(),
            },
        );
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: WorkspaceConfig = toml::from_str(&s).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn save_then_load_is_identity() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join(".forge/config.toml");
        let mut cfg = WorkspaceConfig::default();
        cfg.plan_complete = true;
        cfg.directive.mode = DirectiveMode::DevPlanningOnly;
        cfg.save(&path).unwrap();
        let loaded = WorkspaceConfig::load(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn apply_answer_q1_directive_dev_planning_only() {
        let mut cfg = WorkspaceConfig::default();
        apply_answer(&mut cfg, "q1_directive", "dev-planning-only").unwrap();
        assert_eq!(cfg.directive.mode, DirectiveMode::DevPlanningOnly);
    }

    #[test]
    fn apply_answer_q2_dev_planning_wins_sets_authority_order() {
        let mut cfg = WorkspaceConfig::default();
        apply_answer(&mut cfg, "q2_expectations_authority", "dev-planning-wins").unwrap();
        assert_eq!(cfg.directive.authority_order, vec!["agreed", "wishlist"]);
    }

    #[test]
    fn apply_answer_q3_hard_becomes_requested() {
        let mut cfg = WorkspaceConfig::default();
        apply_answer(&mut cfg, "q3_requirements_authority", "hard").unwrap();
        assert_eq!(cfg.folders["requirements"].default_authority, "requested");
    }

    #[test]
    fn apply_answer_q4_authoritative_flips_flag() {
        let mut cfg = WorkspaceConfig::default();
        apply_answer(&mut cfg, "q4_progress_authority", "authoritative").unwrap();
        assert!(cfg.folders["progress"].flag_conflicts_with_ground_truth);
    }

    #[test]
    fn apply_answer_q5_story_per_row() {
        let mut cfg = WorkspaceConfig::default();
        apply_answer(&mut cfg, "q5_xlsx_handling", "story-per-row").unwrap();
        assert_eq!(cfg.xlsx.mode, XlsxMode::StoryPerRow);
    }

    #[test]
    fn apply_answer_q6_gap_first() {
        let mut cfg = WorkspaceConfig::default();
        apply_answer(&mut cfg, "q6_gap_phase", "gap-first").unwrap();
        assert_eq!(cfg.run.gap_report_phase, GapPhase::GapFirst);
    }

    #[test]
    fn apply_answer_unknown_question_errors() {
        let mut cfg = WorkspaceConfig::default();
        let err = apply_answer(&mut cfg, "q9_nonsense", "foo").unwrap_err();
        assert!(format!("{}", err).contains("unknown question"));
    }

    #[test]
    fn apply_answer_unknown_option_errors() {
        let mut cfg = WorkspaceConfig::default();
        let err = apply_answer(&mut cfg, "q1_directive", "martian").unwrap_err();
        assert!(format!("{}", err).contains("unknown q1"));
    }
}

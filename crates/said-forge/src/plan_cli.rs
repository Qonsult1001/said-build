//! The six plan-phase questions and the interactive Q&A loop driving
//! `said forge plan`.
//!
//! The question list is **scan-driven** — we only show options for content
//! the user has actually dropped in. If the workspace has no OpenAPI file,
//! the "OpenAPI only" option doesn't appear; if no XLSX is present, Q5 is
//! auto-answered as "skip".

use serde::Serialize;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::plan_scan::{quick_is_openapi, scan_project, ProjectScan, ScannedFile};
use crate::workspace_config::{apply_answer, WorkspaceConfig};
use crate::{ForgeError, ForgeResult};

#[derive(Debug, Clone, Serialize)]
pub struct Question {
    pub id: String,
    pub prompt: String,
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestionOption {
    pub key: String,
    pub label: String,
    pub description: String,
}

/// Build the 6-question list from a workspace scan. Questions that have no
/// applicable options are replaced by a "skip" placeholder so the loop
/// always offers six entries (stable ordering across runs).
pub fn questions_for(scan: &ProjectScan) -> Vec<Question> {
    vec![
        q1_directive(scan),
        q2_expectations_authority(),
        q3_requirements_authority(),
        q4_progress_authority(),
        if scan.has_xlsx {
            q5_xlsx_handling(scan)
        } else {
            q5_placeholder()
        },
        q6_gap_phase(),
    ]
}

fn q1_directive(scan: &ProjectScan) -> Question {
    let mut options = Vec::new();
    let openapi: Vec<&ScannedFile> = scan
        .expectations
        .iter()
        .filter(|f| (f.extension == "yaml" || f.extension == "yml") && quick_is_openapi(&f.path))
        .collect();
    let md: Vec<&ScannedFile> = scan
        .expectations
        .iter()
        .filter(|f| f.extension == "md" || f.extension == "markdown")
        .collect();

    if !openapi.is_empty() {
        options.push(QuestionOption {
            key: "openapi-only".into(),
            label: format!("{} (OpenAPI)", short_name(&openapi[0].path)),
            description: "Iterate OpenAPI, use Dev Planning as grounding".into(),
        });
    }
    if !md.is_empty() {
        options.push(QuestionOption {
            key: "dev-planning-only".into(),
            label: format!("{} (Dev Planning MDs)", short_name(&md[0].path)),
            description: "Iterate Dev Planning, use OpenAPI as grounding".into(),
        });
    }
    if !openapi.is_empty() && !md.is_empty() {
        options.push(QuestionOption {
            key: "both".into(),
            label: "Both — generate two story sets + reconciliation report".into(),
            description: "Writes .forge/openapi/<slug>/ AND .forge/dev-planning/<slug>/ plus .forge/gaps.md comparing them per-operation".into(),
        });
    }
    if options.is_empty() {
        options.push(QuestionOption {
            key: "none".into(),
            label: "No directive found in 4-expectations/".into(),
            description: "Drop an OpenAPI .yaml or Dev Planning .md file and re-run `forge plan`"
                .into(),
        });
    }
    Question {
        id: "q1_directive".into(),
        prompt: "Which directive drives story generation?".into(),
        options,
    }
}

fn q2_expectations_authority() -> Question {
    Question {
        id: "q2_expectations_authority".into(),
        prompt: "When OpenAPI and Dev Planning disagree...".into(),
        options: vec![
            QuestionOption {
                key: "dev-planning-wins".into(),
                label: "Dev Planning wins (OpenAPI is wishlist)".into(),
                description: String::new(),
            },
            QuestionOption {
                key: "openapi-wins".into(),
                label: "OpenAPI wins (Dev Planning is internal notes)".into(),
                description: String::new(),
            },
            QuestionOption {
                key: "flag-all".into(),
                label: "Flag every disagreement — don't pick a winner".into(),
                description: String::new(),
            },
        ],
    }
}

fn q3_requirements_authority() -> Question {
    Question {
        id: "q3_requirements_authority".into(),
        prompt: "For each PDF/XLSM in 3-requirements/, authority is...".into(),
        options: vec![
            QuestionOption {
                key: "hard".into(),
                label: "Client-stated requirement (hard — must be addressed)".into(),
                description: String::new(),
            },
            QuestionOption {
                key: "soft".into(),
                label: "Background context (soft — use for grounding only)".into(),
                description: String::new(),
            },
            QuestionOption {
                key: "mixed".into(),
                label: "Mixed — let me tag per file later in config.toml".into(),
                description: "Default to hard; edit .forge/config.toml to override per file"
                    .into(),
            },
        ],
    }
}

fn q4_progress_authority() -> Question {
    Question {
        id: "q4_progress_authority".into(),
        prompt: "2-progress/ contains existing code + reference examples. These are...".into(),
        options: vec![
            QuestionOption {
                key: "reference-only".into(),
                label: "Reference only".into(),
                description: "Don't emit 'matches existing pattern' language in stories".into(),
            },
            QuestionOption {
                key: "authoritative".into(),
                label: "Authoritative implementation".into(),
                description: "Flag when ground-truth SQL disagrees with existing code".into(),
            },
            QuestionOption {
                key: "split".into(),
                label: "Authoritative for skills/examples, reference for code files".into(),
                description: String::new(),
            },
        ],
    }
}

fn q5_xlsx_handling(scan: &ProjectScan) -> Question {
    let names: Vec<String> = scan
        .requirements
        .iter()
        .filter(|f| f.extension == "xlsx" || f.extension == "xlsm")
        .map(|f| short_name(&f.path))
        .collect();
    Question {
        id: "q5_xlsx_handling".into(),
        prompt: format!("Found XLSX/XLSM: {}. How to treat it?", names.join(", ")),
        options: vec![
            QuestionOption {
                key: "story-per-row".into(),
                label: "One story per row (structured — ingests as TableRow stories)".into(),
                description: String::new(),
            },
            QuestionOption {
                key: "grounding-only".into(),
                label: "Grounding only (searchable frames, no stories generated)".into(),
                description: String::new(),
            },
            QuestionOption {
                key: "preview".into(),
                label: "Preview first rows then decide on next run".into(),
                description: String::new(),
            },
        ],
    }
}

fn q5_placeholder() -> Question {
    Question {
        id: "q5_xlsx_skip".into(),
        prompt: "(no XLSX/XLSM files detected — Q5 skipped)".into(),
        options: vec![QuestionOption {
            key: "skip".into(),
            label: "Skip".into(),
            description: "No action; Q5 will re-prompt if you add an XLSX later".into(),
        }],
    }
}

fn q6_gap_phase() -> Question {
    Question {
        id: "q6_gap_phase".into(),
        prompt: "Before generating stories, should forge produce...".into(),
        options: vec![
            QuestionOption {
                key: "gap-first".into(),
                label: "Gap report first — approve, THEN stories".into(),
                description: "Halts after .forge/gaps.md; set approved=true to continue".into(),
            },
            QuestionOption {
                key: "one-shot".into(),
                label: "Stories + gap report together".into(),
                description: "Generates everything in one pass".into(),
            },
            QuestionOption {
                key: "stories-only".into(),
                label: "Stories only (skip gap report)".into(),
                description: String::new(),
            },
        ],
    }
}

fn short_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.display().to_string())
}

/// Programmatic driver — used by MCP + tests. Takes a list of
/// `(question_id, answer_key)` pairs; missing answers error.
pub fn apply_answers_noninteractive(
    scan: &ProjectScan,
    answers: &[(String, String)],
) -> ForgeResult<WorkspaceConfig> {
    let mut cfg = WorkspaceConfig::default();
    let qs = questions_for(scan);
    for q in &qs {
        let (_, answer) = answers
            .iter()
            .find(|(id, _)| id == &q.id)
            .ok_or_else(|| {
                ForgeError::Validation(format!("no answer supplied for question {}", q.id))
            })?;
        apply_answer(&mut cfg, &q.id, answer)?;
    }
    // Remember the primary/secondary file paths when directive mode fixes them.
    populate_directive_paths(&mut cfg, scan);
    cfg.plan_complete = true;
    Ok(cfg)
}

fn populate_directive_paths(cfg: &mut WorkspaceConfig, scan: &ProjectScan) {
    use crate::workspace_config::DirectiveMode;
    let openapi_path = scan
        .expectations
        .iter()
        .find(|f| (f.extension == "yaml" || f.extension == "yml") && quick_is_openapi(&f.path))
        .map(|f| workspace_rel(&scan.root, &f.path));
    let md_path = scan
        .expectations
        .iter()
        .find(|f| f.extension == "md" || f.extension == "markdown")
        .map(|f| workspace_rel(&scan.root, &f.path));
    match cfg.directive.mode {
        DirectiveMode::OpenapiOnly => {
            cfg.directive.primary = openapi_path.clone();
            cfg.directive.secondary = md_path;
        }
        DirectiveMode::DevPlanningOnly => {
            cfg.directive.primary = md_path.clone();
            cfg.directive.secondary = openapi_path;
        }
        DirectiveMode::Both => {
            cfg.directive.primary = md_path.clone();
            cfg.directive.secondary = openapi_path;
        }
        DirectiveMode::Unset => {}
    }
}

fn workspace_rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| p.display().to_string())
}

/// Interactive terminal loop. Reads answers from stdin, prompts on stdout.
/// Writes `.forge/config.toml` on approval.
pub fn run_interactive_plan(root: &Path, reconfigure: bool) -> ForgeResult<WorkspaceConfig> {
    let scan = scan_project(root)?;
    print_scan_summary(&scan);

    let qs = questions_for(&scan);
    let config_path = WorkspaceConfig::default_path_for(root);
    let mut cfg = if reconfigure && config_path.exists() {
        WorkspaceConfig::load(&config_path).unwrap_or_default()
    } else {
        WorkspaceConfig::default()
    };

    for (i, q) in qs.iter().enumerate() {
        println!();
        println!("Question {}/{} — {}", i + 1, qs.len(), q.prompt);
        for (idx, opt) in q.options.iter().enumerate() {
            println!("  [{}] {}", idx + 1, opt.label);
            if !opt.description.is_empty() {
                println!("      {}", opt.description);
            }
        }
        // Single-option questions auto-answer without prompting.
        if q.options.len() == 1 {
            println!("> 1 (auto — only option)");
            apply_answer(&mut cfg, &q.id, &q.options[0].key)?;
            continue;
        }
        print!("> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| ForgeError::Io {
                path: "<stdin>".into(),
                cause: e,
            })?;
        let trimmed = line.trim();
        let idx: usize = trimmed.parse().map_err(|_| {
            ForgeError::Validation(format!(
                "expected a number in 1..={}, got: {:?}",
                q.options.len(),
                trimmed
            ))
        })?;
        if idx == 0 || idx > q.options.len() {
            return Err(ForgeError::Validation(format!(
                "choice {} out of range (1..={})",
                idx,
                q.options.len()
            )));
        }
        let chosen = &q.options[idx - 1];
        apply_answer(&mut cfg, &q.id, &chosen.key)?;
    }

    populate_directive_paths(&mut cfg, &scan);

    println!();
    println!("── Summary ──");
    println!("  Directive:         {:?}", cfg.directive.mode);
    if let Some(p) = &cfg.directive.primary {
        println!("  Primary file:      {}", p);
    }
    if let Some(s) = &cfg.directive.secondary {
        println!("  Secondary file:    {}", s);
    }
    println!("  Authority order:   {:?}", cfg.directive.authority_order);
    println!(
        "  Requirements auth: {}",
        cfg.folders.get("requirements").map(|f| f.default_authority.as_str()).unwrap_or("?")
    );
    println!(
        "  Progress auth:     {} (flag conflicts: {})",
        cfg.folders.get("progress").map(|f| f.default_authority.as_str()).unwrap_or("?"),
        cfg.folders.get("progress").map(|f| f.flag_conflicts_with_ground_truth).unwrap_or(false)
    );
    println!("  XLSX mode:         {:?}", cfg.xlsx.mode);
    println!("  Gap phase:         {:?}", cfg.run.gap_report_phase);
    println!();
    print!("Approve and write .forge/config.toml? [y/N] ");
    io::stdout().flush().ok();
    let mut approval = String::new();
    io::stdin().lock().read_line(&mut approval).ok();
    if !approval.trim().eq_ignore_ascii_case("y") {
        return Err(ForgeError::Validation("plan not approved".into()));
    }
    cfg.plan_complete = true;
    cfg.save(&config_path)?;
    println!("✓ Wrote {}", config_path.display());
    Ok(cfg)
}

fn print_scan_summary(scan: &ProjectScan) {
    println!("Scanning {}...", scan.root.display());
    println!("  1-ground-truth/  {} files", scan.ground_truth.len());
    println!("  2-progress/      {} files", scan.progress.len());
    println!(
        "  3-requirements/  {} files ({})",
        scan.requirements.len(),
        if scan.has_xlsx { "XLSX present" } else { "no XLSX" }
    );
    println!(
        "  4-expectations/  {} files ({}, {})",
        scan.expectations.len(),
        if scan.has_openapi { "OpenAPI present" } else { "no OpenAPI" },
        if scan.has_dev_planning_md { "Dev Planning MDs" } else { "no Dev Planning MDs" }
    );
    if !scan.orphan_files.is_empty() {
        println!(
            "  ⚠ {} orphan file(s) at project root (won't be ingested)",
            scan.orphan_files.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{apply_scaffold, ApplyMode};

    fn workspace_with_files(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        apply_scaffold("w", &root, ApplyMode::Merge).unwrap();
        for (rel, content) in files {
            let p = root.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, content).unwrap();
        }
        (tmp, root)
    }

    #[test]
    fn questions_for_empty_workspace_returns_six_with_q1_noop_and_q5_skip() {
        let (_t, root) = workspace_with_files(&[]);
        let scan = scan_project(&root).unwrap();
        let qs = questions_for(&scan);
        assert_eq!(qs.len(), 6);
        assert_eq!(qs[0].id, "q1_directive");
        assert_eq!(qs[0].options[0].key, "none");
        assert_eq!(qs[4].id, "q5_xlsx_skip");
    }

    #[test]
    fn questions_for_openapi_plus_md_lists_three_q1_options() {
        let (_t, root) = workspace_with_files(&[
            ("4-expectations/api.yaml", "openapi: 3.0.0\ninfo: {}\n"),
            ("4-expectations/OVERVIEW.md", "# x"),
        ]);
        let scan = scan_project(&root).unwrap();
        let qs = questions_for(&scan);
        let q1_keys: Vec<&str> = qs[0].options.iter().map(|o| o.key.as_str()).collect();
        assert!(q1_keys.contains(&"openapi-only"));
        assert!(q1_keys.contains(&"dev-planning-only"));
        assert!(q1_keys.contains(&"both"));
    }

    #[test]
    fn q5_appears_when_xlsm_present() {
        let (_t, root) = workspace_with_files(&[("3-requirements/reqs.xlsm", "binary")]);
        let scan = scan_project(&root).unwrap();
        let qs = questions_for(&scan);
        assert_eq!(qs[4].id, "q5_xlsx_handling");
        assert!(qs[4].options.iter().any(|o| o.key == "story-per-row"));
    }

    #[test]
    fn non_interactive_end_to_end_produces_approved_config() {
        let (_t, root) = workspace_with_files(&[
            ("4-expectations/api.yaml", "openapi: 3.0.0\ninfo: {}\n"),
            ("4-expectations/Dev Planning/spec.md", "# dev plan"),
            ("3-requirements/reqs.xlsm", "binary"),
        ]);
        let scan = scan_project(&root).unwrap();
        let cfg = apply_answers_noninteractive(
            &scan,
            &[
                ("q1_directive".into(), "dev-planning-only".into()),
                ("q2_expectations_authority".into(), "dev-planning-wins".into()),
                ("q3_requirements_authority".into(), "hard".into()),
                ("q4_progress_authority".into(), "reference-only".into()),
                ("q5_xlsx_handling".into(), "story-per-row".into()),
                ("q6_gap_phase".into(), "gap-first".into()),
            ],
        )
        .unwrap();
        assert!(cfg.plan_complete);
        use crate::workspace_config::{DirectiveMode, GapPhase, XlsxMode};
        assert_eq!(cfg.directive.mode, DirectiveMode::DevPlanningOnly);
        assert_eq!(cfg.xlsx.mode, XlsxMode::StoryPerRow);
        assert_eq!(cfg.run.gap_report_phase, GapPhase::GapFirst);
        // Directive paths should have been populated from the scan.
        assert!(cfg.directive.primary.as_deref().unwrap().contains("spec.md"));
        assert!(cfg.directive.secondary.as_deref().unwrap().contains("api.yaml"));
    }

    #[test]
    fn non_interactive_missing_answer_errors() {
        let (_t, root) = workspace_with_files(&[]);
        let scan = scan_project(&root).unwrap();
        let err = apply_answers_noninteractive(
            &scan,
            &[("q1_directive".into(), "none".into())],
        )
        .unwrap_err();
        assert!(format!("{}", err).contains("no answer supplied"));
    }

    #[test]
    fn non_interactive_empty_workspace_with_q1_none_ok() {
        let (_t, root) = workspace_with_files(&[]);
        let scan = scan_project(&root).unwrap();
        let cfg = apply_answers_noninteractive(
            &scan,
            &[
                ("q1_directive".into(), "none".into()),
                ("q2_expectations_authority".into(), "flag-all".into()),
                ("q3_requirements_authority".into(), "hard".into()),
                ("q4_progress_authority".into(), "reference-only".into()),
                ("q5_xlsx_skip".into(), "skip".into()),
                ("q6_gap_phase".into(), "one-shot".into()),
            ],
        )
        .unwrap();
        assert!(cfg.plan_complete);
    }
}

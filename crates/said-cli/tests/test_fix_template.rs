//! `said fix-template` prints the 10-section iteration template an agent fills for
//! `learn-fix --note-file`. The learn-fix help references this command; before this it
//! did not exist (a doc↔code gap). Behavior test via the public CLI only.
use std::process::Command;

const SAID: &str = env!("CARGO_BIN_EXE_said");

#[test]
fn fix_template_prints_the_ten_section_template() {
    let out = Command::new(SAID).args(["fix-template"]).output().expect("run fix-template");
    assert!(out.status.success(), "fix-template should exit 0; stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    // The 10 section headers the orchestrator's ITERATION_TEMPLATE carries — the structure
    // that recalls well (not a one-line label). Agent fills these, passes via --note-file.
    for header in [
        "# Title",
        "# Current State",
        "# Task",
        "# Files and Functions",
        "# Workflow",
        "# Errors and Corrections",
        "# Learnings",
        "# Key Results",
        "# Worklog",
    ] {
        assert!(s.contains(header), "fix-template output missing section `{header}`\n--- got ---\n{s}");
    }
}

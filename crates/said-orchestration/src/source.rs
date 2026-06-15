//! Real-source surfacing — Claude Code's "Read before Edit" rule, applied via
//! the orchestrator's context instead of a tool call.
//!
//! Claude prevents anchor hallucination with a hard gate: the model MUST have
//! Read a file before it can Edit it, and the edit's `old_string` must match the
//! real file exactly (or the edit errors). Our apply.rs already enforces the
//! exact-match half (a hallucinated anchor fails `resolve_text_anchor`). This
//! module supplies the OTHER half: inject the actual file content into the
//! code/repair prompt so the model anchors on REAL lines, not invented ones.
//!
//! Files are rendered with 1-based line numbers (like Claude's Read output) so
//! the model copies anchors verbatim. Large files are capped to keep the prompt
//! affordable — we keep the head and tail (where edits usually land).

use std::path::Path;

/// Max chars of a single file to inject (keep head+tail when over).
const MAX_FILE_CHARS: usize = 12_000;

/// Render the relevant source files as a context block for the code/repair phase.
/// `repo_root` is the project root; `files` are repo-relative paths the task is
/// likely to touch. Missing files are skipped (a new-project task may have none).
///
/// In addition to the numbered source, this surfaces a VETTED ANCHOR MENU per
/// file (Advisory's RealEndpointAnchors lesson): a list of safe, COMPLETE
/// statement-ending lines the model should pick `anchor` from — so it copies a
/// real line instead of reconstructing (and truncating/splitting) one. This is
/// what makes anchor selection reliable across models, not just careful ones.
pub fn source_context(repo_root: &str, files: &[String]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("# Current source (READ THESE before editing — copy anchors VERBATIM from here)\n");
    let mut any = false;
    for rel in files {
        let path = Path::new(repo_root).join(rel);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue, // file doesn't exist yet (e.g. new file) — skip
        };
        any = true;
        out.push_str(&format!("\n## {}\n```\n{}\n```\n", rel, with_line_numbers(&content)));
        let menu = safe_anchor_menu(&content);
        if !menu.is_empty() {
            out.push_str(&format!(
                "\n### SAFE anchor lines for {} (use insert-after-text with ONE of these EXACT lines — they end a complete statement):\n{}\n",
                rel, menu
            ));
        }
    }
    if !any {
        return String::new();
    }
    out
}

/// A line is a SAFE insert-after anchor if it ENDS a complete statement/block
/// (ends in `;`, `}`, or `{`) — inserting after it never splits a multi-line
/// statement. (Mirrors apply.rs::ends_statement.)
fn is_safe_anchor(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.len() < 4 {
        return false;
    }
    t.ends_with(';') || t.ends_with('}') || t.ends_with('{') || t.ends_with("*/")
}

/// Build a numbered menu of safe, unique anchor lines from the file. Caps the
/// count so the prompt stays affordable; prefers later lines (where appends
/// usually go) and deduplicates.
fn safe_anchor_menu(content: &str) -> String {
    const MAX: usize = 30;
    let mut seen = std::collections::HashSet::new();
    let mut safe: Vec<String> = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if is_safe_anchor(t) && seen.insert(t.to_string()) {
            safe.push(t.to_string());
        }
    }
    // Keep the LAST MAX (appends/new endpoints land near the end).
    let start = safe.len().saturating_sub(MAX);
    safe[start..]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("  {}. {}", i + 1, l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Number lines 1-based (like Claude's Read), capping huge files head+tail.
fn with_line_numbers(content: &str) -> String {
    let numbered: Vec<String> = content
        .lines()
        .enumerate()
        .map(|(i, l)| format!("{:>5}\t{}", i + 1, l))
        .collect();
    let joined = numbered.join("\n");
    if joined.len() <= MAX_FILE_CHARS {
        return joined;
    }
    // Keep head + tail (edits usually land near declarations or the end).
    let half = MAX_FILE_CHARS / 2;
    let head: String = joined.chars().take(half).collect();
    let tail: String = joined.chars().skip(joined.len().saturating_sub(half)).collect();
    format!("{}\n   …\t(file truncated — {} lines total; head+tail shown)\n{}",
        head, numbered.len(), tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_files_yields_empty() {
        assert!(source_context("/x", &[]).is_empty());
    }

    #[test]
    fn missing_files_yield_empty() {
        assert!(source_context("/nonexistent", &["nope.rs".into()]).is_empty());
    }

    #[test]
    fn anchor_menu_lists_only_statement_enders() {
        let dir = std::env::temp_dir().join(format!("said_menu_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // line 1 ends a statement (;), line 2 is a mid-statement open, line 3 closes it.
        std::fs::write(dir.join("p.cs"), "app.Run();\napp.MapGet(\"/x\", () => 1)\n   .AllowAnonymous();\n").unwrap();
        let ctx = source_context(dir.to_str().unwrap(), &["p.cs".into()]);
        assert!(ctx.contains("SAFE anchor lines"));
        assert!(ctx.contains("app.Run();"));
        assert!(ctx.contains(".AllowAnonymous();"));
        // the mid-statement line must NOT be offered as a safe anchor
        assert!(!ctx.contains("1. app.MapGet(\"/x\", () => 1)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_file_is_numbered() {
        let dir = std::env::temp_dir().join(format!("said_src_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "line one\nline two\n").unwrap();
        let ctx = source_context(dir.to_str().unwrap(), &["a.rs".into()]);
        assert!(ctx.contains("1\tline one"));
        assert!(ctx.contains("2\tline two"));
        assert!(ctx.contains("## a.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

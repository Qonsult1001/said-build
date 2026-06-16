//! Apply a code/repair step's change-set to files on disk via sca-core's anchored
//! edit ops. NO whole-file rewrite path — every edit is bounded to a resolved
//! text anchor (`said edit`'s guarantee), so an LLM-driven loop physically cannot
//! gut a file. This is the closure the pipeline calls after the code/repair step.
//!
//! Change-set contract (what the CODE/REPAIR prompt must emit, same shape as
//! `said learn-fix --edits`):
//!
//! ```json
//! { "edits": [
//!     { "file": "src/Program.cs", "mode": "insert-after-text",
//!       "anchor": ".AllowAnonymous();", "content": "app.MapGet(...);" }
//! ] }
//! ```
//!
//! Supported modes (text-anchored, resolvable without the symbol index so apply
//! stays self-contained): insert-after-text | insert-before-text | replace-text.

use sca_core::edit::{self, EditOp};
use std::path::{Path, PathBuf};

/// One edit from the LLM's change-set.
#[derive(Debug, Clone)]
struct ChangeEdit {
    file: String,
    mode: String,
    anchor: String,
    content: String,
}

/// Parse the change-set JSON out of an LLM step output. The model may wrap it in
/// prose/markdown; we extract the first JSON object/array with an `edits` array.
fn parse_change_set(output: &str) -> Result<Vec<ChangeEdit>, String> {
    let val = extract_json(output).ok_or("no JSON change-set found in step output")?;
    let arr = val
        .get("edits")
        .and_then(|v| v.as_array())
        .or_else(|| val.as_array())
        .ok_or("change-set has no `edits` array")?;
    let mut out = Vec::new();
    for e in arr {
        let file = e.get("file").and_then(|v| v.as_str()).ok_or("edit missing `file`")?;
        let mode = e.get("mode").and_then(|v| v.as_str()).ok_or("edit missing `mode`")?;
        let anchor = e.get("anchor").and_then(|v| v.as_str()).unwrap_or("");
        let content = e.get("content").and_then(|v| v.as_str()).unwrap_or("");
        out.push(ChangeEdit {
            file: file.to_string(),
            mode: mode.to_string(),
            anchor: anchor.to_string(),
            content: content.to_string(),
        });
    }
    if out.is_empty() {
        return Err("change-set `edits` is empty".into());
    }
    Ok(out)
}

/// Find the first balanced JSON value (object or array) in arbitrary text.
fn extract_json(s: &str) -> Option<serde_json::Value> {
    // Fast path: whole string is JSON.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s.trim()) {
        return Some(v);
    }
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'{' || b == b'[' {
            let open = b;
            let close = if b == b'{' { b'}' } else { b']' };
            let mut depth = 0i32;
            let mut in_str = false;
            let mut esc = false;
            for (j, &c) in bytes.iter().enumerate().skip(i) {
                if in_str {
                    if esc { esc = false; }
                    else if c == b'\\' { esc = true; }
                    else if c == b'"' { in_str = false; }
                } else if c == b'"' { in_str = true; }
                else if c == open { depth += 1; }
                else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s[i..=j]) {
                            return Some(v);
                        }
                        break;
                    }
                }
            }
        }
    }
    None
}

/// Find the actual file line matching `anchor`, tolerating whitespace
/// differences (Claude's `findActualString` idea). Returns the file's real
/// substring so resolve/replace operate on exact bytes. Exact match first; then
/// a whitespace-collapsed comparison per line.
fn resolve_anchor_tolerant(content: &str, anchor: &str) -> Option<String> {
    // Exact substring (fast path — what resolve_text_anchor uses).
    if content.lines().any(|l| l.contains(anchor)) {
        return Some(anchor.to_string());
    }
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let na = norm(anchor);
    if na.is_empty() {
        return None;
    }
    // Single-line whitespace-tolerant match.
    for line in content.lines() {
        if norm(line).contains(&na) {
            return Some(line.trim().to_string());
        }
    }
    // MULTI-LINE match: the model often collapses a multi-line statement (e.g.
    // `app.MapGet(...)\n  .AllowAnonymous();`) into one anchor line, joining the
    // parts with or without a space. Compare with ALL whitespace removed so the
    // join style doesn't matter; on a hit, return the LAST line of the span — the
    // statement-ending line, the safe insert-after anchor (Advisory's lesson).
    let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let sa = strip(anchor);
    let lines: Vec<&str> = content.lines().collect();
    for start in 0..lines.len() {
        let mut window = String::new();
        for end in start..lines.len().min(start + 8) {
            window.push_str(&strip(lines[end]));
            if window.contains(&sa) {
                return Some(lines[end].trim().to_string());
            }
        }
    }
    None
}

/// Find the EXACT substring in `content` to replace for a replace-text edit,
/// tolerating whitespace/newline differences in the model's anchor. Returns the
/// real substring from the file (single OR multi-line) so ReplaceSubstring removes
/// exactly it. Without this, a multi-line anchor collapses to one line and only
/// part gets replaced (file corruption).
fn resolve_replace_needle(content: &str, anchor: &str) -> Option<String> {
    // 1. Exact substring already present.
    if content.contains(anchor) {
        return Some(anchor.to_string());
    }
    // 2. Whitespace-insensitive search: strip all whitespace from anchor, then
    //    scan the file for the substring whose stripped form contains it, and
    //    return that exact original span. Walk windows by char index.
    let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let sa = strip(anchor);
    if sa.is_empty() { return None; }
    let chars: Vec<char> = content.chars().collect();
    // Precompute, for each start index, the stripped length needed.
    for start in 0..chars.len() {
        let mut stripped = String::new();
        let mut end = start;
        while end < chars.len() && stripped.len() < sa.len() {
            if !chars[end].is_whitespace() { stripped.push(chars[end]); }
            end += 1;
        }
        if stripped == sa {
            // Trim trailing whitespace-only chars from the span for a clean needle.
            let span: String = chars[start..end].iter().collect();
            return Some(span);
        }
    }
    None
}

/// True if `file_line` ends a complete statement/block — safe to insert AFTER
/// without splitting a multi-line statement. Guards the multi-line hazard: a
/// `app.MapGet(...)` whose `.AllowAnonymous();` is on the next line is NOT a safe
/// insert-after anchor (Advisory's RealEndpointAnchors lesson). Statement-enders
/// end in ; } { (or a line comment / blank).
fn ends_statement(content: &str, anchor: &str) -> bool {
    // Find the actual file line containing the (already-resolved) anchor.
    let line = content.lines().find(|l| l.contains(anchor)).unwrap_or(anchor);
    let t = line.trim_end();
    t.ends_with(';') || t.ends_with('}') || t.ends_with('{') || t.is_empty()
        || t.ends_with("*/")
}

/// Resolve one text-anchored edit to a bounded `EditOp` over the file content.
fn to_edit_op(content: &str, e: &ChangeEdit) -> Result<EditOp, String> {
    match e.mode.as_str() {
        "insert-after-text" => {
            let anchor = resolve_anchor_tolerant(content, &e.anchor)
                .ok_or_else(|| format!("anchor text not found: {:?}", e.anchor))?;
            // Guard: inserting after a line that does NOT end a statement would
            // split a multi-line statement (the build-break we hit live). Reject
            // so repair picks a statement-ending anchor instead.
            if !ends_statement(content, &anchor) {
                return Err(format!(
                    "unsafe insert-after anchor (mid-statement — does not end in ; }} or {{): {:?}. \
                     Anchor on a line that ENDS a complete statement.",
                    anchor
                ));
            }
            let line = edit::resolve_text_anchor(content, &anchor)?;
            Ok(EditOp::InsertAfterLine { line, text: e.content.clone() })
        }
        "insert-before-text" => {
            let anchor = resolve_anchor_tolerant(content, &e.anchor)
                .ok_or_else(|| format!("anchor text not found: {:?}", e.anchor))?;
            let line = edit::resolve_text_anchor(content, &anchor)?;
            Ok(EditOp::InsertBeforeLine { line, text: e.content.clone() })
        }
        "replace-text" => {
            // For replace-text the needle must be the EXACT substring to remove —
            // multi-line included. resolve_anchor_tolerant collapses to a single
            // line (fine for insert anchors, WRONG here: it'd replace only part of
            // a multi-line anchor and corrupt the file). Find the real multi-line
            // span instead.
            let needle = resolve_replace_needle(content, &e.anchor)
                .ok_or_else(|| format!("anchor text not found: {:?}", e.anchor))?;
            Ok(EditOp::ReplaceSubstring { needle, replacement: e.content.clone() })
        }
        other => Err(format!(
            "unsupported mode '{}' (orchestrator apply handles text-anchored modes: \
             insert-after-text | insert-before-text | replace-text)",
            other
        )),
    }
}

/// Apply an LLM change-set under `repo_root`. Groups edits by file, resolves +
/// applies each file's ops via sca-core, writes back. Returns a summary line.
/// Errors (bad JSON, unresolved anchor) propagate so the gate/repair loop reacts.
pub fn apply_change_set(repo_root: &str, output: &str) -> Result<String, String> {
    let edits = parse_change_set(output)?;
    // Group by file, preserving order.
    let mut files: Vec<(String, Vec<ChangeEdit>)> = Vec::new();
    for e in edits {
        if let Some((_, v)) = files.iter_mut().find(|(f, _)| *f == e.file) {
            v.push(e);
        } else {
            files.push((e.file.clone(), vec![e]));
        }
    }
    let mut applied = 0usize;
    for (file, file_edits) in &files {
        let path: PathBuf = Path::new(repo_root).join(file);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;
        let mut ops = Vec::with_capacity(file_edits.len());
        for e in file_edits {
            ops.push(to_edit_op(&content, e)?);
        }
        let new_content = edit::apply_all(&content, &ops)?;
        std::fs::write(&path, new_content)
            .map_err(|e| format!("write {}: {}", path.display(), e))?;
        applied += file_edits.len();
    }
    Ok(format!("applied {} edit(s) across {} file(s)", applied, files.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_prose() {
        let out = "Here is the change:\n```json\n{\"edits\":[{\"file\":\"a.rs\",\"mode\":\"replace-text\",\"anchor\":\"foo\",\"content\":\"bar\"}]}\n```\nDone.";
        let edits = parse_change_set(out).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].file, "a.rs");
        assert_eq!(edits[0].mode, "replace-text");
    }

    #[test]
    fn replace_text_applies() {
        let dir = std::env::temp_dir().join(format!("said_apply_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.txt");
        std::fs::write(&f, "hello world\n").unwrap();
        let out = r#"{"edits":[{"file":"x.txt","mode":"replace-text","anchor":"world","content":"said"}]}"#;
        apply_change_set(dir.to_str().unwrap(), out).unwrap();
        let got = std::fs::read_to_string(&f).unwrap();
        assert!(got.contains("hello said"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiline_anchor_resolves_to_ending_line() {
        // Model collapsed a 2-line statement into one anchor; resolver should
        // map it to the statement-ending line (.AllowAnonymous();).
        let content = "app.MapGet(\"/api/health\", () => Results.Ok(new { status = \"ok\" }))\n   .AllowAnonymous();\napp.Run();\n";
        let collapsed = "app.MapGet(\"/api/health\", () => Results.Ok(new { status = \"ok\" })).AllowAnonymous();";
        let resolved = resolve_anchor_tolerant(content, collapsed).unwrap();
        assert_eq!(resolved, ".AllowAnonymous();");
        // And it's a valid statement-ender → insert-after is allowed.
        let e = ChangeEdit { file: "f".into(), mode: "insert-after-text".into(), anchor: collapsed.into(), content: "x".into() };
        assert!(to_edit_op(content, &e).is_ok());
    }

    #[test]
    fn multiline_replace_text_removes_whole_span() {
        // The bug: a multi-line replace anchor must replace ALL its lines, not one.
        let dir = std::env::temp_dir().join(format!("said_repl_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("r.js");
        std::fs::write(&f, "function add() {\n  throw new Error('x');\n}\nmodule.exports={add};\n").unwrap();
        // Model's anchor differs in whitespace; replacement is the real impl.
        let out = r#"{"edits":[{"file":"r.js","mode":"replace-text","anchor":"function add() {\n  throw new Error('x');\n}","content":"function add(a,b){ return a+b; }"}]}"#;
        apply_change_set(dir.to_str().unwrap(), out).unwrap();
        let got = std::fs::read_to_string(&f).unwrap();
        assert!(got.contains("return a+b"), "impl present");
        assert!(!got.contains("throw new Error"), "old body fully removed");
        assert_eq!(got.matches("function add").count(), 1, "no duplicated function");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_mid_statement_insert_after() {
        // Anchor on the FIRST line of a multi-line statement → must be rejected.
        let content = "app.MapGet(\"/x\", () => 1)\n   .AllowAnonymous();\n";
        let e = ChangeEdit {
            file: "f".into(), mode: "insert-after-text".into(),
            anchor: "app.MapGet(\"/x\", () => 1)".into(), content: "y".into(),
        };
        assert!(to_edit_op(content, &e).is_err());
        // Anchoring on the statement-ending line is fine.
        let mut e2 = e.clone();
        e2.anchor = ".AllowAnonymous();".into();
        assert!(to_edit_op(content, &e2).is_ok());
    }

    #[test]
    fn unsupported_mode_errors() {
        let out = r#"{"edits":[{"file":"x","mode":"delete-symbol","anchor":"Foo","content":""}]}"#;
        // parse ok, apply fails on mode at to_edit_op (via apply_change_set read first)
        let edits = parse_change_set(out).unwrap();
        assert_eq!(edits[0].mode, "delete-symbol");
    }
}

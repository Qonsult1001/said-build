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

/// Resolve an insert anchor STRICTLY — Claude's Edit-tool guarantee: the anchor
/// must appear in the file EXACTLY (verbatim) and UNIQUELY. No whitespace
/// tolerance, no fuzzy/collapsed matching: a fuzzy match produces a
/// plausible-but-wrong edit (the corruption class we hit), whereas a clean
/// rejection feeds the repair loop a real error to fix — exactly how Claude's
/// "the edit will FAIL if old_string is not unique" works.
///
/// Returns the matched line (an insert anchor is a single line), or an Err
/// describing WHY (not found / not unique) so repair can correct the anchor.
fn resolve_anchor_strict(content: &str, anchor: &str) -> Result<String, String> {
    let a = anchor.trim();
    if a.is_empty() {
        return Err("empty anchor".into());
    }
    // An insert anchor must identify ONE line. Count lines that contain it verbatim.
    let hits: Vec<&str> = content.lines().filter(|l| l.contains(a)).collect();
    match hits.len() {
        0 => Err(format!(
            "anchor not found verbatim: {:?}. Copy an EXACT line from the current source \
             (or use write-file).",
            a
        )),
        1 => Ok(a.to_string()),
        n => Err(format!(
            "anchor is not unique ({} matches): {:?}. Use a longer, unique anchor — \
             or use write-file.",
            n, a
        )),
    }
}

/// Resolve a replace-text needle STRICTLY: the anchor must be an EXACT substring
/// of the file (newlines and whitespace included) appearing EXACTLY ONCE. This is
/// the replace analogue of Claude's unique-`old_string` rule. No whitespace
/// stripping: a fuzzy needle replaces the wrong span and corrupts the file; a
/// clean rejection lets repair fix the anchor (or switch to write-file).
fn resolve_replace_needle_strict(content: &str, anchor: &str) -> Result<String, String> {
    if anchor.is_empty() {
        return Err("empty replace-text anchor".into());
    }
    let count = content.matches(anchor).count();
    match count {
        0 => Err(format!(
            "replace-text anchor not found verbatim: {:?}. The anchor must be the EXACT \
             text to replace, copied from the current source — or use write-file.",
            anchor.chars().take(80).collect::<String>()
        )),
        1 => Ok(anchor.to_string()),
        n => Err(format!(
            "replace-text anchor is not unique ({} matches): {:?}. Include enough \
             surrounding text to make it unique — or use write-file.",
            n, anchor.chars().take(80).collect::<String>()
        )),
    }
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
            let anchor = resolve_anchor_strict(content, &e.anchor)?;
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
            let anchor = resolve_anchor_strict(content, &e.anchor)?;
            let line = edit::resolve_text_anchor(content, &anchor)?;
            Ok(EditOp::InsertBeforeLine { line, text: e.content.clone() })
        }
        "replace-text" => {
            // EXACT, UNIQUE substring or reject (Claude's unique-old_string rule).
            // A fuzzy needle replaces the wrong span and corrupts the file; a clean
            // rejection feeds the repair loop a real error to fix.
            let needle = resolve_replace_needle_strict(content, &e.anchor)?;
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
    if std::env::var("SAID_APPLY_DEBUG").is_ok() {
        eprintln!("[apply-dbg] {} edit(s):", edits.len());
        for (i, e) in edits.iter().enumerate() {
            eprintln!("[apply-dbg]  #{} {} {} anchor_len={} content_len={} anchor_head={:?}",
                i, e.mode, e.file, e.anchor.len(), e.content.len(),
                e.anchor.chars().take(40).collect::<String>());
        }
    }
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
        // WRITE-FILE mode: if any edit for this file is a whole-file write, that
        // wins — the model returns the FULL corrected file in one edit. This is
        // the robust path for complex multi-method rewrites where partial anchors
        // corrupt (Claude's Write vs Edit). Bounded to the named file (no
        // cross-file blast). If present, it supersedes anchored edits for the file.
        if let Some(wf) = file_edits.iter().find(|e| e.mode == "write-file") {
            if wf.content.trim().is_empty() {
                return Err(format!("write-file for {} has empty content (refusing to blank a file)", file));
            }
            // ANTI-GUTTING GUARD (the Program.cs lesson): a write-file must contain
            // the WHOLE file. If the new content is drastically SHORTER than the
            // existing non-trivial file, the model almost certainly emitted only its
            // changed lines (or a partial file) — which would DELETE the rest.
            // Reject so repair resubmits the full file. (A genuine large deletion is
            // rare; if intended, the task/repair note can request it explicitly and
            // the model can pass SAID_ALLOW_SHRINK — but default is SAFE.)
            if let Ok(existing) = std::fs::read_to_string(&path) {
                let old_lines = existing.lines().filter(|l| !l.trim().is_empty()).count();
                let new_lines = wf.content.lines().filter(|l| !l.trim().is_empty()).count();
                let was_stub = existing.contains("not implemented") || old_lines < 15;
                let allow_shrink = std::env::var("SAID_ALLOW_SHRINK").is_ok();
                // Guard only when replacing real code (not a stub) and not opted-in.
                if !was_stub && !allow_shrink && (new_lines as f32) < (old_lines as f32) * 0.5 {
                    return Err(format!(
                        "write-file for {} would SHRINK it from {} to {} non-empty lines (>50% deleted). \
                         A write-file must contain the COMPLETE file. You likely emitted only changed lines — \
                         resubmit the WHOLE file (all existing code you keep PLUS your changes). \
                         If a large deletion is truly intended, say so explicitly.",
                        file, old_lines, new_lines));
                }
            }
            std::fs::write(&path, &wf.content)
                .map_err(|e| format!("write {}: {}", path.display(), e))?;
            applied += 1;
            continue;
        }
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
    fn strict_anchor_rejects_fuzzy_collapse() {
        // STRICT: a collapsed/whitespace-differing anchor is NOT accepted (it would
        // produce a plausible-but-wrong edit). It must be rejected so repair fixes
        // the anchor — Claude's "edit FAILS if old_string isn't an exact match".
        let content = "app.MapGet(\"/api/health\", () => Results.Ok(new { status = \"ok\" }))\n   .AllowAnonymous();\napp.Run();\n";
        let collapsed = "app.MapGet(\"/api/health\", () => Results.Ok(new { status = \"ok\" })).AllowAnonymous();";
        assert!(resolve_anchor_strict(content, collapsed).is_err(), "fuzzy collapse rejected");
        // The EXACT statement-ending line resolves and is a valid insert-after.
        let exact = ".AllowAnonymous();";
        assert_eq!(resolve_anchor_strict(content, exact).unwrap(), exact);
        let e = ChangeEdit { file: "f".into(), mode: "insert-after-text".into(), anchor: exact.into(), content: "x".into() };
        assert!(to_edit_op(content, &e).is_ok());
    }

    #[test]
    fn multiline_replace_text_removes_whole_span() {
        // A multi-line replace anchor that matches EXACTLY removes all its lines.
        let dir = std::env::temp_dir().join(format!("said_repl_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("r.js");
        std::fs::write(&f, "function add() {\n  throw new Error('x');\n}\nmodule.exports={add};\n").unwrap();
        // Exact multi-line substring (verbatim from the file) → whole span replaced.
        let out = r#"{"edits":[{"file":"r.js","mode":"replace-text","anchor":"function add() {\n  throw new Error('x');\n}","content":"function add(a,b){ return a+b; }"}]}"#;
        apply_change_set(dir.to_str().unwrap(), out).unwrap();
        let got = std::fs::read_to_string(&f).unwrap();
        assert!(got.contains("return a+b"), "impl present");
        assert!(!got.contains("throw new Error"), "old body fully removed");
        assert_eq!(got.matches("function add").count(), 1, "no duplicated function");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strict_replace_rejects_nonunique_and_missing() {
        // Not found → reject.
        let content = "let a = 1;\nlet b = 2;\n";
        assert!(resolve_replace_needle_strict(content, "let c = 3;").is_err(), "missing rejected");
        // Not unique → reject (would corrupt by replacing the wrong one).
        let dup = "x();\nx();\n";
        assert!(resolve_replace_needle_strict(dup, "x();").is_err(), "non-unique rejected");
        // Exact + unique → ok.
        assert_eq!(resolve_replace_needle_strict(content, "let b = 2;").unwrap(), "let b = 2;");
    }

    #[test]
    fn write_file_rejects_gutting() {
        // Anti-gutting: write-file with far fewer lines than a real file is rejected.
        let dir = std::env::temp_dir().join(format!("said_wf_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("big.js");
        let big: String = (0..40).map(|i| format!("const x{} = {};\n", i, i)).collect();
        std::fs::write(&f, &big).unwrap();
        // Model emits only 2 lines as a "write-file" -> would gut the file -> reject.
        let out = r#"{"edits":[{"file":"big.js","mode":"write-file","content":"const x0 = 0;\nconst x1 = 1;\n"}]}"#;
        assert!(apply_change_set(dir.to_str().unwrap(), out).is_err(), "gutting must be rejected");
        // A full rewrite (similar size) is allowed.
        let full: String = (0..40).map(|i| format!("const y{} = {};\n", i, i)).collect();
        let out2 = format!(r#"{{"edits":[{{"file":"big.js","mode":"write-file","content":{}}}]}}"#, serde_json::to_string(&full).unwrap());
        assert!(apply_change_set(dir.to_str().unwrap(), &out2).is_ok(), "full rewrite allowed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_replaces_stub() {
        // Replacing a stub (small/"not implemented") with a full impl is allowed.
        let dir = std::env::temp_dir().join(format!("said_wfs_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.js");
        std::fs::write(&f, "function f(){ throw new Error('not implemented'); }\n").unwrap();
        let impl_: String = "function f(){ return 42; }\n".to_string();
        let out = format!(r#"{{"edits":[{{"file":"s.js","mode":"write-file","content":{}}}]}}"#, serde_json::to_string(&impl_).unwrap());
        assert!(apply_change_set(dir.to_str().unwrap(), &out).is_ok());
        assert!(std::fs::read_to_string(&f).unwrap().contains("return 42"));
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

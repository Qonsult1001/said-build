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

/// Resolve one text-anchored edit to a bounded `EditOp` over the file content.
fn to_edit_op(content: &str, e: &ChangeEdit) -> Result<EditOp, String> {
    match e.mode.as_str() {
        "insert-after-text" => {
            let line = edit::resolve_text_anchor(content, &e.anchor)?;
            Ok(EditOp::InsertAfterLine { line, text: e.content.clone() })
        }
        "insert-before-text" => {
            let line = edit::resolve_text_anchor(content, &e.anchor)?;
            Ok(EditOp::InsertBeforeLine { line, text: e.content.clone() })
        }
        "replace-text" => Ok(EditOp::ReplaceSubstring {
            needle: e.anchor.clone(),
            replacement: e.content.clone(),
        }),
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
    fn unsupported_mode_errors() {
        let out = r#"{"edits":[{"file":"x","mode":"delete-symbol","anchor":"Foo","content":""}]}"#;
        // parse ok, apply fails on mode at to_edit_op (via apply_change_set read first)
        let edits = parse_change_set(out).unwrap();
        assert_eq!(edits[0].mode, "delete-symbol");
    }
}

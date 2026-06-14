//! `said edit` — surgical, anchored edits to a source file on disk.
//!
//! The whole point of this module is that there is **no whole-file-write path**.
//! Every edit is bounded to a resolved anchor (a symbol's line range, or a line
//! matched by an exact substring), so an LLM-driven caller physically cannot
//! delete the rest of a file — the failure mode that gutted Program.cs.
//!
//! `.said` resolves *where* (symbol → line range via the symbol index); the
//! bytes are written to the real file on disk. The editing core here is a set
//! of pure functions over the file's text so they are unit-testable without a
//! brain or the filesystem.

/// Newline style of a source file — preserved across an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newline {
    Lf,
    Crlf,
}

impl Newline {
    /// Detect the dominant newline style of `content` (CRLF if any `\r\n`).
    pub fn detect(content: &str) -> Newline {
        if content.contains("\r\n") {
            Newline::Crlf
        } else {
            Newline::Lf
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Newline::Lf => "\n",
            Newline::Crlf => "\r\n",
        }
    }
}

/// A resolved, bounded edit operation over 1-based line numbers.
///
/// `InsertAfter`/`InsertBefore` take a line number; `ReplaceLines`/`DeleteLines`
/// take an inclusive 1-based range. `ReplaceSubstring` is a single in-line
/// replacement of the first occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOp {
    InsertAfterLine { line: usize, text: String },
    InsertBeforeLine { line: usize, text: String },
    ReplaceLines { start: usize, end: usize, text: String },
    DeleteLines { start: usize, end: usize },
    ReplaceSubstring { needle: String, replacement: String },
}

/// Result of applying an edit: the new file content plus a summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditResult {
    pub content: String,
    pub applied_at_line: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

/// Maximum line span a single replace/delete may touch without `--allow-large`.
pub const DEFAULT_MAX_SPAN: usize = 200;

/// Apply a resolved [`EditOp`] to `content`, preserving its newline style.
///
/// Pure: no I/O. Returns the rewritten content + a change summary, or an error
/// describing why the edit could not be applied (never silently rewrites).
pub fn apply_edit(content: &str, op: &EditOp) -> Result<EditResult, String> {
    let nl = Newline::detect(content);
    // Split into logical lines without their terminators. Track whether the
    // file ended with a trailing newline so we can reproduce it exactly.
    let had_trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    match op {
        EditOp::InsertAfterLine { line, text } => {
            if *line == 0 || *line > lines.len() {
                return Err(format!(
                    "anchor line {} out of range (file has {} lines)",
                    line,
                    lines.len()
                ));
            }
            let added = count_inserted_lines(text);
            // Insert after the 1-based `line` → at vec index `line`.
            insert_lines_at(&mut lines, *line, text);
            Ok(EditResult {
                content: join_lines(&lines, nl, had_trailing_newline),
                applied_at_line: *line + 1,
                lines_added: added,
                lines_removed: 0,
            })
        }
        EditOp::InsertBeforeLine { line, text } => {
            if *line == 0 || *line > lines.len() {
                return Err(format!(
                    "anchor line {} out of range (file has {} lines)",
                    line,
                    lines.len()
                ));
            }
            let added = count_inserted_lines(text);
            insert_lines_at(&mut lines, *line - 1, text);
            Ok(EditResult {
                content: join_lines(&lines, nl, had_trailing_newline),
                applied_at_line: *line,
                lines_added: added,
                lines_removed: 0,
            })
        }
        EditOp::ReplaceLines { start, end, text } => {
            check_range(*start, *end, lines.len())?;
            let removed = end - start + 1;
            let added = count_inserted_lines(text);
            // Drop [start-1 ..= end-1], then splice the new text in at start-1.
            lines.drain((start - 1)..=(end - 1));
            insert_lines_at(&mut lines, start - 1, text);
            Ok(EditResult {
                content: join_lines(&lines, nl, had_trailing_newline),
                applied_at_line: *start,
                lines_added: added,
                lines_removed: removed,
            })
        }
        EditOp::DeleteLines { start, end } => {
            check_range(*start, *end, lines.len())?;
            let removed = end - start + 1;
            lines.drain((start - 1)..=(end - 1));
            Ok(EditResult {
                content: join_lines(&lines, nl, had_trailing_newline),
                applied_at_line: *start,
                lines_added: 0,
                lines_removed: removed,
            })
        }
        EditOp::ReplaceSubstring { needle, replacement } => {
            let pos = content
                .find(needle.as_str())
                .ok_or_else(|| format!("anchor text not found: {:?}", needle))?;
            // 1-based line number where the match starts.
            let line_no = content[..pos].matches('\n').count() + 1;
            let new_content = content.replacen(needle.as_str(), replacement, 1);
            Ok(EditResult {
                content: new_content,
                applied_at_line: line_no,
                lines_added: 0,
                lines_removed: 0,
            })
        }
    }
}

/// Anchor-drift / freshness check for symbol-mode edits (recall correctness).
///
/// The brain stores what a symbol looked like when it was indexed. Before we
/// edit by symbol line-range, confirm the file on disk still matches what the
/// brain indexed — otherwise the range is stale (the file changed since the
/// last `said init`/`reindex`) and the edit could land on the wrong lines.
///
/// Comparison is whitespace-insensitive (LF/CRLF, trailing spaces, blank-line
/// runs don't count as drift) so cosmetic formatting differences never block a
/// legitimate edit. Returns `Err` only on a real content divergence, with a
/// message telling the caller to reindex.
pub fn check_symbol_fresh(indexed: &str, ondisk: &str) -> Result<(), String> {
    if normalize_for_compare(indexed) == normalize_for_compare(ondisk) {
        Ok(())
    } else {
        Err("stale anchor: the file changed since it was indexed — run `said reindex <file>` and retry (refusing to edit against an out-of-date brain)".to_string())
    }
}

/// Normalize text for drift comparison: unify newlines, trim trailing
/// whitespace per line, and drop blank lines so formatting noise isn't drift.
fn normalize_for_compare(s: &str) -> String {
    s.replace("\r\n", "\n")
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Apply a sequence of edits transactionally to `content`. Each op sees the
/// result of the previous one. If ANY op fails, the whole call returns `Err`
/// and the returned string is never the partially-edited content — the caller
/// writes nothing, so no half-applied change reaches disk.
///
/// Note: ops are applied in the given order against shifting line numbers, so
/// callers passing line-based ops should order them bottom-to-top or use
/// text/context anchors (which re-resolve against the current content). The
/// transactional guarantee holds regardless of ordering.
pub fn apply_all(content: &str, ops: &[EditOp]) -> Result<String, String> {
    let mut current = content.to_string();
    for (i, op) in ops.iter().enumerate() {
        let r = apply_edit(&current, op)
            .map_err(|e| format!("edit {} of {} failed: {}", i + 1, ops.len(), e))?;
        current = r.content;
    }
    Ok(current)
}

/// Validate an inclusive 1-based line range against the file's line count.
fn check_range(start: usize, end: usize, total: usize) -> Result<(), String> {
    if start == 0 || end == 0 {
        return Err("line numbers are 1-based; got 0".to_string());
    }
    if start > end {
        return Err(format!("invalid range: start {} > end {}", start, end));
    }
    if end > total {
        return Err(format!(
            "range {}..={} out of bounds (file has {} lines)",
            start, end, total
        ));
    }
    Ok(())
}

/// A symbol candidate from the brain's symbol index. `doc_id` encodes the file
/// as its first `::`-delimited segment (`<rel_path>::<name>::<kind>:<line>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymCandidate {
    pub doc_id: String,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Extract the file path (first `::` segment) from a doc_id.
fn doc_id_file(doc_id: &str) -> &str {
    doc_id.split("::").next().unwrap_or(doc_id)
}

/// Compare two repo-relative paths ignoring separator style (`/` vs `\`).
pub fn paths_equal(a: &str, b: &str) -> bool {
    a.replace('\\', "/") == b.replace('\\', "/")
}

/// Strict resolve: errors on 0 or >1 matches. Backward-compatible wrapper over
/// [`resolve_symbol_ex`] with no line filter and no largest-span preference.
pub fn resolve_symbol_in_file(
    candidates: &[SymCandidate],
    file: &str,
) -> Result<(usize, usize), String> {
    resolve_symbol_ex(candidates, file, None, false)
}

/// Resolve a symbol to its `(start_line, end_line)` **scoped to `file`**, with
/// disambiguation options for the C# class/ctor name-clash (and similar):
///
/// - `line = Some(N)` → select the candidate whose `start_line == N` (the error
///   message lists candidate start lines, so a caller can pass one back).
/// - `prefer_largest = true` → when multiple candidates match, pick the one with
///   the **largest span** (the enclosing class, not its 1-line constructor).
///   Used by `append-into-symbol`, where "add a member to ClassName" means the
///   class body.
/// - otherwise multiple matches → error listing the candidate start lines.
pub fn resolve_symbol_ex(
    candidates: &[SymCandidate],
    file: &str,
    line: Option<usize>,
    prefer_largest: bool,
) -> Result<(usize, usize), String> {
    let matches: Vec<&SymCandidate> = candidates
        .iter()
        .filter(|c| paths_equal(doc_id_file(&c.doc_id), file))
        .collect();
    if matches.is_empty() {
        return Err(format!("symbol not found in {}", file));
    }
    // Explicit line wins.
    if let Some(n) = line {
        return match matches.iter().find(|c| c.start_line == n) {
            Some(c) => Ok((c.start_line, c.end_line)),
            None => {
                let starts: Vec<String> = matches.iter().map(|c| c.start_line.to_string()).collect();
                Err(format!(
                    "no symbol starts at line {} in {} (candidates start at: {})",
                    n, file, starts.join(", ")
                ))
            }
        };
    }
    if matches.len() == 1 {
        return Ok((matches[0].start_line, matches[0].end_line));
    }
    // Multiple matches.
    if prefer_largest {
        let best = matches.iter().max_by_key(|c| c.end_line.saturating_sub(c.start_line)).unwrap();
        return Ok((best.start_line, best.end_line));
    }
    let listed: Vec<String> = matches
        .iter()
        .map(|c| format!("{}:{}-{} ({})", c.name, c.start_line, c.end_line, doc_id_kind(&c.doc_id)))
        .collect();
    let starts: Vec<String> = matches.iter().map(|c| c.start_line.to_string()).collect();
    Err(format!(
        "ambiguous: {} symbols match in {} ({}); disambiguate with --line <N> (one of: {})",
        matches.len(), file, listed.join(", "), starts.join(", ")
    ))
}

/// Extract the kind (third `::`-segment, before the trailing `:line`) from a doc_id.
fn doc_id_kind(doc_id: &str) -> &str {
    doc_id.split("::").nth(2)
        .map(|s| s.split(':').next().unwrap_or(s))
        .unwrap_or("?")
}

/// Guard a replace/delete against accidentally selecting a huge range.
/// `span` is the number of lines the op would remove. Rejected if it exceeds
/// `max` unless `allow_large` is set.
pub fn check_span(span: usize, max: usize, allow_large: bool) -> Result<(), String> {
    if span > max && !allow_large {
        return Err(format!(
            "edit would change {} lines (>{} max); pass --allow-large to override",
            span, max
        ));
    }
    Ok(())
}

/// Reject `--file` values that escape the working directory: absolute paths
/// (POSIX `/...` or Windows `C:\...` / drive-letter) or any `..` component.
pub fn is_safe_relative_path(file: &str) -> Result<(), String> {
    if file.is_empty() {
        return Err("--file is empty".to_string());
    }
    // Absolute POSIX, UNC, or Windows drive-letter paths.
    let bytes = file.as_bytes();
    let is_windows_abs = bytes.len() >= 2 && bytes[1] == b':';
    if file.starts_with('/') || file.starts_with('\\') || is_windows_abs {
        return Err(format!("--file must be relative, not absolute: {}", file));
    }
    // Any `..` path component (handle both separators).
    for comp in file.split(['/', '\\']) {
        if comp == ".." {
            return Err(format!("--file must not contain '..': {}", file));
        }
    }
    Ok(())
}

/// Leading whitespace (indent) of a line.
pub fn indent_of(line: &str) -> &str {
    let end = line.find(|c: char| !c.is_whitespace()).unwrap_or(line.len());
    &line[..end]
}

/// Re-indent a (possibly multi-line) content block to a `base` indent, while
/// preserving the block's own relative nesting. Strips the block's common
/// leading indent first so a block authored at column 0 OR already indented
/// both land correctly at `base`. Blank lines stay blank.
pub fn reindent_block(content: &str, base: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    // Common leading whitespace across all non-blank lines.
    let common = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l).len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", base, &l[common..])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Verify that `content` is still syntactically valid for the given file
/// `extension` after an edit, using tree-sitter (only when the `code` feature
/// is enabled). Returns `Ok(())` if valid or if no grammar is available; `Err`
/// if the edit left the file un-parseable. Without the `code` feature this is
/// always `Ok` (no grammars compiled in).
#[cfg(feature = "code")]
pub fn verify_syntax(content: &str, extension: &str) -> Result<(), String> {
    crate::code_search::verify_syntax(content, extension)
}

/// Resolve a (possibly multi-line) context block to the 1-based line number
/// where it starts. Unlike [`resolve_text_anchor`], this requires the block to
/// occur **exactly once** — errors on 0 (missing) or >1 (ambiguous) so an edit
/// can never land in the wrong place when a short string repeats.
pub fn resolve_context_anchor(content: &str, block: &str) -> Result<usize, String> {
    // Count occurrences of the exact block.
    let mut count = 0usize;
    let mut first_pos: Option<usize> = None;
    let mut search_from = 0usize;
    while let Some(rel) = content[search_from..].find(block) {
        let pos = search_from + rel;
        if first_pos.is_none() {
            first_pos = Some(pos);
        }
        count += 1;
        search_from = pos + block.len().max(1);
    }
    match count {
        0 => Err(format!("context block not found: {:?}", block)),
        1 => {
            let pos = first_pos.unwrap();
            Ok(content[..pos].matches('\n').count() + 1)
        }
        n => Err(format!(
            "ambiguous: context block occurs {} times; add more surrounding lines to make it unique",
            n
        )),
    }
}

/// Resolve an exact-substring anchor to the 1-based line number of the first
/// line that contains it. Errors if no line matches (never guesses).
pub fn resolve_text_anchor(content: &str, anchor: &str) -> Result<usize, String> {
    for (i, line) in content.lines().enumerate() {
        if line.contains(anchor) {
            return Ok(i + 1);
        }
    }
    Err(format!("anchor text not found: {:?}", anchor))
}

/// Number of logical lines a piece of insert `text` contributes.
fn count_inserted_lines(text: &str) -> usize {
    text.lines().count().max(1)
}

/// Insert `text` (possibly multi-line) into `lines` at vec index `idx`.
fn insert_lines_at(lines: &mut Vec<String>, idx: usize, text: &str) {
    let new: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let new = if new.is_empty() { vec![String::new()] } else { new };
    for (offset, l) in new.into_iter().enumerate() {
        lines.insert(idx + offset, l);
    }
}

/// Join logical lines back into file content with the given newline style,
/// reproducing the original trailing-newline state.
fn join_lines(lines: &[String], nl: Newline, trailing: bool) -> String {
    let mut out = lines.join(nl.as_str());
    if trailing && !lines.is_empty() {
        out.push_str(nl.as_str());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_after_line_adds_one_line_and_keeps_the_rest() {
        let src = "fn a() {}\napp.MapGet(\"/api/pid\");\nfn c() {}\n";
        // Insert after line 2 (the MapGet line).
        let op = EditOp::InsertAfterLine {
            line: 2,
            text: "app.MapGet(\"/api/host\");".to_string(),
        };

        let r = apply_edit(src, &op).expect("edit should apply");

        assert_eq!(
            r.content,
            "fn a() {}\napp.MapGet(\"/api/pid\");\napp.MapGet(\"/api/host\");\nfn c() {}\n"
        );
        assert_eq!(r.applied_at_line, 3, "new line lands at line 3");
        assert_eq!(r.lines_added, 1);
        assert_eq!(r.lines_removed, 0);
    }

    #[test]
    fn insert_before_line_lands_above_the_anchor() {
        let src = "one\ntwo\nthree\n";
        let op = EditOp::InsertBeforeLine { line: 2, text: "inserted".to_string() };
        let r = apply_edit(src, &op).unwrap();
        assert_eq!(r.content, "one\ninserted\ntwo\nthree\n");
        assert_eq!(r.applied_at_line, 2);
        assert_eq!(r.lines_added, 1);
    }

    #[test]
    fn replace_lines_swaps_only_the_range() {
        let src = "keep1\nold_a\nold_b\nkeep2\n";
        let op = EditOp::ReplaceLines { start: 2, end: 3, text: "new_single".to_string() };
        let r = apply_edit(src, &op).unwrap();
        assert_eq!(r.content, "keep1\nnew_single\nkeep2\n");
        assert_eq!(r.lines_removed, 2);
        assert_eq!(r.lines_added, 1);
        assert_eq!(r.applied_at_line, 2);
    }

    #[test]
    fn delete_lines_removes_only_the_range() {
        let src = "a\nb\nc\nd\n";
        let op = EditOp::DeleteLines { start: 2, end: 3 };
        let r = apply_edit(src, &op).unwrap();
        assert_eq!(r.content, "a\nd\n");
        assert_eq!(r.lines_removed, 2);
        assert_eq!(r.lines_added, 0);
    }

    #[test]
    fn replace_substring_changes_only_first_occurrence() {
        let src = "let x = OLD;\nlet y = OLD;\n";
        let op = EditOp::ReplaceSubstring {
            needle: "OLD".to_string(),
            replacement: "NEW".to_string(),
        };
        let r = apply_edit(src, &op).unwrap();
        assert_eq!(r.content, "let x = NEW;\nlet y = OLD;\n");
        assert_eq!(r.applied_at_line, 1);
    }

    #[test]
    fn crlf_files_keep_crlf_after_edit() {
        let src = "a\r\nMapGet\r\nc\r\n";
        let op = EditOp::InsertAfterLine { line: 2, text: "added".to_string() };
        let r = apply_edit(src, &op).unwrap();
        assert_eq!(r.content, "a\r\nMapGet\r\nadded\r\nc\r\n");
    }

    #[test]
    fn out_of_range_anchor_errors_and_does_not_panic() {
        let src = "only\ntwo\n";
        let op = EditOp::InsertAfterLine { line: 99, text: "x".to_string() };
        assert!(apply_edit(src, &op).is_err());
    }

    #[test]
    fn resolve_text_anchor_returns_first_matching_line() {
        let src = "alpha\nfind ME here\nbeta\nfind ME again\n";
        // 1-based line number of the FIRST line containing the substring.
        assert_eq!(resolve_text_anchor(src, "find ME").unwrap(), 2);
    }

    #[test]
    fn resolve_text_anchor_missing_errors() {
        let src = "alpha\nbeta\n";
        assert!(resolve_text_anchor(src, "nope").is_err());
    }

    #[test]
    fn span_within_limit_is_allowed() {
        assert!(check_span(10, DEFAULT_MAX_SPAN, false).is_ok());
    }

    #[test]
    fn span_over_limit_is_rejected_without_allow_large() {
        let r = check_span(DEFAULT_MAX_SPAN + 1, DEFAULT_MAX_SPAN, false);
        assert!(r.is_err());
    }

    #[test]
    fn span_over_limit_is_allowed_with_allow_large() {
        assert!(check_span(DEFAULT_MAX_SPAN + 50, DEFAULT_MAX_SPAN, true).is_ok());
    }

    #[test]
    fn path_with_dotdot_is_rejected() {
        assert!(is_safe_relative_path("../etc/passwd").is_err());
        assert!(is_safe_relative_path("src/../../secret").is_err());
    }

    #[test]
    fn absolute_path_is_rejected() {
        assert!(is_safe_relative_path("/etc/passwd").is_err());
        assert!(is_safe_relative_path("C:\\Windows\\system32").is_err());
    }

    #[test]
    fn ordinary_relative_path_is_ok() {
        assert!(is_safe_relative_path("src/Advisory.Api/Program.cs").is_ok());
    }

    fn cand(doc_id: &str, name: &str, s: usize, e: usize) -> SymCandidate {
        SymCandidate { doc_id: doc_id.to_string(), name: name.to_string(), start_line: s, end_line: e }
    }

    #[test]
    fn symbol_resolves_to_single_match_in_file() {
        let cands = vec![
            cand("src/Program.cs::Configure::method:10", "Configure", 10, 20),
            cand("src/Other.cs::Configure::method:5", "Configure", 5, 8),
        ];
        let (s, e) = resolve_symbol_in_file(&cands, "src/Program.cs").unwrap();
        assert_eq!((s, e), (10, 20), "scoped to Program.cs only");
    }

    #[test]
    fn symbol_not_in_file_errors() {
        let cands = vec![cand("src/Other.cs::Foo::method:5", "Foo", 5, 8)];
        assert!(resolve_symbol_in_file(&cands, "src/Program.cs").is_err());
    }

    #[test]
    fn ambiguous_symbol_in_same_file_errors() {
        let cands = vec![
            cand("src/Program.cs::Foo::method:5", "Foo", 5, 8),
            cand("src/Program.cs::Foo::method:30", "Foo", 30, 33),
        ];
        let err = resolve_symbol_in_file(&cands, "src/Program.cs").unwrap_err();
        assert!(err.contains("2") || err.to_lowercase().contains("ambiguous"));
    }

    #[test]
    fn doc_id_file_matches_with_either_separator() {
        // Brain may store paths with forward slashes even on Windows.
        let cands = vec![cand("src/a/Program.cs::Foo::method:5", "Foo", 5, 8)];
        assert!(resolve_symbol_in_file(&cands, "src\\a\\Program.cs").is_ok());
    }

    // ---- C# class/ctor name-clash disambiguation (FIX 1) ---------------

    fn cs_class_and_ctor() -> Vec<SymCandidate> {
        // Real C# shape: class HealthTests (10-124) + ctor HealthTests (13-13).
        vec![
            cand("tests/HealthTests.cs::HealthTests::class_declaration:10", "HealthTests", 10, 124),
            cand("tests/HealthTests.cs::HealthTests::constructor_declaration:13", "HealthTests", 13, 13),
        ]
    }

    #[test]
    fn disambiguate_by_line_picks_the_matching_span() {
        let cands = cs_class_and_ctor();
        // --line 10 selects the class.
        let (s, e) = resolve_symbol_ex(&cands, "tests/HealthTests.cs", Some(10), false).unwrap();
        assert_eq!((s, e), (10, 124));
        // --line 13 selects the constructor.
        let (s, e) = resolve_symbol_ex(&cands, "tests/HealthTests.cs", Some(13), false).unwrap();
        assert_eq!((s, e), (13, 13));
    }

    #[test]
    fn disambiguate_by_line_errors_when_no_span_starts_there() {
        let cands = cs_class_and_ctor();
        assert!(resolve_symbol_ex(&cands, "tests/HealthTests.cs", Some(999), false).is_err());
    }

    #[test]
    fn prefer_largest_picks_the_class_over_the_constructor() {
        let cands = cs_class_and_ctor();
        // append-into-symbol uses prefer_largest=true → the class body (10-124).
        let (s, e) = resolve_symbol_ex(&cands, "tests/HealthTests.cs", None, true).unwrap();
        assert_eq!((s, e), (10, 124), "should pick the enclosing class, not the 1-line ctor");
    }

    #[test]
    fn ambiguous_still_errors_without_line_or_prefer_largest() {
        let cands = cs_class_and_ctor();
        let err = resolve_symbol_ex(&cands, "tests/HealthTests.cs", None, false).unwrap_err();
        assert!(err.to_lowercase().contains("ambiguous"));
        // The error must list the candidate START lines so the caller can pass --line.
        assert!(err.contains("10") && err.contains("13"));
    }

    #[test]
    fn old_resolve_still_errors_on_ambiguity() {
        // Backward-compat wrapper: no line, no prefer → still strict.
        assert!(resolve_symbol_in_file(&cs_class_and_ctor(), "tests/HealthTests.cs").is_err());
    }

    // ---- multi-line context anchor (disambiguation) --------------------

    #[test]
    fn context_anchor_resolves_unique_multiline_block() {
        let src = "fn a() {\n    let x = 1;\n    return x;\n}\nfn b() {\n    let x = 1;\n    return x + 1;\n}\n";
        // "let x = 1;\n    return x;" is unique (only in fn a). 1-based start line.
        let line = resolve_context_anchor(src, "let x = 1;\n    return x;").unwrap();
        assert_eq!(line, 2);
    }

    #[test]
    fn context_anchor_ambiguous_block_errors() {
        // "let x = 1;" alone appears twice → ambiguous, must error not guess.
        let src = "fn a() {\n    let x = 1;\n}\nfn b() {\n    let x = 1;\n}\n";
        let err = resolve_context_anchor(src, "    let x = 1;").unwrap_err();
        assert!(err.to_lowercase().contains("ambiguous") || err.contains("2"));
    }

    #[test]
    fn context_anchor_missing_errors() {
        let src = "fn a() {}\n";
        assert!(resolve_context_anchor(src, "no such block").is_err());
    }

    // ---- post-edit syntax verification (code feature only) -------------

    #[cfg(feature = "code")]
    #[test]
    fn valid_rust_passes_syntax_check() {
        assert!(verify_syntax("fn a() { let x = 1; }\n", "rs").is_ok());
    }

    #[cfg(feature = "code")]
    #[test]
    fn broken_rust_fails_syntax_check() {
        // Unbalanced brace — tree-sitter produces an ERROR node.
        assert!(verify_syntax("fn a() { let x = 1;\n", "rs").is_err());
    }

    #[cfg(feature = "code")]
    #[test]
    fn unknown_extension_skips_check_returns_ok() {
        // No grammar for this extension → we can't verify, so don't block.
        assert!(verify_syntax("anything at all }{", "zzz").is_ok());
    }

    #[cfg(feature = "code")]
    #[test]
    fn parse_check_covers_many_languages() {
        // Layer 1 must verify ALL bundled grammars, not just rs/py/js/ts/go/java/cs.
        // Valid snippets pass:
        assert!(verify_syntax("def f():\n    return 1\n", "py").is_ok());
        assert!(verify_syntax("package main\nfunc f() {}\n", "go").is_ok());
        assert!(verify_syntax("int main() { return 0; }\n", "c").is_ok());
        assert!(verify_syntax("class A { public: int x; };\n", "cpp").is_ok());
        assert!(verify_syntax("def m\n  1\nend\n", "rb").is_ok());
        assert!(verify_syntax("echo hello\n", "sh").is_ok());
        assert!(verify_syntax("{\"a\": 1}\n", "json").is_ok());
    }

    #[cfg(feature = "code")]
    #[test]
    fn parse_check_rejects_broken_in_more_languages() {
        // Broken snippets are caught for newly-wired languages:
        assert!(verify_syntax("int main( { return 0; }\n", "c").is_err());      // bad paren
        assert!(verify_syntax("{\"a\": }\n", "json").is_err());                 // bad json
    }

    // ---- anchor drift detection (recall correctness) -------------------

    #[test]
    fn fresh_anchor_when_disk_matches_index() {
        let indexed = "fn target() {\n    do_thing();\n}";
        let ondisk = "fn target() {\n    do_thing();\n}";
        assert!(check_symbol_fresh(indexed, ondisk).is_ok());
    }

    #[test]
    fn fresh_anchor_tolerates_whitespace_only_difference() {
        // Trailing whitespace / CRLF vs LF shouldn't count as drift.
        let indexed = "fn target() {\n    do_thing();\n}";
        let ondisk = "fn target() {\r\n    do_thing();  \r\n}";
        assert!(check_symbol_fresh(indexed, ondisk).is_ok());
    }

    #[test]
    fn auto_indent_applies_base_indent_to_each_line() {
        // Content authored at column 0; reindent to a 4-space base.
        let content = "public void Added()\n{\n    var y = 2;\n}";
        let out = reindent_block(content, "    ");
        assert_eq!(out, "    public void Added()\n    {\n        var y = 2;\n    }");
    }

    #[test]
    fn auto_indent_noop_when_already_indented_to_base() {
        // A single line already at the right indent stays put (no double-indent).
        let out = reindent_block("public void X() { }", "    ");
        assert_eq!(out, "    public void X() { }");
    }

    #[test]
    fn indent_of_reads_leading_whitespace() {
        assert_eq!(indent_of("    public void M()"), "    ");
        assert_eq!(indent_of("\t\tcode"), "\t\t");
        assert_eq!(indent_of("no_indent"), "");
    }

    #[test]
    fn stale_anchor_when_disk_diverged_errors() {
        // The function at this range was changed on disk since indexing.
        let indexed = "fn target() {\n    do_thing();\n}";
        let ondisk = "fn target() {\n    do_something_completely_different();\n}";
        let err = check_symbol_fresh(indexed, ondisk).unwrap_err();
        assert!(err.to_lowercase().contains("stale") || err.to_lowercase().contains("reindex"));
    }

    // ---- transactional multi-edit (all-or-nothing) --------------------

    #[test]
    fn apply_all_applies_every_op_in_sequence() {
        let src = "line1\nANCHOR_A\nline3\nANCHOR_B\n";
        let ops = vec![
            EditOp::InsertAfterLine { line: 2, text: "added_after_A".to_string() },
            EditOp::ReplaceSubstring { needle: "ANCHOR_B".to_string(), replacement: "REPLACED_B".to_string() },
        ];
        let out = apply_all(src, &ops).unwrap();
        assert!(out.contains("added_after_A"));
        assert!(out.contains("REPLACED_B"));
        assert!(out.contains("line1") && out.contains("line3"));
    }

    #[test]
    fn apply_all_rolls_back_if_any_op_fails() {
        let src = "only\ntwo\n";
        let ops = vec![
            EditOp::InsertAfterLine { line: 1, text: "ok_insert".to_string() }, // valid
            EditOp::InsertAfterLine { line: 999, text: "boom".to_string() },     // out of range → fails
        ];
        let result = apply_all(src, &ops);
        assert!(result.is_err(), "the set must fail as a whole");
        // apply_all returns Err on any failure, so the caller writes nothing
        // (no partial application reaches disk). Confirm it carries the failure.
        let err = result.unwrap_err();
        assert!(err.contains("999") || err.to_lowercase().contains("range"));
    }
}

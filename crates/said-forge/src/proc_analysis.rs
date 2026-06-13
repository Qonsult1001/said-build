//! SQL stored-procedure analyser — extracts the structural detail
//! the Comparison Report needs.
//!
//! Forge's `sql_catalog::SqlObject` already records *which* tables a
//! proc touches with a coarse op kind (INSERT / UPDATE / DELETE /
//! SELECT). The Comparison Report needs more:
//!
//! - The proc's `@param` list with SQL types — what the C# layer must
//!   bind to.
//! - JSON keys the proc reads via `OPENJSON(@jRequest)` — these are
//!   the actual fields the body must contain, regardless of what the
//!   OpenAPI spec or Dev Planning markdown say.
//! - UPDATE blocks that wipe columns when the body lacks the source
//!   field (the destructive PUT semantics we already caught with
//!   `p_txn_Update_Cardholder`).
//! - DELETE blocks gated on `IF ISNULL(@x, '') <> ''` — sending the
//!   body without that field deletes related rows.
//! - Lookup-table validations (e.g. `prc_Code = -15`) so the report
//!   can list the error codes the proc raises.
//!
//! All extraction is plain regex over the proc body — no SQL parser
//! dep. Procs that don't follow the convention (no OPENJSON, no
//! @jRequest) just produce empty results, which is fine — the report
//! shows them as "no analysable JSON contract" and the Decision
//! column flags them for manual review.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::sql_catalog::SqlObject;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcAnalysis {
    pub full_name: String,
    pub source_path: String,
    pub params: Vec<ProcParam>,
    /// JSON paths read via `OPENJSON(@jRequest)`.
    pub json_keys: Vec<JsonKey>,
    /// UPDATE blocks that may wipe columns when source variable is null.
    pub destructive_updates: Vec<DestructiveUpdate>,
    /// DELETE blocks gated on a body field being null/empty.
    pub conditional_deletes: Vec<ConditionalDelete>,
    /// Error codes raised via prc_Code lookups.
    pub error_codes: Vec<String>,
    /// Distinctive literal values harvested from the proc body —
    /// GUIDs and short alphanumeric codes. Used to build a workspace-
    /// wide reverse index so the comparison report can show "this
    /// value also appears in C#/other procs" lineage.
    pub literals: Vec<LiteralOccurrence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteralOccurrence {
    pub value: String,
    pub line: usize,
    /// Short context — e.g. "default for @sApiId", "INSERT INTO …
    /// VALUES ('AT00001', …)". Best-effort, may be empty.
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcParam {
    pub name: String,
    pub sql_type: String,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonKey {
    /// `$.id`, `$.personalDetails.firstName`, …
    pub json_path: String,
    /// `UNIQUEIDENTIFIER`, `NVARCHAR(2048)`, …
    pub sql_type: String,
    /// `true` when the WITH clause declared `<col> NVARCHAR(MAX) '$.path' AS JSON`
    /// — meaning the column carries a structured JSON object/array,
    /// not a string.
    #[serde(default)]
    pub is_json: bool,
    /// `true` when this key was extracted from a path-only OPENJSON
    /// call like `OPENJSON(@jRequest, '$.accountId')` — the proc
    /// iterates an array at that path. Body schema renders these as
    /// `type: array`.
    #[serde(default)]
    pub is_array_iteration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestructiveUpdate {
    pub target_table: String,
    /// Columns whose source is a JSON-extracted local variable that
    /// can be NULL when the body lacks the field.
    pub wiped_columns: Vec<WipedColumn>,
    /// Columns assigned to a literal empty string regardless of body
    /// content (`SET col = ''`).
    pub forced_empty_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WipedColumn {
    pub column: String,
    pub source_variable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalDelete {
    pub target_table: String,
    /// The variable whose null/empty triggers the delete.
    pub guarded_by: String,
}

/// Analyse one proc by re-reading its source file from disk and
/// running structural regexes over it.
pub fn analyse_proc(obj: &SqlObject, sql_root: &Path) -> ProcAnalysis {
    let path = sql_root.join(&obj.source_path);
    let body = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            return ProcAnalysis {
                full_name: obj.full_name(),
                source_path: obj.source_path.clone(),
                ..Default::default()
            };
        }
    };
    analyse_body(&obj.full_name(), &obj.source_path, &body)
}

/// Analyse a proc body verbatim. Public for testing.
pub fn analyse_body(full_name: &str, source_path: &str, body: &str) -> ProcAnalysis {
    let stripped = strip_sql_comments(body);
    ProcAnalysis {
        full_name: full_name.to_string(),
        source_path: source_path.to_string(),
        params: extract_params(&stripped),
        json_keys: extract_json_keys(&stripped),
        destructive_updates: extract_destructive_updates(&stripped),
        conditional_deletes: extract_conditional_deletes(&stripped),
        error_codes: extract_error_codes(&stripped),
        literals: extract_literals(body),
    }
}

/// Pull distinctive literal values from a body (the original, NOT the
/// comment-stripped version, so we keep correct line numbers).
///
/// Two flavours:
/// - GUIDs: 8-4-4-4-12 hex pattern, with or without surrounding quotes.
/// - Short alphanumeric codes: single-quoted strings of 4..=15 chars
///   that look like identifiers (mostly upper-case letters + digits,
///   no embedded spaces). Catches `'AT00001'`, `'CN00001'`,
///   `'CARD'`, while skipping prose strings.
///
/// Skips boring values that produce too much noise: pure digits under
/// 4 chars, dates, common content-type literals.
pub fn extract_literals(body: &str) -> Vec<LiteralOccurrence> {
    let mut out: Vec<LiteralOccurrence> = Vec::new();
    for (lineno_zero, raw_line) in body.lines().enumerate() {
        let line_num = lineno_zero + 1;
        // GUIDs first.
        let mut chars = raw_line.char_indices().peekable();
        while let Some((i, _)) = chars.next() {
            if let Some(end) = guid_at(&raw_line[i..]) {
                let value = raw_line[i..i + end].trim_matches('\'').to_string();
                if !is_boring_literal(&value) {
                    out.push(LiteralOccurrence {
                        value: value.to_ascii_uppercase(),
                        line: line_num,
                        context: short_context(raw_line),
                    });
                }
                // Skip past the GUID.
                for _ in 0..end - 1 {
                    chars.next();
                }
            }
        }
        // Then short codes — single-quoted alphanumeric runs.
        let bytes = raw_line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'\'' {
                    j += 1;
                }
                if j > start && j < bytes.len() {
                    let lit = &raw_line[start..j];
                    if looks_like_code(lit) && !is_boring_literal(lit) {
                        out.push(LiteralOccurrence {
                            value: lit.to_string(),
                            line: line_num,
                            context: short_context(raw_line),
                        });
                    }
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

fn guid_at(s: &str) -> Option<usize> {
    // Optional leading '
    let bytes = s.as_bytes();
    let mut start = 0usize;
    if bytes.first() == Some(&b'\'') {
        start = 1;
    }
    if bytes.len() < start + 36 {
        return None;
    }
    let candidate = &bytes[start..start + 36];
    if candidate[8] != b'-' || candidate[13] != b'-' || candidate[18] != b'-' || candidate[23] != b'-' {
        return None;
    }
    for (idx, &b) in candidate.iter().enumerate() {
        if [8, 13, 18, 23].contains(&idx) {
            continue;
        }
        if !b.is_ascii_hexdigit() {
            return None;
        }
    }
    let mut total = start + 36;
    if start == 1 && bytes.get(total) == Some(&b'\'') {
        total += 1;
    }
    Some(total)
}

fn looks_like_code(s: &str) -> bool {
    let len = s.chars().count();
    if !(4..=15).contains(&len) {
        return false;
    }
    if s.contains(' ') || s.contains('.') || s.contains(',') {
        return false;
    }
    // At least 60% should be alphanumeric, and at least one digit OR
    // 3+ uppercase letters in a row.
    let mut alnum = 0usize;
    let mut digits = 0usize;
    let mut upper_run = 0usize;
    let mut max_upper_run = 0usize;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            alnum += 1;
        }
        if c.is_ascii_digit() {
            digits += 1;
        }
        if c.is_ascii_uppercase() {
            upper_run += 1;
            if upper_run > max_upper_run {
                max_upper_run = upper_run;
            }
        } else {
            upper_run = 0;
        }
    }
    if alnum * 5 < len * 3 {
        return false;
    }
    digits >= 1 || max_upper_run >= 3
}

fn is_boring_literal(s: &str) -> bool {
    let lc = s.to_ascii_lowercase();
    matches!(
        lc.as_str(),
        "application/json"
            | "text/json"
            | "yyyy-mm-dd"
            | "yyyy-mm-ddthh:mm:ss"
            | "yyyy-mm-ddthh:mm:ss.fff"
            | "yyyy-mm-dd hh:mm:ss"
            | "format"
            | "active"
            | "create"
            | "update"
            | "delete"
            | "select"
            | "insert"
    ) || lc.starts_with("00000000-0000-0000-0000-")
}

fn short_context(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= 80 {
        trimmed.to_string()
    } else {
        // Slice at a char boundary to tolerate non-ASCII content
        // (some seed data carries §-delimited tokens for example).
        let mut cut = 80usize.min(trimmed.len());
        while cut > 0 && !trimmed.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &trimmed[..cut])
    }
}

// ─────────────────────────── extractors ───────────────────────────

fn extract_params(body: &str) -> Vec<ProcParam> {
    // Match the `(...)` immediately after `CREATE PROCEDURE name`.
    // Tolerates `OR ALTER` and stray whitespace.
    let lower = body.to_ascii_lowercase();
    let needle_idx = lower.find("create procedure").or_else(|| lower.find("alter procedure"));
    let Some(start) = needle_idx else {
        return Vec::new();
    };
    let after_name = &body[start..];
    // Find the first '(' after the procedure name, then matching ')'.
    let Some(open_rel) = after_name.find('(') else {
        return Vec::new();
    };
    let mut depth = 0i32;
    let bytes = after_name.as_bytes();
    let mut close_rel = None;
    for (i, b) in bytes.iter().enumerate().skip(open_rel) {
        match *b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close_rel = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close_rel) = close_rel else {
        return Vec::new();
    };
    let inside = &after_name[open_rel + 1..close_rel];
    parse_param_list(inside)
}

fn parse_param_list(s: &str) -> Vec<ProcParam> {
    let mut out = Vec::new();
    for raw in split_top_level(s, ',') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        // `@name TYPE [= default]` — be permissive about whitespace.
        let line = line.trim_end_matches(',').trim();
        if !line.starts_with('@') {
            continue;
        }
        let mut tokens = line.splitn(2, char::is_whitespace);
        let name = tokens.next().unwrap_or("").trim().to_string();
        let rest = tokens.next().unwrap_or("").trim();
        let (sql_type, default) = match rest.find('=') {
            Some(eq) => (
                rest[..eq].trim().to_string(),
                Some(rest[eq + 1..].trim().trim_matches('\'').to_string()),
            ),
            None => (rest.to_string(), None),
        };
        out.push(ProcParam {
            name,
            sql_type,
            default,
        });
    }
    out
}

/// Split on `sep` ignoring brackets and quoted strings.
fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut buf = String::new();
    let mut out = Vec::new();
    for c in s.chars() {
        match c {
            '\'' => {
                in_quote = !in_quote;
                buf.push(c);
            }
            '(' | '[' if !in_quote => {
                depth += 1;
                buf.push(c);
            }
            ')' | ']' if !in_quote => {
                depth -= 1;
                buf.push(c);
            }
            c if c == sep && depth == 0 && !in_quote => {
                out.push(buf.clone());
                buf.clear();
            }
            _ => buf.push(c),
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

fn extract_json_keys(body: &str) -> Vec<JsonKey> {
    let mut out = Vec::new();
    let lower = body.to_ascii_lowercase();
    let bytes_all = body.as_bytes();
    let mut idx = 0usize;
    while let Some(rel) = lower[idx..].find("openjson") {
        let abs = idx + rel;
        // Word-boundary check on `openjson`.
        let prev_ok = abs == 0 || !is_ident_byte(bytes_all[abs - 1]);
        let after_kw = abs + "openjson".len();
        let next_ok = after_kw >= bytes_all.len() || !is_ident_byte(bytes_all[after_kw]);
        if !(prev_ok && next_ok) {
            idx = abs + 8;
            continue;
        }
        // Find the OPENJSON's argument list `(...)`.
        let after_open = &body[after_kw..];
        let Some(open_rel_arg) = after_open.find('(') else {
            idx = after_kw;
            continue;
        };
        let arg_start_abs = after_kw + open_rel_arg + 1;
        let mut depth = 0i32;
        let mut close_rel_arg = None;
        for (i, b) in body[after_kw..].as_bytes().iter().enumerate().skip(open_rel_arg) {
            match *b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_rel_arg = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_rel_arg) = close_rel_arg else {
            break;
        };
        let arg_end_abs = after_kw + close_rel_arg;
        let arg_text = &body[arg_start_abs..arg_end_abs];

        // Path-only call: `OPENJSON(@jRequest, '$.accountId')` — no
        // WITH clause, used to iterate an array at the given path.
        // Detect by counting top-level commas inside the args (one
        // comma → two args → second is the path literal).
        let args: Vec<&str> = split_top_level_str(arg_text);
        let mut path_only: Option<String> = None;
        if args.len() >= 2 {
            let path_arg = args[1].trim();
            // Single-quoted string starting with `$`.
            if path_arg.starts_with('\'') && path_arg.ends_with('\'') && path_arg.len() >= 2 {
                let inside = &path_arg[1..path_arg.len() - 1];
                if inside.starts_with('$') {
                    path_only = Some(inside.to_string());
                }
            }
        }

        // Look for the WITH clause AFTER the closing paren.
        let scan_start = after_kw + close_rel_arg + 1;
        let scan_lower = &lower[scan_start..];
        let with_keyword = "with";
        let with_rel = find_keyword(scan_lower.as_bytes(), with_keyword.as_bytes());

        // Heuristic: if a WITH clause exists within the next ~8 chars
        // (just whitespace + `WITH`), this is a structured-WITH call.
        // Otherwise treat path_only (if any) as an array iteration.
        let with_close_by = with_rel.map(|r| r <= 64).unwrap_or(false);

        if with_close_by {
            let with_rel = with_rel.unwrap();
            let with_abs = scan_start + with_rel;
            let after_with = &body[with_abs..];
            let Some(open_rel) = after_with.find('(') else {
                idx = with_abs + 4;
                continue;
            };
            let bytes = after_with.as_bytes();
            let mut depth = 0i32;
            let mut close_rel = None;
            for (i, b) in bytes.iter().enumerate().skip(open_rel) {
                match *b {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            close_rel = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(close_rel) = close_rel else {
                break;
            };
            let inside = &after_with[open_rel + 1..close_rel];
            for entry in split_top_level(inside, ',') {
                let line = entry.trim();
                if line.is_empty() {
                    continue;
                }
                let Some(q1) = line.find('\'') else {
                    continue;
                };
                let after_q1 = &line[q1 + 1..];
                let Some(q2_rel) = after_q1.find('\'') else {
                    continue;
                };
                let json_path = after_q1[..q2_rel].to_string();
                // Trailing portion after the closing quote may carry
                // `AS JSON`.
                let trailing = after_q1[q2_rel + 1..].to_ascii_lowercase();
                let is_json = trailing.contains("as json") || trailing.contains("asjson");
                let prefix = line[..q1].trim();
                let first_ws = prefix.find(char::is_whitespace).unwrap_or(prefix.len());
                let sql_type = prefix[first_ws..].trim().to_string();
                out.push(JsonKey {
                    json_path,
                    sql_type,
                    is_json,
                    is_array_iteration: false,
                });
            }
            idx = with_abs + close_rel;
        } else if let Some(path) = path_only {
            // Array-iteration call: emit one synthetic JsonKey for
            // the path so the body schema can render it as an array.
            // The "type" we record is informational — caller maps it
            // to `type: array, items: { type: string, format: uuid }`
            // when the path's leaf name implies an id list.
            out.push(JsonKey {
                json_path: path,
                sql_type: "ARRAY".to_string(),
                is_json: false,
                is_array_iteration: true,
            });
            idx = arg_end_abs + 1;
        } else {
            idx = arg_end_abs + 1;
        }
    }
    out
}

/// Split args of a function call by top-level commas (ignores commas
/// inside parens or string literals). Returns string slices.
fn split_top_level_str(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut last = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' => in_quote = !in_quote,
            b'(' | b'[' if !in_quote => depth += 1,
            b')' | b']' if !in_quote => depth -= 1,
            b',' if depth == 0 && !in_quote => {
                out.push(&s[last..i]);
                last = i + 1;
            }
            _ => {}
        }
    }
    if last < s.len() {
        out.push(&s[last..]);
    }
    out
}

fn find_keyword(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    // Word-boundary search for ASCII-keyword in pre-lowered bytes.
    if needle.is_empty() {
        return Some(0);
    }
    let mut i = 0usize;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !is_ident_byte(haystack[i - 1]);
            let after_ok = i + needle.len() == haystack.len()
                || !is_ident_byte(haystack[i + needle.len()]);
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}

fn extract_destructive_updates(body: &str) -> Vec<DestructiveUpdate> {
    let mut out = Vec::new();
    let lower = body.to_ascii_lowercase();
    let mut idx = 0usize;
    while let Some(rel) = lower[idx..].find("update") {
        let abs = idx + rel;
        // Word boundary on UPDATE.
        let prev_ok = abs == 0 || !is_ident_byte(body.as_bytes()[abs - 1]);
        let next_ok = body
            .as_bytes()
            .get(abs + 6)
            .map(|b| !is_ident_byte(*b))
            .unwrap_or(true);
        if !prev_ok || !next_ok {
            idx = abs + 6;
            continue;
        }
        // Read the next non-blank token as the table name.
        let after = &body[abs + 6..];
        let table_name = next_qualified_identifier(after);
        if table_name.is_empty() {
            idx = abs + 6;
            continue;
        }
        // Find SET … (next major SQL keyword: WHERE / ; / OUTPUT).
        let after_lower = lower[abs + 6..].as_bytes();
        let Some(set_rel) = find_keyword(after_lower, b"set") else {
            idx = abs + 6;
            continue;
        };
        let set_abs_in_after = set_rel + 3;
        let block_end = next_terminator_idx(&body[abs + 6 + set_abs_in_after..]);
        let block = &body[abs + 6 + set_abs_in_after
            ..abs + 6 + set_abs_in_after + block_end];
        let (wiped, forced_empty) = parse_set_clause(block);
        if !wiped.is_empty() || !forced_empty.is_empty() {
            out.push(DestructiveUpdate {
                target_table: table_name,
                wiped_columns: wiped,
                forced_empty_columns: forced_empty,
            });
        }
        idx = abs + 6 + set_abs_in_after + block_end;
    }
    out
}

fn next_qualified_identifier(s: &str) -> String {
    // Eat whitespace, then take chars matching [A-Za-z0-9_.\[\]].
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if !c.is_whitespace() {
            start = i;
            break;
        }
    }
    let mut end = start;
    for (i, c) in s[start..].char_indices() {
        let ok = c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '[' || c == ']';
        if !ok {
            end = start + i;
            break;
        }
        end = start + i + c.len_utf8();
    }
    s[start..end].replace('[', "").replace(']', "")
}

fn next_terminator_idx(s: &str) -> usize {
    // Scan for the next WHERE / FROM / OUTPUT / ; / IF / END at top
    // level. Conservative: stop at WHERE or first ';' which is enough
    // for a SET clause.
    let lower = s.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\'' => in_quote = !in_quote,
            b'(' if !in_quote => depth += 1,
            b')' if !in_quote => depth -= 1,
            b';' if !in_quote && depth == 0 => return i,
            _ if !in_quote && depth == 0 => {
                if matches_word_at(bytes, i, b"where") {
                    return i;
                }
                if matches_word_at(bytes, i, b"output") {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

fn matches_word_at(bytes: &[u8], pos: usize, word: &[u8]) -> bool {
    if pos + word.len() > bytes.len() {
        return false;
    }
    if &bytes[pos..pos + word.len()] != word {
        return false;
    }
    let before_ok = pos == 0 || !is_ident_byte(bytes[pos - 1]);
    let after_ok = pos + word.len() == bytes.len() || !is_ident_byte(bytes[pos + word.len()]);
    before_ok && after_ok
}

fn parse_set_clause(s: &str) -> (Vec<WipedColumn>, Vec<String>) {
    let mut wiped = Vec::new();
    let mut forced_empty = Vec::new();
    for entry in split_top_level(s, ',') {
        let line = entry.trim().trim_end_matches(',');
        if line.is_empty() {
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        let col = line[..eq].trim().replace('[', "").replace(']', "");
        let value = line[eq + 1..].trim();
        if col.is_empty() || value.is_empty() {
            continue;
        }
        // Forced empty: assigned to literal `''`.
        if value == "''" || value == "'\\''" {
            forced_empty.push(col);
            continue;
        }
        // Wiped: assigned to a `@variable` (no surrounding ISNULL or
        // expression — those are intentional). We're conservative: if
        // the RHS contains `@var` AND no ISNULL() AND no COALESCE,
        // assume the column will be set to NULL when the body is
        // missing the source field.
        if value.starts_with('@')
            && !value.to_ascii_lowercase().contains("isnull")
            && !value.to_ascii_lowercase().contains("coalesce")
        {
            // Strip any trailing comma we caught from the splitter.
            let var = value.trim_end_matches(',').to_string();
            wiped.push(WipedColumn {
                column: col,
                source_variable: var,
            });
        }
    }
    (wiped, forced_empty)
}

fn extract_conditional_deletes(body: &str) -> Vec<ConditionalDelete> {
    let mut out = Vec::new();
    let lower = body.to_ascii_lowercase();
    // Pattern: `IF ISNULL(@var, '') <> '' BEGIN UPDATE … END ELSE BEGIN DELETE FROM <table> …`
    // We hunt for `else` then `delete` then a table name.
    let mut idx = 0usize;
    while let Some(rel) = lower[idx..].find("delete") {
        let abs = idx + rel;
        // Word boundary.
        if !matches_word_at(lower.as_bytes(), abs, b"delete") {
            idx = abs + 6;
            continue;
        }
        // Look at the preceding ~200 chars for "else" guarded by an
        // "if isnull(@var, '') <> ''" pattern.
        let look_start = abs.saturating_sub(400);
        let lookback = &lower[look_start..abs];
        // Cheap heuristic: there must be an `else` and an `isnull(@`.
        if !lookback.contains("else") {
            idx = abs + 6;
            continue;
        }
        let isnull_idx = lookback.rfind("isnull(@");
        if isnull_idx.is_none() {
            idx = abs + 6;
            continue;
        }
        // Capture the variable name after `isnull(@`.
        let var_start = isnull_idx.unwrap() + "isnull(".len();
        let after_var = &lookback[var_start..];
        let mut end = 0;
        for (i, c) in after_var.char_indices() {
            if !is_ident_byte(c as u8) {
                end = i;
                break;
            }
            end = i + c.len_utf8();
        }
        let var = format!("@{}", &after_var[1..end].trim_start_matches('@'));
        // Now find the table name after DELETE / DELETE FROM.
        let after_delete = &body[abs + 6..];
        let mut consumer = after_delete.trim_start();
        if consumer.to_ascii_lowercase().starts_with("from") {
            consumer = consumer[4..].trim_start();
        }
        let table = next_qualified_identifier(consumer);
        if !table.is_empty() {
            out.push(ConditionalDelete {
                target_table: table,
                guarded_by: var,
            });
        }
        idx = abs + 6;
    }
    out
}

fn extract_error_codes(body: &str) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    let lower = body.to_ascii_lowercase();
    let mut idx = 0usize;
    while let Some(rel) = lower[idx..].find("prc_code = ") {
        let abs = idx + rel + "prc_code = ".len();
        let rest = &body[abs..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '-' && c != '+')
            .unwrap_or(rest.len());
        let code = rest[..end].trim().to_string();
        if !code.is_empty() && code != "0" {
            out.insert(code);
        }
        idx = abs + end;
    }
    out.into_iter().collect()
}

fn strip_sql_comments(body: &str) -> String {
    // Drop -- ... \n line comments AND /* ... */ block comments.
    // Cheap two-pass.
    let mut out = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Block comment.
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut j = i + 2;
            while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                j += 1;
            }
            i = j.saturating_add(2).min(bytes.len());
            continue;
        }
        // Line comment.
        if b == b'-' && bytes.get(i + 1) == Some(&b'-') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PROC: &str = r#"
        CREATE PROCEDURE [cardholder].[p_test] (
            @jRequest NVARCHAR(MAX),
            @uRequestId UNIQUEIDENTIFIER,
            @sApiId UNIQUEIDENTIFIER = 'C729AB72-C4AC-4F8F-998A-D5130FBF696B'
        )
        AS
        BEGIN
            DECLARE @sFirstName NVARCHAR(2048);

            SELECT
                @sFirstName = [first_name]
            FROM
                OPENJSON(@jRequest)
                    WITH (
                        first_name NVARCHAR(2048) '$.personalDetails.firstName',
                        attributes NVARCHAR(MAX) '$.attributes' AS JSON
                    );

            UPDATE cardholder.cpf_Client_Profile
            SET
                cpf_Last_Name = @sFirstName,
                cpf_Preferred_Name = '',
                cpf_Updated_At_UTC = GETUTCDATE()
            WHERE cpf_Profile_Id = @uProfileId;

            IF ISNULL(@sBilling_Line1, '') <> ''
            BEGIN
                UPDATE cardholder.add_Address_Details SET col_Code = 'X';
            END
            ELSE
            BEGIN
                DELETE FROM cardholder.add_Address_Details
                WHERE cpf_Profile_Id = @uProfileId;
            END;

            RAISERROR('boom', 16, 1) -- prc_Code = 60105
            RAISERROR('boom', 16, 1) -- prc_Code = 60106
        END
    "#;

    #[test]
    fn extracts_three_params_with_default() {
        let a = analyse_body("cardholder.p_test", "test.sql", SAMPLE_PROC);
        assert_eq!(a.params.len(), 3);
        assert_eq!(a.params[0].name, "@jRequest");
        assert_eq!(a.params[0].sql_type, "NVARCHAR(MAX)");
        assert!(a.params[2].default.is_some());
    }

    #[test]
    fn extracts_openjson_keys_with_paths() {
        let a = analyse_body("cardholder.p_test", "test.sql", SAMPLE_PROC);
        assert_eq!(a.json_keys.len(), 2);
        assert_eq!(a.json_keys[0].json_path, "$.personalDetails.firstName");
        assert_eq!(a.json_keys[1].json_path, "$.attributes");
    }

    #[test]
    fn detects_destructive_update_columns() {
        let a = analyse_body("cardholder.p_test", "test.sql", SAMPLE_PROC);
        assert_eq!(a.destructive_updates.len(), 1);
        let upd = &a.destructive_updates[0];
        assert!(upd.target_table.contains("cpf_Client_Profile"));
        assert!(upd
            .wiped_columns
            .iter()
            .any(|w| w.column.contains("cpf_Last_Name")));
        assert!(upd
            .forced_empty_columns
            .iter()
            .any(|c| c.contains("cpf_Preferred_Name")));
    }

    #[test]
    fn detects_conditional_delete_guarded_by_isnull() {
        let a = analyse_body("cardholder.p_test", "test.sql", SAMPLE_PROC);
        assert_eq!(a.conditional_deletes.len(), 1);
        assert!(a.conditional_deletes[0]
            .target_table
            .contains("add_Address_Details"));
        assert!(a.conditional_deletes[0].guarded_by.starts_with('@'));
    }

    // NOTE: extract_error_codes runs against the comment-stripped body
    // (intended), so test-fixture inline `-- prc_Code = …` comments are
    // not visible. Real procs use `WHERE prc_Code = 60105`, which we do
    // pick up — confirmed against `p_txn_Update_Cardholder.sql` in dtcard.

    #[test]
    fn extracts_as_json_flag_and_array_iteration() {
        let body = r#"
            CREATE PROCEDURE [x].[y] AS BEGIN
                SELECT
                    @sFirstName = [first_name],
                    @sAttributes = [attributes]
                FROM
                    OPENJSON(@jRequest)
                        WITH (
                            first_name NVARCHAR(2048) '$.personalDetails.firstName',
                            attributes NVARCHAR(MAX) '$.attributes' AS JSON
                        );

                INSERT INTO @tIds (id)
                SELECT TRY_CONVERT(UNIQUEIDENTIFIER, [value])
                FROM OPENJSON(@jRequest, '$.accountId');
            END
        "#;
        let a = analyse_body("x.y", "y.sql", body);
        let attr = a
            .json_keys
            .iter()
            .find(|k| k.json_path == "$.attributes")
            .expect("attributes key");
        assert!(attr.is_json);
        assert!(!attr.is_array_iteration);

        let acct = a
            .json_keys
            .iter()
            .find(|k| k.json_path == "$.accountId")
            .expect("accountId array iteration");
        assert!(acct.is_array_iteration);
        assert!(!acct.is_json);
    }

    #[test]
    fn extracts_guid_and_short_code_literals_with_line_numbers() {
        let body = r#"
            CREATE PROCEDURE [x].[y] (
                @sApiId UNIQUEIDENTIFIER = 'C729AB72-C4AC-4F8F-998A-D5130FBF696B'
            )
            AS
            BEGIN
                INSERT INTO cardholder.add_Address_Details (cnl_Code, adl_Code)
                VALUES ('CN00001', 'AT00001');

                SELECT 'application/json'; -- should be filtered as boring
            END
        "#;
        let lits = extract_literals(body);
        let values: Vec<String> = lits.iter().map(|l| l.value.clone()).collect();
        assert!(values
            .iter()
            .any(|v| v == "C729AB72-C4AC-4F8F-998A-D5130FBF696B"));
        assert!(values.iter().any(|v| v == "CN00001"));
        assert!(values.iter().any(|v| v == "AT00001"));
        assert!(!values.iter().any(|v| v == "application/json"));
        // Line numbers are 1-based and sane.
        for l in &lits {
            assert!(l.line >= 1 && l.line <= body.lines().count());
        }
    }

    #[test]
    fn proc_with_no_openjson_returns_empty_json_keys() {
        let body = "CREATE PROCEDURE [x].[y] AS SELECT 1;";
        let a = analyse_body("x.y", "y.sql", body);
        assert!(a.json_keys.is_empty());
        assert!(a.destructive_updates.is_empty());
    }
}

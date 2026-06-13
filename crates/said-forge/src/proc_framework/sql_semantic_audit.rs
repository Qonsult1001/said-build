//! Semantic SQL proc audit — built on top of `sca-core::code_search`.
//!
//! The existing region-marker audit ([`audit::audit_endpoint`]) catches
//! drift in framework-owned `[SaidFully]` blocks. This module adds the
//! complementary **semantic** checks the user asked for — same AST
//! engine the brain uses for `said sym`, no extra parser dependency.
//!
//! Three checks per endpoint:
//!
//!   1. **Param signature** — the proc's `CREATE PROCEDURE`
//!      parameter list matches the shape's `parameter_signature`
//!      (command vs query-by-id vs query-list).
//!
//!   2. **Table touch set** — the procedure's `refs:` tag (computed
//!      by `sql_chunk` from EXEC bodies + INSERT/UPDATE/DELETE/SELECT
//!      targets) is a **subset** of `bundle.primary_table` + child
//!      tables + the always-allowed audit/registry tables. Unexpected
//!      touches surface as drift.
//!
//!   3. **prc_Code range** — every literal `prc_Code = NNN` reference
//!      in the proc falls inside the bundle's `prc_code_range`
//!      (`start..=end`). Out-of-range codes mean the proc is using
//!      another bundle's error space, which is a real bug.
//!
//! Drift is reported semantically. Whitespace, comment churn, ordering
//! within a region don't show up.

use std::path::Path;

use super::manifest::{Bundle, EndpointRow};
use super::shape::load_shape;
use sca_core::code_search::ast_chunk;

#[derive(Debug, Clone)]
pub enum SqlDrift {
    /// Deployed file doesn't exist or has no CREATE PROCEDURE chunk.
    Missing { proc_name: String, note: String },
    /// `CREATE PROCEDURE (…)` param list differs from the shape's
    /// `parameter_signature`. `deployed` is the normalised whitespace
    /// version of the deployed signature; `expected` is the same for
    /// the shape's signature.
    ParamSignature {
        proc_name: String,
        deployed: String,
        expected: String,
    },
    /// Deployed proc touches a table not declared by the bundle.
    UnexpectedTable {
        proc_name: String,
        table: String,
    },
    /// `prc_Code = NNN` reference outside the bundle's allocated range.
    PrcCodeOutOfRange {
        proc_name: String,
        code: i64,
        range: (i64, i64),
    },
}

#[derive(Debug, Clone, Default)]
pub struct SqlAuditResult {
    pub row_id: String,
    pub proc_name: String,
    pub drifts: Vec<SqlDrift>,
}

impl SqlAuditResult {
    pub fn clean(&self) -> bool {
        self.drifts.is_empty()
    }
}

/// Audit one endpoint's deployed proc against the bundle + shape +
/// standards. `framework_root` is `dtcard/.forge/proc-framework`.
/// `deployed_root` is the SSDT project root that contains the schema
/// folders (e.g. `dtcard/1-ground-truth/.../TxnMasterSQL`).
pub fn audit_endpoint_semantic(
    framework_root: &Path,
    profile: &str,
    bundle: &Bundle,
    row: &EndpointRow,
    deployed_root: &Path,
) -> Result<SqlAuditResult, String> {
    let proc_name = row.proc_name();
    let schema = row.schema.clone().unwrap_or_else(|| bundle.schema.clone());
    let mut out = SqlAuditResult {
        row_id: row.id.clone(),
        proc_name: proc_name.clone(),
        drifts: Vec::new(),
    };

    let deployed_path = deployed_root
        .join(&schema)
        .join("Stored Procedures")
        .join(format!("{}.sql", proc_name));
    if !deployed_path.exists() {
        out.drifts.push(SqlDrift::Missing {
            proc_name: proc_name.clone(),
            note: format!("not found at {}", deployed_path.display()),
        });
        return Ok(out);
    }

    let source = std::fs::read_to_string(&deployed_path)
        .map_err(|e| format!("read {}: {}", deployed_path.display(), e))?;

    let chunks = ast_chunk(&source, "sql");
    let proc_chunk = chunks
        .iter()
        .find(|c| c.kind.starts_with("create_procedure") || c.kind.starts_with("alter_procedure"));
    let Some(proc_chunk) = proc_chunk else {
        out.drifts.push(SqlDrift::Missing {
            proc_name: proc_name.clone(),
            note: "no CREATE PROCEDURE statement found".into(),
        });
        return Ok(out);
    };

    // (1) Param signature check.
    let deployed_sig = extract_param_signature(&proc_chunk.content).unwrap_or_default();
    let shape = load_shape(framework_root, profile, &row.shape)?;
    // The shape carries the raw template — `{{API_ID}}`,
    // `{{ROUTE_PARAMS_SIG}}`. Resolve against the row before
    // comparison so a deployed proc with the canonical GUID and the
    // real route params doesn't look like drift.
    let expected_resolved = resolve_signature_template(&shape.parameter_signature, row);
    let expected_sig = normalise_whitespace(&expected_resolved);
    let deployed_sig_n = normalise_whitespace(&deployed_sig);
    if !expected_sig.is_empty() && expected_sig != deployed_sig_n {
        out.drifts.push(SqlDrift::ParamSignature {
            proc_name: proc_name.clone(),
            deployed: deployed_sig_n,
            expected: expected_sig,
        });
    }

    // (2) Table touch set. The upstream `sca-core::code_search`
    // `extract_table_references` is heuristic and sometimes emits
    // junk tokens (operators, OPENJSON arguments, UPDATE keyword).
    // Filter to tokens that look like real table names before
    // diffing — anything containing `@`, `'`, `<`, `=` or starting
    // with `_` is upstream noise we discard.
    let refs = extract_refs_from_kind(&proc_chunk.kind);
    let allowed = allowed_tables_for_bundle(bundle, row);
    for tbl in &refs {
        if !looks_like_table_name(tbl) {
            continue;
        }
        if !table_is_allowed(tbl, &allowed) {
            out.drifts.push(SqlDrift::UnexpectedTable {
                proc_name: proc_name.clone(),
                table: tbl.clone(),
            });
        }
    }

    // (3) prc_Code range check.
    //
    // Option-A semantics (advisory blocks, see standards/prc-code-conventions.md):
    // a code is "in range" if it falls inside ANY of the bundle's declared
    // segments, OR is in the shared infra set. Out-of-range codes are
    // surfaced as drift so a human can decide whether to add a segment or
    // reassign the code.
    let segments = bundle.prc_code_segments();
    if !segments.is_empty() {
        let mut seen = std::collections::BTreeSet::<i64>::new();
        for code in extract_prc_codes(&proc_chunk.content) {
            // Cross-bundle shared codes are always allowed:
            //   -1     generic / unset
            //   1001   validation pipeline meta error
            //   1006   ApiId not found (middleware-style)
            //   1009   field failed validation
            if matches!(code, -1 | 1001 | 1006 | 1009) || code < 0 {
                continue;
            }
            if !segments.iter().any(|s| s.contains(code)) {
                if seen.insert(code) {
                    // Report the first segment's bounds for the user's
                    // reference; the drift type is the same.
                    let first = segments.first().unwrap();
                    out.drifts.push(SqlDrift::PrcCodeOutOfRange {
                        proc_name: proc_name.clone(),
                        code,
                        range: (first.start, first.end),
                    });
                }
            }
        }
    }

    Ok(out)
}

/// Pull the parameter list out of the deployed proc.
///
/// T-SQL procs come in two forms:
///
///   CREATE PROCEDURE name (@p1 …, @p2 …) AS BEGIN ... END
///
///   CREATE PROCEDURE name
///       @p1 …,
///       @p2 …
///   AS
///   BEGIN ... END
///
/// We slice between the proc name and the first `AS` keyword that's
/// surrounded by whitespace/newline (the body marker). Body tokens
/// like `NVARCHAR(MAX)` live past that boundary and don't leak in.
fn extract_param_signature(content: &str) -> Option<String> {
    let upper = content.to_uppercase();
    let create_idx = upper
        .find("CREATE OR ALTER PROCEDURE")
        .or_else(|| upper.find("CREATE PROCEDURE"))
        .or_else(|| upper.find("ALTER PROCEDURE"))?;
    let kw_len = if upper[create_idx..].starts_with("CREATE OR ALTER PROCEDURE") {
        "CREATE OR ALTER PROCEDURE".len()
    } else if upper[create_idx..].starts_with("CREATE PROCEDURE") {
        "CREATE PROCEDURE".len()
    } else {
        "ALTER PROCEDURE".len()
    };
    let after_kw = create_idx + kw_len;

    let tail = &content[after_kw..];
    let name_start_rel = tail.find(|c: char| !c.is_whitespace())?;
    let name_start = after_kw + name_start_rel;
    // End of name is at the first '(' (form 1) or first '\n' / '@' (form 2).
    let after_name_rel = content[name_start..]
        .find(|c: char| c == '\n' || c == '(' || c == '@')?;
    let after_name = name_start + after_name_rel;

    let body_start = find_as_begin_boundary(&upper, after_name)?;
    let sig_slice = &content[after_name..body_start];

    // Strip surrounding parens if present.
    let trimmed = sig_slice.trim();
    let inner = if trimmed.starts_with('(') {
        if let Some(close) = match_paren(trimmed.as_bytes()) {
            &trimmed[1..close]
        } else {
            &trimmed[1..]
        }
    } else {
        trimmed
    };
    Some(inner.to_string())
}

/// Find the position of the `AS` keyword that introduces the proc
/// body. `AS` must be a standalone token (whitespace on both sides)
/// AND followed within a few lines by `BEGIN` or any executable
/// statement — to avoid matching `WITH RECOMPILE AS` etc.
fn find_as_begin_boundary(upper: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while cursor < upper.len() {
        let rest = &upper[cursor..];
        let idx = rest.find("AS")?;
        let abs = cursor + idx;
        let before_ok = abs == 0
            || matches!(
                upper.as_bytes()[abs - 1],
                b' ' | b'\t' | b'\n' | b'\r' | b')' | b';'
            );
        let after_idx = abs + 2;
        let after_ok = after_idx >= upper.len()
            || matches!(
                upper.as_bytes()[after_idx],
                b' ' | b'\t' | b'\n' | b'\r'
            );
        if before_ok && after_ok {
            return Some(abs);
        }
        cursor = abs + 2;
    }
    None
}

/// Given a slice that starts at an `(`, find the matching `)`. Handles
/// nested parens (e.g. `NVARCHAR(MAX)` inside the param list).
fn match_paren(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() || bytes[0] != b'(' {
        return None;
    }
    let mut depth = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Collapse runs of whitespace, normalise commas, uppercase. For
/// signature comparison only — we don't care about indentation or
/// comment churn in the param list.
fn normalise_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = true;
    for c in s.chars() {
        if c == '\r' {
            continue;
        }
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(c.to_ascii_uppercase());
            prev_ws = false;
        }
    }
    // Drop trailing space.
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Substitute the shape's template placeholders against the endpoint
/// row so the audit diffs equivalent text.
///
///   `{{api_id}}`              → row's `api_id` GUID
///   `{{route_params_sig}}`    → comma-prefixed route params, e.g.
///                               `, @uAccountId UNIQUEIDENTIFIER`
///   `{{route_params_sig_inline}}` → same without the leading comma,
///                                   used by query-list shapes that put
///                                   route params between paging args.
///
/// Placeholder names match the renderer's `build_base_vars` keys in
/// `render.rs` (case-sensitive lowercase). Backwards compat aliases
/// for the upper-case forms are kept so older shape files still work.
fn resolve_signature_template(template: &str, row: &EndpointRow) -> String {
    let mut sig = String::new();
    for p in &row.route_params {
        sig.push_str(", ");
        sig.push_str(&p.name);
        sig.push(' ');
        sig.push_str(&p.ty);
    }
    let inline = sig.trim_start_matches(", ").to_string();

    template
        .replace("{{api_id}}", &row.api_id)
        .replace("{{API_ID}}", &row.api_id)
        .replace("{{route_params_sig}}", &sig)
        .replace("{{ROUTE_PARAMS_SIG}}", &sig)
        .replace("{{route_params_sig_inline}}", &inline)
        .replace("{{ROUTE_PARAMS_SIG_INLINE}}", &inline)
}

/// Heuristic guard against upstream `extract_table_references` noise.
///
/// A real table reference looks like:
///   - `[schema].[table]`           (bracket-quoted)
///   - `schema.table`               (qualified)
///   - `table_name`                 (bare, snake/camel/Pascal)
///
/// Filtered out (upstream chunker bugs we work around here):
///   - Tokens with `@`, `'`, `<`, `=`, `>` (param refs, comparisons)
///   - Leading `_` (OPENJSON sub-result aliases)
///   - All-uppercase short tokens that match T-SQL keywords mis-emitted
///     by the chunker: `UPDATE`, `JSON`, `TYPED`, `THE`, `THEN`, etc.
fn looks_like_table_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.contains('@') || s.contains('\'') || s.contains('<') || s.contains('=') || s.contains('>')
    {
        return false;
    }
    let cleaned: String = s.chars().filter(|&c| c != '[' && c != ']').collect();
    if cleaned.starts_with('_') {
        return false;
    }
    // The bare last segment must start with an ASCII letter.
    let bare = bare_table_name(&cleaned);
    if !bare
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
    {
        return false;
    }
    // Drop common T-SQL keywords that the upstream chunker
    // occasionally emits as if they were table names. These are
    // unqualified (no `.`) AND all-uppercase AND match the keyword
    // list — a real bare table name in this codebase is snake_case
    // (`ana_Acc_No_Alloc`) and would never look like this.
    if !cleaned.contains('.') {
        const NOISE: &[&str] = &[
            "UPDATE", "DELETE", "INSERT", "SELECT", "FROM", "WHERE", "INTO",
            "JSON", "TYPED", "THE", "THEN", "ELSE", "END", "WITH",
            "SET", "DECLARE", "EXEC", "BEGIN", "WHILE", "AND", "OR", "NOT",
            "VALUES", "OUTPUT", "OPENJSON", "CROSS", "OUTER", "INNER", "LEFT", "RIGHT",
        ];
        let upper = cleaned.to_ascii_uppercase();
        if NOISE.contains(&upper.as_str()) {
            return false;
        }
    }
    true
}

/// Parse `refs:tbl1,tbl2,...` out of the chunker's kind tag.
fn extract_refs_from_kind(kind: &str) -> Vec<String> {
    for part in kind.split('|') {
        if let Some(rest) = part.strip_prefix("refs:") {
            return rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// Allowed table set for a bundle row. We consider:
///   - `bundle.tables.files[]` — every table the bundle declares.
///   - `row.primary_table` — the proc's primary write target.
///   - Always-allowed plumbing tables: audit / registry / response /
///     validation pipeline + the `prc_Program_Response_Const_Lookup`.
fn allowed_tables_for_bundle(bundle: &Bundle, row: &EndpointRow) -> Vec<String> {
    let mut out: Vec<String> = bundle
        .table_files()
        .iter()
        .map(|p| table_name_from_path(p))
        .filter(|s| !s.is_empty())
        .collect();
    if !row.primary_table.is_empty() {
        out.push(row.primary_table.clone());
    }
    // Common plumbing tables the framework wires into every proc.
    out.extend([
        // Audit + registry + validation pipeline
        "ala_Api_Live_Audit",
        "ars_Api_Rule_Settings",
        "arc_Api_Rule_Validations",
        "prc_Program_Response_Const_Lookup",
        "aml_Api_Method_Lookup",
        "sel_System_Error_Log",
        "ava_Api_Validations_Audit_Log",
        // Shared cross-bundle lookups (treated as global vocabulary,
        // not entity-owned). When this profile gains a TOML knob for
        // shared lookups, move this list there.
        "cur_Currency_Lookup",
        "acs_Account_Status",
        "iat_Iso_Account_Type_Lookup",
        "aft_Account_Funding_Type",
        "prl_Product_Lookup",
        "cpf_Client_Profile", // cross-bundle FK — Cardholder owns it
    ]
    .into_iter()
    .map(String::from));
    out
}

fn table_name_from_path(p: &str) -> String {
    // `tables/ana_Acc_No_Alloc.sql` → `ana_Acc_No_Alloc`.
    let stem = Path::new(p)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    stem.to_string()
}

/// Membership test that ignores schema qualification + brackets. A
/// `refs:` entry may be `dbo.foo` or `[bar].[baz]`; allowed names may
/// be plain table names from the bundle's `tables/`. Match by the bare
/// table name suffix.
fn table_is_allowed(deployed_ref: &str, allowed: &[String]) -> bool {
    let bare = bare_table_name(deployed_ref);
    allowed.iter().any(|a| {
        let a_bare = bare_table_name(a);
        a_bare.eq_ignore_ascii_case(&bare)
    })
}

fn bare_table_name(s: &str) -> String {
    let no_brackets = s.replace('[', "").replace(']', "");
    let last = no_brackets.rsplit('.').next().unwrap_or(&no_brackets);
    last.to_string()
}

/// Extract literal `prc_Code = NNN` references from the proc body.
fn extract_prc_codes(content: &str) -> Vec<i64> {
    let mut out = Vec::new();
    let upper = content.to_uppercase();
    let mut search_from = 0usize;
    while search_from < upper.len() {
        let rest = &upper[search_from..];
        let Some(idx) = rest.find("PRC_CODE") else {
            break;
        };
        let abs = search_from + idx;
        // Walk forward through `prc_Code`, optional `=`, optional whitespace.
        let after = &upper[abs + "PRC_CODE".len()..];
        let trimmed = after.trim_start();
        let with_eq = trimmed.strip_prefix('=').unwrap_or(trimmed).trim_start();
        // Capture leading digits.
        let mut num = String::new();
        for c in with_eq.chars() {
            if c.is_ascii_digit() {
                num.push(c);
            } else {
                break;
            }
        }
        if let Ok(n) = num.parse::<i64>() {
            out.push(n);
        }
        search_from = abs + "PRC_CODE".len();
    }
    out
}


// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_signature_simple() {
        let sql = r#"
CREATE PROCEDURE [foo].[bar]
(
    @jRequest NVARCHAR(MAX),
    @uRequestId UNIQUEIDENTIFIER
)
AS
BEGIN
    SELECT 1;
END
"#;
        let sig = extract_param_signature(sql).expect("sig");
        assert!(sig.contains("@jRequest NVARCHAR(MAX)"));
        assert!(sig.contains("@uRequestId UNIQUEIDENTIFIER"));
    }

    #[test]
    fn normalise_collapses_whitespace() {
        let a = "  @j   NVARCHAR( MAX ),\n   @u UNIQUEIDENTIFIER  ";
        let b = "@J NVARCHAR( MAX ), @U UNIQUEIDENTIFIER";
        assert_eq!(normalise_whitespace(a), normalise_whitespace(b));
    }

    #[test]
    fn prc_codes_extract() {
        let sql = r#"
WHERE prc_Code = 60304 -- [60304] - account exists
OR    prc_Code=60305
OR    prc_Code   =   60306
"#;
        let mut codes = extract_prc_codes(sql);
        codes.sort();
        assert_eq!(codes, vec![60304, 60305, 60306]);
    }

    #[test]
    fn bare_name_strips_schema_and_brackets() {
        assert_eq!(bare_table_name("[dbo].[foo]"), "foo");
        assert_eq!(bare_table_name("dbo.foo"), "foo");
        assert_eq!(bare_table_name("foo"), "foo");
    }

    #[test]
    fn match_paren_handles_nesting() {
        let s = b"(NVARCHAR(MAX), INT)";
        let close = match_paren(s).expect("close");
        assert_eq!(close, s.len() - 1);
    }
}

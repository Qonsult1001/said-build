//! SQL object catalog — one entry per CREATE TABLE / PROCEDURE / VIEW /
//! TRIGGER / FUNCTION in the workspace ground-truth.
//!
//! The catalog is the substrate for Phase 19's TechnicalGrounding — given
//! an op and its candidate tables, we can now enumerate:
//! - which procs reference those tables (read / write / mixed)
//! - which views depend on those tables
//! - which triggers fire on those tables
//! - the FK graph (via TableSchema.foreign_keys)
//!
//! Reference extraction is deliberately simple: scan the proc/view/trigger
//! body for `FROM [schema].[table]`, `JOIN [schema].[table]`, `UPDATE
//! [schema].[table]`, `INSERT INTO [schema].[table]`, `DELETE [schema].[table]`.
//! Misses dynamic SQL (EXEC sp_executesql @sql), CTEs that re-alias
//! (usually not an issue for our purposes), and table variables. Good
//! enough for 90% of real-world procs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::schema::{parse_create_table, TableSchema};
use crate::{ForgeError, ForgeResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SqlKind {
    Table,
    Procedure,
    View,
    Trigger,
    Function,
}

/// One SQL object in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlObject {
    pub kind: SqlKind,
    pub schema: Option<String>,
    pub name: String,
    /// Tables this object references in its body (UPDATE / INSERT / DELETE /
    /// SELECT FROM / JOIN). Each entry is `schema.table` form when possible,
    /// or bare `table`.
    pub referenced_tables: Vec<String>,
    /// Operation types observed against each referenced table.
    pub ops: Vec<TableOp>,
    /// Source file for provenance.
    pub source_path: String,
}

impl SqlObject {
    pub fn full_name(&self) -> String {
        match &self.schema {
            Some(s) => format!("{}.{}", s, self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableOp {
    pub table: String,
    pub kinds: Vec<TableOpKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TableOpKind {
    Select,
    Insert,
    Update,
    Delete,
}

/// The whole catalog for a workspace.
#[derive(Debug, Default)]
pub struct SqlCatalog {
    pub tables: Vec<TableSchema>,
    pub objects: Vec<SqlObject>,
}

impl SqlCatalog {
    /// Objects that reference any of the supplied table full-names. Uses
    /// suffix matching so `cpf_Client_Profile` matches both
    /// `cardholder.cpf_Client_Profile` (registered) and `cpf_Client_Profile`
    /// (bare reference in body).
    pub fn objects_referencing(&self, table_full_names: &[String]) -> Vec<&SqlObject> {
        self.objects
            .iter()
            .filter(|o| {
                o.referenced_tables.iter().any(|rt| {
                    table_full_names.iter().any(|t| table_names_match(t, rt))
                })
            })
            .collect()
    }

    pub fn find_table(&self, full_or_bare: &str) -> Option<&TableSchema> {
        self.tables
            .iter()
            .find(|t| table_names_match(&t.full_name(), full_or_bare))
    }
}

fn table_names_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let a_tail = a.rsplit('.').next().unwrap_or(a);
    let b_tail = b.rsplit('.').next().unwrap_or(b);
    a_tail.eq_ignore_ascii_case(b_tail)
}

// ─────────────────────────── catalog build ───────────────────────────

pub fn build_catalog(workspace_root: &Path) -> ForgeResult<SqlCatalog> {
    let ground_truth = workspace_root.join("1-ground-truth");
    if !ground_truth.is_dir() {
        return Ok(SqlCatalog::default());
    }
    let mut cat = SqlCatalog::default();
    let mut stack = vec![ground_truth];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| ForgeError::Io {
            path: dir.display().to_string(),
            cause: e,
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| ForgeError::Io {
                path: dir.display().to_string(),
                cause: e,
            })?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)
                != Some("sql".into())
            {
                continue;
            }
            let content = match std::fs::read_to_string(&p) {
                Ok(c) => c,
                Err(_) => continue,
            };
            classify_and_absorb(&p, &content, &mut cat);
        }
    }
    Ok(cat)
}

fn classify_and_absorb(path: &Path, src: &str, cat: &mut SqlCatalog) {
    for batch in split_go_batches(src) {
        let stripped = strip_comments(batch);
        let no_bom = stripped.trim_start_matches('\u{FEFF}').trim_start();
        let upper = no_bom.to_uppercase();

        if upper.starts_with("CREATE TABLE") {
            if let Some(t) = parse_create_table(batch) {
                cat.tables.push(t);
            }
            continue;
        }
        let (kind, name_start) = if upper.starts_with("CREATE PROCEDURE")
            || upper.starts_with("CREATE OR ALTER PROCEDURE")
            || upper.starts_with("ALTER PROCEDURE")
        {
            (SqlKind::Procedure, find_name_start(no_bom, &["PROCEDURE", "PROC"]))
        } else if upper.starts_with("CREATE PROC") || upper.starts_with("ALTER PROC") {
            (SqlKind::Procedure, find_name_start(no_bom, &["PROC"]))
        } else if upper.starts_with("CREATE VIEW") || upper.starts_with("CREATE OR ALTER VIEW") {
            (SqlKind::View, find_name_start(no_bom, &["VIEW"]))
        } else if upper.starts_with("CREATE TRIGGER") || upper.starts_with("CREATE OR ALTER TRIGGER") {
            (SqlKind::Trigger, find_name_start(no_bom, &["TRIGGER"]))
        } else if upper.starts_with("CREATE FUNCTION") || upper.starts_with("CREATE OR ALTER FUNCTION") {
            (SqlKind::Function, find_name_start(no_bom, &["FUNCTION"]))
        } else {
            continue;
        };
        let Some(name_start) = name_start else {
            continue;
        };
        let (schema, name) = extract_qualified_name(&no_bom[name_start..]);
        if name.is_empty() {
            continue;
        }
        let (referenced_tables, ops) = extract_table_references(&stripped);
        cat.objects.push(SqlObject {
            kind,
            schema,
            name,
            referenced_tables,
            ops,
            source_path: path.to_string_lossy().to_string(),
        });
    }
}

/// Find the byte index of the first character after the first matching
/// keyword (one of `keywords`) — i.e. the start of the object name.
fn find_name_start(s: &str, keywords: &[&str]) -> Option<usize> {
    let upper = s.to_uppercase();
    for kw in keywords {
        if let Some(idx) = upper.find(kw) {
            return Some(idx + kw.len());
        }
    }
    None
}

fn extract_qualified_name(rest: &str) -> (Option<String>, String) {
    // Skip whitespace
    let s = rest.trim_start();
    // Take up to whitespace or `(` or `;` or `\n`
    let end = s
        .find(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .unwrap_or(s.len());
    let raw = &s[..end];
    let parts: Vec<String> = raw
        .split('.')
        .map(|p| p.trim_start_matches('[').trim_end_matches(']').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    match parts.len() {
        2 => (Some(parts[0].clone()), parts[1].clone()),
        1 => (None, parts[0].clone()),
        _ => (None, String::new()),
    }
}

/// Scan body for `FROM x`, `JOIN x`, `UPDATE x`, `INSERT INTO x`,
/// `DELETE x` / `DELETE FROM x`. Returns deduplicated table list + per-table
/// op kinds.
fn extract_table_references(src: &str) -> (Vec<String>, Vec<TableOp>) {
    let upper = src.to_uppercase();
    let bytes = src.as_bytes();
    let upper_bytes = upper.as_bytes();

    let mut by_table: BTreeMap<String, Vec<TableOpKind>> = BTreeMap::new();

    let patterns: [(&[u8], TableOpKind, bool); 5] = [
        (b"FROM ", TableOpKind::Select, false),
        (b"JOIN ", TableOpKind::Select, false),
        (b"UPDATE ", TableOpKind::Update, false),
        (b"INSERT INTO ", TableOpKind::Insert, true),
        (b"DELETE FROM ", TableOpKind::Delete, true),
    ];

    for (pat, kind, _mandatory_into) in patterns {
        let mut i = 0;
        while let Some(rel) = find_keyword(&upper_bytes[i..], pat) {
            let start = i + rel + pat.len();
            if let Some((table, len)) = parse_identifier_at(&bytes[start..]) {
                if !is_noise(&table) {
                    by_table.entry(table).or_default().push(kind);
                }
                i = start + len;
            } else {
                i = start;
            }
            if i >= bytes.len() {
                break;
            }
        }
    }

    // Bare DELETE <table> — less common but cheap to add.
    let mut i = 0;
    let delete_kw = b"DELETE ";
    while let Some(rel) = find_keyword(&upper_bytes[i..], delete_kw) {
        let after = i + rel + delete_kw.len();
        // Skip if followed by FROM (handled above).
        let tail = &upper_bytes[after..].iter().take(5).copied().collect::<Vec<_>>();
        if tail.starts_with(b"FROM ") {
            i = after;
            continue;
        }
        if let Some((table, len)) = parse_identifier_at(&bytes[after..]) {
            if !is_noise(&table) {
                by_table.entry(table).or_default().push(TableOpKind::Delete);
            }
            i = after + len;
        } else {
            i = after;
        }
    }

    let mut tables: Vec<String> = by_table.keys().cloned().collect();
    tables.sort();
    let ops: Vec<TableOp> = by_table
        .into_iter()
        .map(|(table, mut kinds)| {
            kinds.sort_by_key(|k| format!("{:?}", k));
            kinds.dedup();
            TableOp { table, kinds }
        })
        .collect();
    (tables, ops)
}

/// True if a keyword match is a real token boundary (preceded by
/// whitespace / start-of-string, not inside a longer identifier).
fn find_keyword(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            let prev = if i == 0 { b' ' } else { haystack[i - 1] };
            if prev == b' ' || prev == b'\t' || prev == b'\n' || prev == b'\r' || prev == b',' || prev == b'(' {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Parse an identifier starting at `bytes[0]`. Accepts `[schema].[name]`,
/// `schema.name`, `[name]`, or `name`. Returns `(full_name, consumed_len)`.
fn parse_identifier_at(bytes: &[u8]) -> Option<(String, usize)> {
    let mut i = 0;
    // Skip leading whitespace.
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n' || bytes[i] == b'\r') {
        i += 1;
    }
    let start = i;
    let mut buf = String::new();
    let mut saw_something = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '[' {
            // Find matching `]`.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b']' {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            buf.push_str(std::str::from_utf8(&bytes[i + 1..j]).ok()?);
            i = j + 1;
            saw_something = true;
        } else if c == '.' {
            buf.push('.');
            i += 1;
        } else if c.is_ascii_alphanumeric() || c == '_' || c == '#' || c == '@' {
            buf.push(c);
            i += 1;
            saw_something = true;
        } else {
            break;
        }
    }
    if !saw_something {
        return None;
    }
    Some((buf, i - start))
}

/// Filter out obvious non-table matches: aliases, `@variables`, `#temptables`,
/// keywords that slipped through.
fn is_noise(name: &str) -> bool {
    if name.starts_with('@') || name.starts_with('#') {
        return true;
    }
    if name.is_empty() {
        return true;
    }
    let upper = name.to_uppercase();
    let keywords = ["SELECT", "WHERE", "AS", "ON", "SET", "WITH", "HAVING", "GROUP", "ORDER", "CTE"];
    keywords.contains(&upper.as_str())
}

// ─────────────────────────── helpers (local copies) ───────────────────────────

fn split_go_batches(src: &str) -> Vec<&str> {
    // Split on `GO` separator lines while preserving valid char boundaries.
    // Files may use `\r\n` line endings AND contain multi-byte UTF-8 chars
    // (em-dashes, BOM, etc.), so naive `offset += line.len() + 1` byte
    // arithmetic falls inside multi-byte sequences and panics on slicing.
    // Walk the source by line spans the safe way: track byte ranges per
    // line via `match_indices('\n')` and slice on those.
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut line_start = 0usize;
    let mut idx = 0usize;
    while idx < bytes.len() {
        // Scan to the next newline.
        while idx < bytes.len() && bytes[idx] != b'\n' {
            idx += 1;
        }
        // Line span = bytes[line_start..idx]; trim a trailing '\r'.
        let mut end = idx;
        if end > line_start && bytes[end.saturating_sub(1)] == b'\r' {
            end -= 1;
        }
        // Char-boundary safe: bytes[line_start..end] is a full subslice of
        // the str because newlines and CR are single-byte ASCII.
        let line = &src[line_start..end];
        if line.trim().eq_ignore_ascii_case("GO") {
            if line_start > start {
                out.push(&src[start..line_start]);
            }
            // Skip past the newline.
            start = (idx + 1).min(src.len());
        }
        // Advance past '\n'.
        idx += 1;
        line_start = idx;
    }
    if start < src.len() {
        out.push(&src[start..]);
    }
    if out.is_empty() {
        out.push(src);
    }
    out
}

fn strip_comments(s: &str) -> String {
    // Char-based scan: SQL files often begin with a UTF-8 BOM (EF BB BF)
    // and can contain non-ASCII content in strings/comments. Byte-wise
    // `bytes[i] as char` would corrupt those into latin-1 garbage.
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '-' && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < chars.len() {
                i += 2;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_identifier_bracketed_two_parts() {
        let s = b"[cardholder].[cpf_Client_Profile] WHERE";
        let (name, _) = parse_identifier_at(s).unwrap();
        assert_eq!(name, "cardholder.cpf_Client_Profile");
    }

    #[test]
    fn parse_identifier_unbracketed() {
        let s = b"dbo.users x";
        let (name, _) = parse_identifier_at(s).unwrap();
        assert_eq!(name, "dbo.users");
    }

    #[test]
    fn parse_identifier_rejects_variables_and_temp() {
        assert!(is_noise("@var"));
        assert!(is_noise("#temp"));
        assert!(is_noise("SELECT"));
        assert!(!is_noise("cardholder.cpf_Client_Profile"));
    }

    #[test]
    fn extract_table_refs_finds_select_join_update_insert_delete() {
        let proc = r#"
            SELECT c.* FROM [cardholder].[cpf_Client_Profile] c
            JOIN [cardholder].[add_Address_Details] a ON a.cpf_Profile_Id = c.cpf_Profile_Id
            WHERE c.cps_Status = 'ACT';

            UPDATE [cardholder].[cpf_Client_Profile]
            SET cpf_Last_Name = @name
            WHERE cpf_Profile_Id = @id;

            INSERT INTO [cardholder].[cst_Cardholder_Status_Transition] (...) VALUES (...);

            DELETE FROM [cardholder].[add_Address_Details_History] WHERE cpf_Profile_Id = @id;
        "#;
        let (tables, ops) = extract_table_references(proc);
        // 4 distinct tables.
        assert_eq!(tables.len(), 4);
        // cpf_Client_Profile should have both Select AND Update.
        let cpf = ops.iter().find(|o| o.table.contains("cpf_Client_Profile")).unwrap();
        assert!(cpf.kinds.contains(&TableOpKind::Select));
        assert!(cpf.kinds.contains(&TableOpKind::Update));
        let cst = ops.iter().find(|o| o.table.contains("cst_Cardholder")).unwrap();
        assert!(cst.kinds.contains(&TableOpKind::Insert));
        let hist = ops.iter().find(|o| o.table.contains("add_Address_Details_History")).unwrap();
        assert!(hist.kinds.contains(&TableOpKind::Delete));
    }

    #[test]
    fn catalog_classifies_procedure_and_table() {
        let tmp = tempfile::TempDir::new().unwrap();
        let gt = tmp.path().join("1-ground-truth/cardholder");
        std::fs::create_dir_all(gt.join("Tables")).unwrap();
        std::fs::create_dir_all(gt.join("Stored Procedures")).unwrap();
        std::fs::write(
            gt.join("Tables/cpf.sql"),
            r#"CREATE TABLE [cardholder].[cpf_Client_Profile] (
                [cpf_Profile_Id] UNIQUEIDENTIFIER NOT NULL,
                CONSTRAINT [PK_cpf] PRIMARY KEY ([cpf_Profile_Id])
            );"#,
        )
        .unwrap();
        std::fs::write(
            gt.join("Stored Procedures/p_create_cardholder.sql"),
            r#"CREATE PROCEDURE [cardholder].[p_create_cardholder]
                @id UNIQUEIDENTIFIER,
                @name NVARCHAR(50)
            AS
                INSERT INTO [cardholder].[cpf_Client_Profile] (cpf_Profile_Id) VALUES (@id);
            "#,
        )
        .unwrap();
        let cat = build_catalog(tmp.path()).unwrap();
        assert_eq!(cat.tables.len(), 1);
        assert_eq!(cat.objects.len(), 1);
        let proc = &cat.objects[0];
        assert_eq!(proc.kind, SqlKind::Procedure);
        assert_eq!(proc.name, "p_create_cardholder");
        assert!(proc
            .referenced_tables
            .iter()
            .any(|t| t.contains("cpf_Client_Profile")));
        assert!(proc
            .ops
            .iter()
            .any(|o| o.kinds.contains(&TableOpKind::Insert)));
    }

    #[test]
    fn catalog_objects_referencing_finds_matching_procs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let gt = tmp.path().join("1-ground-truth/cardholder/Stored Procedures");
        std::fs::create_dir_all(&gt).unwrap();
        std::fs::write(
            gt.join("p_one.sql"),
            "CREATE PROCEDURE p_one AS SELECT * FROM [cardholder].[cpf_Client_Profile];",
        )
        .unwrap();
        std::fs::write(
            gt.join("p_two.sql"),
            "CREATE PROCEDURE p_two AS SELECT * FROM dbo.other;",
        )
        .unwrap();
        let cat = build_catalog(tmp.path()).unwrap();
        let targets = vec!["cardholder.cpf_Client_Profile".to_string()];
        let matched = cat.objects_referencing(&targets);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "p_one");
    }

    #[test]
    fn table_names_match_handles_bracketless_variants() {
        assert!(table_names_match("cardholder.cpf_Client_Profile", "cpf_Client_Profile"));
        assert!(table_names_match("cpf_Client_Profile", "cardholder.cpf_Client_Profile"));
        assert!(table_names_match("dbo.users", "Users"));
        assert!(!table_names_match("dbo.users", "accounts"));
    }
}

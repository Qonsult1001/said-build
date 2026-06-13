//! SQL Server schema parser.
//!
//! Takes the body of a `CREATE TABLE` statement (as produced by
//! `sca_core::code_search::ast_chunk`) and extracts structured column /
//! constraint info: name, SQL type, nullable, primary keys, foreign keys,
//! unique keys.
//!
//! The parser is line-based rather than full-grammar — SQL Server's
//! `CREATE TABLE` grammar is huge but ~95% of real table DDL fits the
//! "one column per line, one constraint per line" shape. Constructs we
//! handle today:
//!
//! - `[Schema].[Name]` and bare `Name` table identifiers.
//! - Column line: `[col_name]  TYPE[(N|MAX|p,s)]  [DEFAULT ...]  NULL|NOT NULL`
//! - `CONSTRAINT [PK_*] PRIMARY KEY (CLUSTERED|NONCLUSTERED)? ([col] ASC|DESC, ...)`
//! - `CONSTRAINT [UK_*] UNIQUE (CLUSTERED|NONCLUSTERED)? ([col], ...)`
//! - `CONSTRAINT [FK_*] FOREIGN KEY ([col]) REFERENCES [Schema].[RefTable] ([RefCol])`
//! - `[colname] AS (expression) PERSISTED` → computed column (recorded as `is_computed`).
//!
//! Unhandled (today — could extend): multi-column FKs, INCLUDE lists on
//! indexes, table variables (`DECLARE @t TABLE`), partition schemes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableSchema {
    /// Full qualified name as found in the DDL — e.g. `[cardholder].[cpf_Client_Profile]`
    /// or just `tbl_Foo`. Square brackets stripped.
    pub schema: Option<String>,
    pub name: String,
    pub columns: Vec<Column>,
    pub primary_key: Vec<String>,
    pub unique_keys: Vec<Vec<String>>,
    pub foreign_keys: Vec<ForeignKey>,
}

impl TableSchema {
    pub fn full_name(&self) -> String {
        match &self.schema {
            Some(s) => format!("{}.{}", s, self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// Raw type token (e.g. `NVARCHAR`, `VARCHAR`, `UNIQUEIDENTIFIER`, `DATETIME`).
    /// Uppercased for easy comparison.
    pub sql_type: String,
    /// Size parameter — e.g. `Some(50)` for `VARCHAR(50)`, `None` for `INT`.
    /// `-1` encodes `MAX` (SQL Server convention).
    pub length: Option<i32>,
    /// Numeric precision for `DECIMAL(p, s)` / `NUMERIC(p, s)`.
    pub precision: Option<(u32, u32)>,
    pub nullable: bool,
    pub identity: bool,
    pub is_computed: bool,
    /// True if the column has a `DEFAULT` expression.
    pub has_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForeignKey {
    pub column: String,
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_column: String,
}

// ─────────────────────────── parser ───────────────────────────

/// Parse a full `CREATE TABLE` statement body (everything starting from the
/// `CREATE TABLE` keyword through the closing `);`). Returns `None` if the
/// input doesn't look like a CREATE TABLE statement.
pub fn parse_create_table(source: &str) -> Option<TableSchema> {
    let stripped = strip_comments(source);
    let no_bom = stripped.trim_start_matches('\u{FEFF}').trim();
    let upper = no_bom.to_uppercase();
    let ct_idx = upper.find("CREATE TABLE")?;
    let after_ct = no_bom[ct_idx + "CREATE TABLE".len()..].trim_start();

    // Everything up to the first `(` is the table identifier.
    let open_paren = after_ct.find('(')?;
    let identifier_raw = after_ct[..open_paren].trim();
    let (schema, name) = parse_qualified_name(identifier_raw);

    // Body is between the first `(` and the LAST matching `)` before the
    // trailing semicolon. We balance parens — inner parens belong to types
    // (VARCHAR(50)) and constraints (PRIMARY KEY ([col] ASC)).
    let body_start = open_paren + 1;
    let body_end = match_closing_paren(&after_ct[body_start..])?;
    let body = &after_ct[body_start..body_start + body_end];

    let top_level_items = split_top_level_items(body);

    let mut columns = Vec::new();
    let mut primary_key: Vec<String> = Vec::new();
    let mut unique_keys: Vec<Vec<String>> = Vec::new();
    let mut foreign_keys: Vec<ForeignKey> = Vec::new();

    for item in top_level_items {
        let t = item.trim().trim_end_matches(',').trim();
        if t.is_empty() {
            continue;
        }
        let t_upper = t.to_uppercase();
        if t_upper.starts_with("CONSTRAINT") || t_upper.starts_with("PRIMARY KEY") || t_upper.starts_with("UNIQUE") || t_upper.starts_with("FOREIGN KEY") {
            // Constraint line. Pick the right branch by looking past the
            // optional CONSTRAINT [name] preamble.
            let body = skip_constraint_name_prefix(t);
            let body_upper = body.to_uppercase();
            if body_upper.starts_with("PRIMARY KEY") {
                if let Some(cols) = parse_column_list_after_keyword(body, "PRIMARY KEY") {
                    primary_key = cols;
                }
            } else if body_upper.starts_with("UNIQUE") {
                if let Some(cols) = parse_column_list_after_keyword(body, "UNIQUE") {
                    unique_keys.push(cols);
                }
            } else if body_upper.starts_with("FOREIGN KEY") {
                if let Some(fk) = parse_foreign_key(body) {
                    foreign_keys.push(fk);
                }
            } else if body_upper.starts_with("CHECK") {
                // Ignore CHECK constraints for schema shape — they're
                // validation logic, not schema.
            }
        } else if let Some(col) = parse_column_line(t) {
            columns.push(col);
        }
    }

    // Apply PK info back onto columns so consumers only need to look at the
    // column list in the common case.
    Some(TableSchema {
        schema,
        name,
        columns,
        primary_key,
        unique_keys,
        foreign_keys,
    })
}

/// `[Schema].[Name]` or `[Name]` or `Schema.Name` or `Name` → (Option<Schema>, Name).
fn parse_qualified_name(raw: &str) -> (Option<String>, String) {
    let cleaned = raw.trim();
    let parts: Vec<String> = cleaned
        .split('.')
        .map(|s| s.trim().trim_start_matches('[').trim_end_matches(']').to_string())
        .collect();
    match parts.len() {
        2 => (Some(parts[0].clone()), parts[1].clone()),
        _ => (None, parts.last().cloned().unwrap_or_default()),
    }
}

/// Scan `body` (starting after the opening `(`) and return the byte index of
/// the matching `)`. Returns `None` if unbalanced.
fn match_closing_paren(body: &str) -> Option<usize> {
    let mut depth: i32 = 1;
    for (i, ch) in body.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
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

/// Split a CREATE TABLE body into top-level items separated by `,` that's at
/// depth 0. Nested `(...)` blocks (type specs, PK column lists) are kept
/// together.
fn split_top_level_items(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    for ch in body.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                out.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Strip `CONSTRAINT [name]` prefix if present, returning the tail
/// (beginning with the constraint type keyword: PRIMARY KEY / UNIQUE / FOREIGN KEY / CHECK).
fn skip_constraint_name_prefix(s: &str) -> &str {
    let upper = s.to_uppercase();
    if !upper.starts_with("CONSTRAINT") {
        return s;
    }
    let after_kw: &str = s["CONSTRAINT".len()..].trim_start();
    let after_name: &str = if let Some(rest) = after_kw.strip_prefix('[') {
        match rest.find(']') {
            Some(idx) => &rest[idx + 1..],
            None => after_kw,
        }
    } else {
        match after_kw.find(char::is_whitespace) {
            Some(idx) => &after_kw[idx..],
            None => "",
        }
    };
    after_name.trim_start()
}

fn parse_column_list_after_keyword(s: &str, keyword: &str) -> Option<Vec<String>> {
    let upper = s.to_uppercase();
    let idx = upper.find(keyword)?;
    let after = &s[idx + keyword.len()..];
    // Optional CLUSTERED / NONCLUSTERED before the `(`.
    let open = after.find('(')?;
    let list = after[open + 1..].trim_start();
    let close = list.find(')')?;
    let cols = &list[..close];
    let out: Vec<String> = cols
        .split(',')
        .map(|p| {
            p.trim()
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn parse_foreign_key(s: &str) -> Option<ForeignKey> {
    let upper = s.to_uppercase();
    let fk_idx = upper.find("FOREIGN KEY")?;
    let after_fk = &s[fk_idx + "FOREIGN KEY".len()..];
    let open = after_fk.find('(')?;
    let close = after_fk[open + 1..].find(')')?;
    let column_raw = after_fk[open + 1..open + 1 + close].trim();
    let column = column_raw
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    // REFERENCES clause.
    let ref_idx = upper[fk_idx..].find("REFERENCES")? + fk_idx;
    let after_ref = &s[ref_idx + "REFERENCES".len()..].trim_start();
    // [Schema].[Table] or [Table] or Schema.Table
    let paren = after_ref.find('(')?;
    let ident = after_ref[..paren].trim();
    let (ref_schema, ref_table) = parse_qualified_name(ident);
    // Referenced column.
    let inner = &after_ref[paren + 1..];
    let close2 = inner.find(')')?;
    let ref_column = inner[..close2]
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    Some(ForeignKey {
        column,
        ref_schema,
        ref_table,
        ref_column,
    })
}

/// Parse a single column-definition line. Returns None if the line isn't a
/// column (empty, constraint, computed-column boilerplate).
fn parse_column_line(line: &str) -> Option<Column> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    // Skip if this is clearly a constraint row.
    let upper = t.to_uppercase();
    if upper.starts_with("CONSTRAINT")
        || upper.starts_with("PRIMARY KEY")
        || upper.starts_with("UNIQUE")
        || upper.starts_with("FOREIGN KEY")
        || upper.starts_with("CHECK")
    {
        return None;
    }

    // First token = column name. May be `[bracketed]`.
    let (name_tok, rest) = split_first_token(t)?;
    let name = name_tok
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if name.is_empty() {
        return None;
    }

    // Second token = SQL type (possibly followed by `(N)` or `(p,s)` or `(MAX)`).
    let (type_tok, after_type) = split_first_token(rest)?;
    let type_upper = type_tok.to_uppercase();
    if type_upper == "AS" {
        // Computed column: `[col] AS (expr) PERSISTED`.
        return Some(Column {
            name,
            sql_type: "COMPUTED".into(),
            length: None,
            precision: None,
            nullable: true,
            identity: false,
            is_computed: true,
            has_default: false,
        });
    }

    // Type may be followed by a parenthesised size spec: `VARCHAR (50)` or
    // `DECIMAL(10, 2)`. The `(...)` might be on the same line separated by
    // whitespace.
    let (length, precision, rest_after_type) = parse_type_parens(after_type);

    // Scan remaining tokens for NULL / NOT NULL / IDENTITY / DEFAULT.
    let upper_rest = rest_after_type.to_uppercase();
    let identity = upper_rest.contains("IDENTITY");
    let has_default = upper_rest.contains("DEFAULT") || upper_rest.contains("CONSTRAINT");
    // Default nullable = true unless explicit NOT NULL. `NULL` alone means nullable.
    let not_null = upper_rest.contains("NOT NULL");
    let nullable = !not_null;

    Some(Column {
        name,
        sql_type: type_upper,
        length,
        precision,
        nullable,
        identity,
        is_computed: false,
        has_default,
    })
}

/// Split a string on the first whitespace boundary, but respect bracketed
/// tokens so `[some name with space]` stays together.
fn split_first_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix('[') {
        let close = rest.find(']')?;
        let end = close + 1 + 1; // `[name]`
        let tok = &s[..end];
        let after = s[end..].trim_start();
        return Some((tok, after));
    }
    match s.find(char::is_whitespace) {
        Some(idx) => Some((&s[..idx], s[idx..].trim_start())),
        None => Some((s, "")),
    }
}

fn parse_type_parens(s: &str) -> (Option<i32>, Option<(u32, u32)>, &str) {
    let s = s.trim_start();
    if !s.starts_with('(') {
        return (None, None, s);
    }
    let Some(close) = s.find(')') else {
        return (None, None, s);
    };
    let inside = s[1..close].trim();
    let after = s[close + 1..].trim_start();
    // `MAX` or `N` or `p, s`.
    if inside.eq_ignore_ascii_case("MAX") {
        return (Some(-1), None, after);
    }
    if let Some(comma) = inside.find(',') {
        let p = inside[..comma].trim().parse::<u32>().ok();
        let sc = inside[comma + 1..].trim().parse::<u32>().ok();
        if let (Some(p), Some(sc)) = (p, sc) {
            return (None, Some((p, sc)), after);
        }
    }
    if let Ok(n) = inside.parse::<i32>() {
        return (Some(n), None, after);
    }
    (None, None, after)
}

/// Strip `-- line` and `/* block */` comments. Simple state machine — good
/// enough for DDL, doesn't try to be clever about strings containing `--`.
/// Char-based iteration so UTF-8 BOMs and non-ASCII identifiers survive.
fn strip_comments(s: &str) -> String {
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

    const CARDHOLDER_DDL: &str = r#"
CREATE TABLE [cardholder].[cpf_Client_Profile] (
    [cpf_Profile_Id]       UNIQUEIDENTIFIER   NOT NULL,
    [cpf_Easy_Profile_Id]  VARCHAR (60)       NOT NULL,
    [cpf_Last_Name]        NVARCHAR (1024)    NULL,
    [cpf_DOB]              VARCHAR (10)       NULL,
    [ptl_Code]             VARCHAR (10)       NOT NULL,
    [cpf_Entry_Id]         INT                IDENTITY (1, 1) NOT NULL,
    [cpf_Created_UTC]      DATETIMEOFFSET (3) CONSTRAINT [DF_cpf_Client_Profile_cpf_Created_UTC] DEFAULT (getutcdate()) NULL,
    [cpf_attributes]       NVARCHAR (MAX)     NULL,
    [cpf_Parent_Id]        UNIQUEIDENTIFIER   NULL,
    [LastUpdatedOrCreated] AS                 (isnull([cpf_Updated_At_UTC],[cpf_Created_UTC])) PERSISTED,
    CONSTRAINT [PK_cpf_Client_Profile] PRIMARY KEY CLUSTERED ([cpf_Profile_Id] ASC),
    CONSTRAINT [CK_cpf_Client_Profile_cpf_attributes] CHECK (isjson([cpf_attributes])=(1)),
    CONSTRAINT [FK_cpf_Client_Profile_cpf_Client_Profile] FOREIGN KEY ([cpf_Parent_Id]) REFERENCES [cardholder].[cpf_Client_Profile] ([cpf_Profile_Id]),
    CONSTRAINT [UK__cpf_Client_Profile__cpf_Easy_Profile_Id] UNIQUE NONCLUSTERED ([cpf_Easy_Profile_Id] ASC)
);
"#;

    #[test]
    fn parses_cardholder_profile_ddl_cleanly() {
        let schema = parse_create_table(CARDHOLDER_DDL).expect("should parse");
        assert_eq!(schema.schema.as_deref(), Some("cardholder"));
        assert_eq!(schema.name, "cpf_Client_Profile");
        assert_eq!(schema.columns.len(), 10);
    }

    #[test]
    fn column_types_and_lengths_correct() {
        let s = parse_create_table(CARDHOLDER_DDL).unwrap();
        let last_name = s.columns.iter().find(|c| c.name == "cpf_Last_Name").unwrap();
        assert_eq!(last_name.sql_type, "NVARCHAR");
        assert_eq!(last_name.length, Some(1024));
        assert!(last_name.nullable);

        let easy = s.columns.iter().find(|c| c.name == "cpf_Easy_Profile_Id").unwrap();
        assert_eq!(easy.sql_type, "VARCHAR");
        assert_eq!(easy.length, Some(60));
        assert!(!easy.nullable);

        let attrs = s.columns.iter().find(|c| c.name == "cpf_attributes").unwrap();
        assert_eq!(attrs.length, Some(-1)); // MAX
    }

    #[test]
    fn identity_column_detected() {
        let s = parse_create_table(CARDHOLDER_DDL).unwrap();
        let id = s.columns.iter().find(|c| c.name == "cpf_Entry_Id").unwrap();
        assert!(id.identity);
        assert_eq!(id.sql_type, "INT");
    }

    #[test]
    fn primary_key_captured() {
        let s = parse_create_table(CARDHOLDER_DDL).unwrap();
        assert_eq!(s.primary_key, vec!["cpf_Profile_Id"]);
    }

    #[test]
    fn foreign_key_captured() {
        let s = parse_create_table(CARDHOLDER_DDL).unwrap();
        let fks = &s.foreign_keys;
        assert_eq!(fks.len(), 1);
        let fk = &fks[0];
        assert_eq!(fk.column, "cpf_Parent_Id");
        assert_eq!(fk.ref_schema.as_deref(), Some("cardholder"));
        assert_eq!(fk.ref_table, "cpf_Client_Profile");
        assert_eq!(fk.ref_column, "cpf_Profile_Id");
    }

    #[test]
    fn unique_key_captured() {
        let s = parse_create_table(CARDHOLDER_DDL).unwrap();
        assert_eq!(s.unique_keys.len(), 1);
        assert_eq!(s.unique_keys[0], vec!["cpf_Easy_Profile_Id"]);
    }

    #[test]
    fn computed_column_flagged() {
        let s = parse_create_table(CARDHOLDER_DDL).unwrap();
        let computed = s
            .columns
            .iter()
            .find(|c| c.name == "LastUpdatedOrCreated")
            .unwrap();
        assert!(computed.is_computed);
    }

    #[test]
    fn handles_bare_identifier_without_schema_prefix() {
        let ddl = "CREATE TABLE tbl_Simple ( [id] INT NOT NULL, [name] VARCHAR(50) NULL );";
        let s = parse_create_table(ddl).unwrap();
        assert_eq!(s.schema, None);
        assert_eq!(s.name, "tbl_Simple");
        assert_eq!(s.columns.len(), 2);
    }

    #[test]
    fn handles_decimal_with_precision_and_scale() {
        let ddl = "CREATE TABLE t ( [amount] DECIMAL (10, 2) NOT NULL );";
        let s = parse_create_table(ddl).unwrap();
        let amt = &s.columns[0];
        assert_eq!(amt.sql_type, "DECIMAL");
        assert_eq!(amt.precision, Some((10, 2)));
    }

    #[test]
    fn strips_block_and_line_comments() {
        let ddl = r#"
            -- header comment
            CREATE TABLE t ( /* inline */ [id] INT NOT NULL );
            -- trailer
        "#;
        let s = parse_create_table(ddl).unwrap();
        assert_eq!(s.name, "t");
        assert_eq!(s.columns.len(), 1);
    }

    #[test]
    fn returns_none_when_not_a_create_table() {
        assert!(parse_create_table("CREATE PROCEDURE foo AS SELECT 1").is_none());
        assert!(parse_create_table("").is_none());
        assert!(parse_create_table("-- just a comment").is_none());
    }

    #[test]
    fn default_constraint_column_still_has_correct_nullable() {
        let ddl = r#"
            CREATE TABLE t (
                [ts] DATETIME CONSTRAINT [DF_ts] DEFAULT (getdate()) NOT NULL
            );
        "#;
        let s = parse_create_table(ddl).unwrap();
        let ts = &s.columns[0];
        assert_eq!(ts.name, "ts");
        assert_eq!(ts.sql_type, "DATETIME");
        assert!(!ts.nullable);
        assert!(ts.has_default);
    }
}

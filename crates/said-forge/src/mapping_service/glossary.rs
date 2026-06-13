//! Glossary — auto-built from the SQL catalog.
//!
//! Two sources feed the glossary:
//!
//! 1. **`lookups.*` tables.** Every table in the `lookups` schema is an
//!    authoritative list of allowed values for one logical concept —
//!    `col_Country_Lookup` is *the* country code list,
//!    `adl_Address_Type_Lookup` is *the* address-type list, and so on.
//!    For each lookup table we register a glossary term keyed on the
//!    table name stripped of its Hungarian prefix (`country_lookup` →
//!    `country`), with the lookup's code column's SQL type stored as
//!    the accepted storage type. The code column is whichever column
//!    is the PK (single-column) or matches `<prefix>_Code`.
//!
//! 2. **FK graph.** When a column in a business table FKs into a
//!    lookup, the business term adopts the lookup's accepted SQL type.
//!    This lets MappingService accept `country_code VARCHAR(2)` as
//!    compatible with a logical `string` field when the field is
//!    clearly a country reference.
//!
//! The glossary is plain data — no I/O beyond reading an in-memory
//! SqlCatalog. It's serialisable so `.forge/glossary.toml` can be
//! written and diffed across runs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::schema::TableSchema;
use crate::sql_catalog::SqlCatalog;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Glossary {
    #[serde(default)]
    terms: BTreeMap<String, Term>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Term {
    pub name: String,
    pub description: String,
    /// SQL types that may legitimately store this concept (uppercase
    /// base type, no length). Used by MappingService to widen compat.
    pub sql_types: Vec<String>,
    /// Human-readable list of schema objects that back this term.
    pub sources: Vec<String>,
}

impl Glossary {
    pub fn term(&self, name: &str) -> Option<&Term> {
        self.terms
            .get(name)
            .or_else(|| self.terms.get(&name.to_ascii_lowercase()))
    }

    pub fn terms(&self) -> impl Iterator<Item = (&String, &Term)> {
        self.terms.iter()
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn insert_term(&mut self, term: Term) {
        self.terms.insert(term.name.to_ascii_lowercase(), term);
    }

    /// Build a glossary from a SqlCatalog. Pure function — deterministic.
    pub fn build(catalog: &SqlCatalog) -> Self {
        let mut g = Glossary::default();

        // Pass 1: every lookup.* table becomes a term.
        for t in &catalog.tables {
            if !is_lookup_schema(t) {
                continue;
            }
            let term_name = lookup_term_name(t);
            if term_name.is_empty() {
                continue;
            }
            let code_col = code_column(t);
            let description = match &code_col {
                Some(col) => format!(
                    "Allowed values list. Backed by `{}` — code column `{}`.",
                    t.full_name(),
                    col.name
                ),
                None => format!("Allowed values list. Backed by `{}`.", t.full_name()),
            };
            let sql_types = match &code_col {
                Some(col) => vec![col.sql_type.to_ascii_uppercase()],
                None => Vec::new(),
            };
            g.insert_term(Term {
                name: term_name,
                description,
                sql_types,
                sources: vec![t.full_name()],
            });
        }

        // Pass 2: FK graph — business columns that FK into a lookup
        // inherit the lookup term's storage type and register a term
        // keyed on the column's stripped name.
        for t in &catalog.tables {
            for fk in &t.foreign_keys {
                let target_schema = fk.ref_schema.as_deref().unwrap_or("");
                if !target_schema.eq_ignore_ascii_case("lookups") {
                    continue;
                }
                let target_full = format!("{}.{}", target_schema, fk.ref_table);
                let target = match catalog.find_table(&target_full) {
                    Some(t) => t,
                    None => continue,
                };
                let code_col = match code_column(target) {
                    Some(c) => c,
                    None => continue,
                };
                let term_name = strip_col_prefix(&fk.column).to_ascii_lowercase();
                if term_name.is_empty() {
                    continue;
                }
                let entry = g.terms.entry(term_name.clone()).or_insert(Term {
                    name: term_name,
                    description: format!("Reference to `{}`.", target_full),
                    sql_types: Vec::new(),
                    sources: Vec::new(),
                });
                let upper = code_col.sql_type.to_ascii_uppercase();
                if !entry.sql_types.iter().any(|s| s.eq_ignore_ascii_case(&upper)) {
                    entry.sql_types.push(upper);
                }
                let source = format!("{} via FK {}", t.full_name(), fk.column);
                if !entry.sources.contains(&source) {
                    entry.sources.push(source);
                }
            }
        }

        g
    }

    /// Render the glossary as a Markdown table — the shape we emit to
    /// `<skill>/references/glossary.md`.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Glossary\n\n");
        out.push_str("_Auto-generated from `lookups.*` + FK graph. Do not edit by hand._\n\n");
        if self.terms.is_empty() {
            out.push_str("_(no terms captured — catalog had no `lookups.*` tables)_\n");
            return out;
        }
        out.push_str("| term | accepted SQL types | backed by |\n");
        out.push_str("|------|--------------------|-----------|\n");
        for (_, term) in &self.terms {
            let types = if term.sql_types.is_empty() {
                "—".to_string()
            } else {
                term.sql_types.join(", ")
            };
            let sources = if term.sources.is_empty() {
                "—".to_string()
            } else {
                term.sources.join("; ")
            };
            out.push_str(&format!(
                "| `{}` | {} | {} |\n",
                term.name, types, sources
            ));
        }
        out
    }
}

fn is_lookup_schema(t: &TableSchema) -> bool {
    t.schema
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("lookups"))
        .unwrap_or(false)
}

/// `col_Country_Lookup` → `country`, `adl_Address_Type_Lookup` → `address_type`.
fn lookup_term_name(t: &TableSchema) -> String {
    let base = strip_col_prefix(&t.name);
    let no_suffix = base
        .trim_end_matches("_Lookup")
        .trim_end_matches("_LOOKUP")
        .trim_end_matches("_lookup");
    no_suffix.to_ascii_lowercase()
}

/// `ptl_Code` → `code`, `col_Country_Lookup` → `Country_Lookup`.
fn strip_col_prefix(name: &str) -> String {
    let mut parts = name.splitn(2, '_');
    let first = parts.next().unwrap_or("");
    let rest = parts.next();
    if rest.is_some()
        && (2..=4).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_lowercase())
    {
        rest.unwrap().to_string()
    } else {
        name.to_string()
    }
}

fn code_column(t: &TableSchema) -> Option<&crate::schema::Column> {
    if t.primary_key.len() == 1 {
        let pk_name = &t.primary_key[0];
        if let Some(col) = t.columns.iter().find(|c| &c.name == pk_name) {
            return Some(col);
        }
    }
    t.columns
        .iter()
        .find(|c| c.name.to_ascii_lowercase().ends_with("_code"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse_create_table;

    fn cat_with_lookup_and_business() -> SqlCatalog {
        let lookup = parse_create_table(
            r#"CREATE TABLE [lookups].[col_Country_Lookup] (
                [col_Code]        VARCHAR (2)  NOT NULL,
                [col_Description] NVARCHAR (255) NULL,
                CONSTRAINT [PK_col] PRIMARY KEY ([col_Code])
            );"#,
        )
        .unwrap();
        let business = parse_create_table(
            r#"CREATE TABLE [cardholder].[add_Address_Details] (
                [cpf_Profile_Id] UNIQUEIDENTIFIER NOT NULL,
                [col_Code]       VARCHAR (2)      NOT NULL,
                CONSTRAINT [PK_add] PRIMARY KEY ([cpf_Profile_Id], [col_Code]),
                CONSTRAINT [FK_col] FOREIGN KEY ([col_Code]) REFERENCES [lookups].[col_Country_Lookup] ([col_Code])
            );"#,
        )
        .unwrap();
        SqlCatalog {
            tables: vec![lookup, business],
            objects: Vec::new(),
        }
    }

    #[test]
    fn build_registers_lookup_term_stripping_hungarian_and_suffix() {
        let cat = cat_with_lookup_and_business();
        let g = Glossary::build(&cat);
        let t = g.term("country").expect("country term");
        assert!(t.sql_types.iter().any(|s| s == "VARCHAR"));
        assert!(t
            .sources
            .iter()
            .any(|s| s.contains("lookups.col_Country_Lookup")));
    }

    #[test]
    fn build_registers_column_term_via_fk() {
        let cat = cat_with_lookup_and_business();
        let g = Glossary::build(&cat);
        // `col_Code` in business table FKs into lookups.col_Country_Lookup,
        // so we register `code` term with VARCHAR acceptance AND record the
        // source.
        let t = g.term("code").expect("code term from FK");
        assert!(t.sql_types.iter().any(|s| s == "VARCHAR"));
        assert!(t
            .sources
            .iter()
            .any(|s| s.contains("add_Address_Details")));
    }

    #[test]
    fn to_markdown_lists_every_term() {
        let cat = cat_with_lookup_and_business();
        let g = Glossary::build(&cat);
        let md = g.to_markdown();
        assert!(md.contains("# Glossary"));
        assert!(md.contains("`country`"));
        assert!(md.contains("`code`"));
    }

    #[test]
    fn empty_catalog_yields_empty_glossary() {
        let cat = SqlCatalog::default();
        let g = Glossary::build(&cat);
        assert!(g.is_empty());
        assert!(g.to_markdown().contains("no terms captured"));
    }

    #[test]
    fn case_insensitive_term_lookup() {
        let cat = cat_with_lookup_and_business();
        let g = Glossary::build(&cat);
        assert!(g.term("Country").is_some());
        assert!(g.term("COUNTRY").is_some());
    }
}

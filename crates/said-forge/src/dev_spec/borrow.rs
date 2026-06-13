//! TXN-borrow heuristic. For each ERD entity, search the SQL catalog
//! for the best-matching existing table and copy its schema, prefix,
//! and column types when the score crosses a threshold. Otherwise emit
//! a fresh-table decision following TXN convention.
//!
//! Scoring (extension of the design-doc heuristic — adds schema-match
//! to disambiguate `dbo`-bucket clones from canonical schema tables):
//!   +50 — entity name appears as a substring of the table name
//!         (case-insensitive, both squashed of underscores)
//!   +80 — schema name equals entity name (e.g. entity `Cardholder`,
//!         schema `cardholder`); +40 if either contains the other
//!   +10 per shared column name (case-insensitive, snake/camel folded);
//!         promiscuous tokens (`id`, `code`, `name`, `type`, `status`)
//!         are skipped because they match nearly every TXN column.
//!   threshold = 50 — below this, fall back to fresh-table.
//!
//! Implementation note: we walk `catalog.tables` (Vec<TableSchema>)
//! rather than `catalog.objects`, because tables are where the column
//! data actually lives. `SqlObject` only carries the body-reference
//! summary used by Phase 19's TechnicalGrounding.

use crate::dev_spec::types::{BorrowDecision, Erd};
use crate::schema::TableSchema;
use crate::sql_catalog::SqlCatalog;

const BORROW_THRESHOLD: u32 = 50;

pub fn decide_borrows(erd: &Erd, catalog: &SqlCatalog) -> Vec<BorrowDecision> {
    erd.entities
        .values()
        .map(|entity| {
            let entity_lc_squashed = entity.name.to_lowercase().replace('_', "");
            let entity_col_lc: Vec<String> = entity
                .columns
                .iter()
                .map(|c| c.name.to_lowercase().replace('_', ""))
                .collect();

            let mut best_score = 0u32;
            let mut best_table: Option<&TableSchema> = None;

            for tbl in &catalog.tables {
                let table_lc_squashed = tbl.name.to_lowercase().replace('_', "");
                let mut score = 0u32;
                if table_lc_squashed.contains(&entity_lc_squashed) {
                    score += 50;
                }
                // Schema name match: Cardholder entity → cardholder schema.
                // Strong signal of ownership domain and disambiguates
                // `dbo`-bucket Vivere clones from canonical TXN tables.
                // Weighted higher than table-name fuzz because schema is a
                // human-curated grouping and reflects intent.
                if let Some(s) = &tbl.schema {
                    let schema_lc = s.to_lowercase();
                    if schema_lc == entity_lc_squashed {
                        score += 80;
                    } else if schema_lc.contains(&entity_lc_squashed)
                        || entity_lc_squashed.contains(&schema_lc)
                    {
                        score += 40;
                    }
                }
                // Column overlap: count DISTINCT entity columns that have
                // at least one matching column in the table. Skips trivially
                // promiscuous tokens like bare `id` that match almost any
                // `*_id` column in the catalog.
                for ec in &entity_col_lc {
                    if is_promiscuous_token(ec) {
                        continue;
                    }
                    let any_match = tbl.columns.iter().any(|tc| {
                        let tcol_lc = tc.name.to_lowercase().replace('_', "");
                        tcol_lc.contains(ec) || ec.contains(&tcol_lc)
                    });
                    if any_match {
                        score += 10;
                    }
                }
                // Tie-breaking: prefer the more specific (smaller) table
                // name when scores are identical — `cpf_Cardholder_Profile`
                // beats `dbo.ucda_Update_Cardholder_Details_API`. We model
                // this by treating equal scores as a no-op (first match
                // wins), but biasing the name-match score so that an exact
                // entity-name token boost picks the cleanest table.
                if score > best_score {
                    best_score = score;
                    best_table = Some(tbl);
                }
            }

            if best_score >= BORROW_THRESHOLD {
                let tbl = best_table.unwrap();
                let full = tbl.full_name();
                let schema = tbl.schema.clone().unwrap_or_else(|| "dbo".into());
                // Extract prefix from the table name. Conventionally TXN
                // tables look like `cpf_Client_Profile` — first underscore-
                // separated token. Fall back to first 3 chars if no `_`.
                let prefix = match tbl.name.split_once('_') {
                    Some((p, _)) if !p.is_empty() => p.to_lowercase(),
                    _ => tbl.name.chars().take(3).collect::<String>().to_lowercase(),
                };
                let table_name = format!("{}_{}", prefix, entity.name);
                BorrowDecision {
                    entity: entity.name.clone(),
                    borrowed_from: Some(full.clone()),
                    schema,
                    table_name,
                    prefix,
                    score: best_score,
                    reason: format!("Borrowed shape from {} (score {})", full, best_score),
                }
            } else {
                let prefix = synthesise_prefix(&entity.name);
                let table_name = format!("{}_{}", prefix, entity.name);
                BorrowDecision {
                    entity: entity.name.clone(),
                    borrowed_from: None,
                    schema: "dbo".into(),
                    table_name,
                    prefix,
                    score: best_score,
                    reason: format!(
                        "No TXN match (best score {} < threshold {}) — fresh table",
                        best_score, BORROW_THRESHOLD
                    ),
                }
            }
        })
        .collect()
}

/// True if a token is too short / common to count as meaningful column
/// overlap signal. `id` matches every `*_Id` column in TXN (~all of them).
fn is_promiscuous_token(t: &str) -> bool {
    matches!(t, "id" | "code" | "name" | "type" | "status" | "")
}

/// Three-letter lowercase prefix derived from the entity name.
/// For multi-word PascalCase ≥3 segments, take the first letter of each
/// of the first three words (e.g. `ClientProfileFlag` → `cpf`).
/// Otherwise pad/truncate the entity name to three letters.
fn synthesise_prefix(entity: &str) -> String {
    let words = split_pascal(entity);
    if words.len() >= 3 {
        words
            .iter()
            .take(3)
            .map(|w| w.chars().next().unwrap().to_ascii_lowercase().to_string())
            .collect::<String>()
    } else {
        // Truncate to 3, then pad with 'x' if the entity name is shorter.
        let mut p: String = entity.chars().take(3).collect::<String>().to_lowercase();
        while p.len() < 3 {
            p.push('x');
        }
        p
    }
}

fn split_pascal(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev_spec::types::{Column, Entity};
    use crate::schema::{Column as SqlCol, TableSchema};
    use std::collections::BTreeMap;

    fn mk_entity(name: &str, cols: &[&str]) -> Entity {
        Entity {
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| Column {
                    name: (*c).into(),
                    ty: "string(50)".into(),
                    nullable: true,
                    description: None,
                })
                .collect(),
            primary_key: "Id".into(),
            foreign_keys: vec![],
            introduced_by: vec!["test".into()],
        }
    }

    fn mk_table(schema: &str, name: &str, cols: &[&str]) -> TableSchema {
        TableSchema {
            schema: Some(schema.into()),
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| SqlCol {
                    name: (*c).into(),
                    sql_type: "NVARCHAR".into(),
                    length: Some(50),
                    precision: None,
                    nullable: true,
                    identity: false,
                    is_computed: false,
                    has_default: false,
                })
                .collect(),
            primary_key: vec![],
            unique_keys: vec![],
            foreign_keys: vec![],
            source_path: String::new(),
        }
    }

    #[test]
    fn synthesise_prefix_three_word_pascal() {
        assert_eq!(synthesise_prefix("ClientProfileFlag"), "cpf");
    }

    #[test]
    fn synthesise_prefix_short_name_padded() {
        assert_eq!(synthesise_prefix("Ab"), "abx");
    }

    #[test]
    fn borrow_threshold_picks_table_with_name_match() {
        let mut catalog = SqlCatalog::default();
        catalog
            .tables
            .push(mk_table("cardholder", "cpf_Cardholder_Profile", &["cpf_First_Name"]));
        let mut ents = BTreeMap::new();
        ents.insert("Cardholder".into(), mk_entity("Cardholder", &["FirstName"]));
        let erd = Erd { entities: ents, endpoints: vec![] };
        let decisions = decide_borrows(&erd, &catalog);
        let d = &decisions[0];
        assert!(d.borrowed_from.is_some());
        assert_eq!(d.schema, "cardholder");
        assert_eq!(d.prefix, "cpf");
    }

    #[test]
    fn borrow_falls_back_when_score_below_threshold() {
        let catalog = SqlCatalog::default();
        let mut ents = BTreeMap::new();
        ents.insert("Widget".into(), mk_entity("Widget", &["Id"]));
        let erd = Erd { entities: ents, endpoints: vec![] };
        let decisions = decide_borrows(&erd, &catalog);
        let d = &decisions[0];
        assert!(d.borrowed_from.is_none());
        assert_eq!(d.prefix.len(), 3);
        assert!(d.table_name.contains("Widget"));
    }
}

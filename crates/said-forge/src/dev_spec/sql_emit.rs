//! Emit `CREATE TABLE` SQL from ERD entities + borrow decisions.
//!
//! Format mirrors TXN convention (sample at
//! `dtcard/1-ground-truth/TXN/.../cpf_Client_Profile.sql`):
//!
//!   CREATE TABLE [schema].[prefix_Name] (
//!       [Pk_Column]  UNIQUEIDENTIFIER NOT NULL,
//!       [Field]      NVARCHAR (255)   NULL,
//!       [Created]    DATETIME         CONSTRAINT DF_*_Created DEFAULT (...) NULL,
//!       [Created_UTC] DATETIMEOFFSET (3) DEFAULT (getutcdate()) NULL,
//!       CONSTRAINT [PK_*] PRIMARY KEY CLUSTERED ([Pk_Column] ASC),
//!       CONSTRAINT [FK_*] FOREIGN KEY ([col]) REFERENCES [schema].[parent] ([id]),
//!   );
//!
//! Wrapped in `IF NOT EXISTS (SELECT * FROM sys.tables WHERE ...)` for
//! idempotency per the design doc's data-preservation rules.

use crate::dev_spec::types::{BorrowDecision, Entity, Erd};

pub fn emit_create_table(
    entity: &Entity,
    decision: &BorrowDecision,
    _decisions: &[BorrowDecision],
) -> String {
    let schema = &decision.schema;
    let table = &decision.table_name;
    let mut out = String::new();
    out.push_str(&format!(
        "IF NOT EXISTS (SELECT * FROM sys.tables t \
         INNER JOIN sys.schemas s ON s.schema_id = t.schema_id \
         WHERE s.name = '{schema}' AND t.name = '{table}')\nBEGIN\n"
    ));
    out.push_str(&format!("CREATE TABLE [{schema}].[{table}] (\n"));
    let mut lines: Vec<String> = Vec::new();
    for col in &entity.columns {
        let null = if col.nullable { "NULL" } else { "NOT NULL" };
        let sql_ty = canonical_to_sql(&col.ty);
        lines.push(format!("    [{}] {} {}", col.name, sql_ty, null));
    }
    // Audit columns mirroring TXN's pattern (Created / Created_UTC).
    // Skip when the entity already has columns with these names — Dev
    // Spec bodies sometimes carry a `created` timestamp field which
    // collides with the audit column at CREATE TABLE.
    let has_col = |needle: &str| {
        entity
            .columns
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(needle))
    };
    if !has_col("Created") {
        lines.push(format!(
            "    [Created]     DATETIME           CONSTRAINT [DF_{table}_Created] DEFAULT (getdate()) NULL"
        ));
    }
    if !has_col("Created_UTC") {
        lines.push(format!(
            "    [Created_UTC] DATETIMEOFFSET (3) CONSTRAINT [DF_{table}_Created_UTC] DEFAULT (getutcdate()) NULL"
        ));
    }
    // PK constraint.
    lines.push(format!(
        "    CONSTRAINT [PK_{table}] PRIMARY KEY CLUSTERED ([{}] ASC)",
        entity.primary_key
    ));
    // NOTE: FK constraints intentionally NOT emitted inline — they are
    // deferred to a second pass via `emit_alter_table_fks` so that
    // circular FK references (e.g. Programmanager <-> Settlement)
    // resolve correctly: every referenced table exists by the time
    // any FK constraint is added.
    out.push_str(&lines.join(",\n"));
    out.push_str("\n);\nEND\nGO\n");
    out
}

/// Emit ALTER TABLE ADD CONSTRAINT statements for all FKs on an
/// entity. Returns empty string when entity has no FKs. Each
/// constraint is wrapped in an existence guard for idempotency.
///
/// Deferring FKs to ALTER TABLE is the standard SQL idiom for
/// circular foreign-key references: every table exists by the time
/// any constraint is added.
pub fn emit_alter_table_fks(
    entity: &Entity,
    decision: &BorrowDecision,
    decisions: &[BorrowDecision],
    erd: &Erd,
) -> String {
    let mut out = String::new();
    let schema = &decision.schema;
    let table = &decision.table_name;
    for fk in &entity.foreign_keys {
        let target = decisions.iter()
            .find(|d| d.entity == fk.references_entity);
        let (target_schema, target_table) = match target {
            Some(d) => (d.schema.as_str(), d.table_name.as_str()),
            None => continue,  // skip orphan FKs silently in deferred pass
        };
        // Resolve the referenced column from the target entity's actual
        // PK rather than trusting the FK's stored `references_column`.
        // Root entities use `Id`; child entities use `<Name>Id`. The FK
        // harvest in erd.rs guesses `<Name>Id` for both, which breaks
        // references to root entities.
        let ref_col = match erd.entities.get(&fk.references_entity) {
            Some(e) => e.primary_key.as_str(),
            None => fk.references_column.as_str(),
        };
        let constraint_name = format!("FK_{}_{}_{}", table, fk.column, fk.references_entity);
        out.push_str(&format!(
            "IF NOT EXISTS (SELECT * FROM sys.foreign_keys WHERE name = '{constraint_name}')\nBEGIN\n\
             ALTER TABLE [{schema}].[{table}]\n\
             ADD CONSTRAINT [{constraint_name}] FOREIGN KEY ([{col}]) REFERENCES [{target_schema}].[{target_table}] ([{ref_col}]);\nEND\nGO\n",
            schema = schema,
            table = table,
            constraint_name = constraint_name,
            col = fk.column,
            target_schema = target_schema,
            target_table = target_table,
            ref_col = ref_col,
        ));
    }
    out
}

fn canonical_to_sql(ty: &str) -> String {
    if ty == "uuid" {
        "UNIQUEIDENTIFIER".into()
    } else if ty == "datetime" {
        "DATETIME".into()
    } else if ty == "int" {
        "INT".into()
    } else if ty == "bit" {
        "BIT".into()
    } else if let Some(n) = ty.strip_prefix("string(").and_then(|s| s.strip_suffix(')')) {
        format!("NVARCHAR ({n})")
    } else if let Some(n) = ty.strip_prefix("decimal(").and_then(|s| s.strip_suffix(')')) {
        format!("DECIMAL ({n})")
    } else {
        "NVARCHAR (255)".into()
    }
}

/// Emit ALL tables for an ERD as one SQL script. Tables ordered so
/// referenced entities precede referencing ones (topological FK sort).
pub fn emit_all_tables(
    erd: &crate::dev_spec::types::Erd,
    decisions: &[BorrowDecision],
) -> String {
    let mut out = String::from(
        "-- Generated table schema — Dev-Spec-driven.\n\
         -- Idempotent: re-running this script is safe.\n\
         -- See docs/superpowers/specs/2026-04-28-dev-spec-source-of-truth-design.md\n\n",
    );
    // Pass 1: tables (no inline FKs).
    let order = topo_sort_entities(erd);
    for ent_name in &order {
        if let (Some(entity), Some(decision)) = (
            erd.entities.get(ent_name),
            decisions.iter().find(|d| d.entity == *ent_name),
        ) {
            out.push_str(&emit_create_table(entity, decision, decisions));
            out.push('\n');
        }
    }
    // Pass 2: FK constraints — every referenced table now exists.
    out.push_str("\n-- Foreign-key constraints (deferred so circular references resolve)\n\n");
    for ent_name in &order {
        if let (Some(entity), Some(decision)) = (
            erd.entities.get(ent_name),
            decisions.iter().find(|d| d.entity == *ent_name),
        ) {
            out.push_str(&emit_alter_table_fks(entity, decision, decisions, erd));
        }
    }
    out
}

fn topo_sort_entities(erd: &crate::dev_spec::types::Erd) -> Vec<String> {
    use std::collections::{BTreeSet, VecDeque};
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut queue: VecDeque<String> = erd.entities.keys().cloned().collect();
    let mut iterations = 0usize;
    let max_iterations = erd.entities.len() * erd.entities.len() + 1;
    while let Some(name) = queue.pop_front() {
        iterations += 1;
        if iterations > max_iterations {
            // Cycle — emit remainder in deterministic order. Include
            // the just-popped `name` since `pop_front` already removed
            // it from the queue, otherwise it would be silently dropped
            // and any FK referencing it would be unresolvable.
            if !visited.contains(&name) {
                order.push(name);
            }
            for n in queue {
                if !visited.contains(&n) {
                    order.push(n);
                }
            }
            break;
        }
        if visited.contains(&name) {
            continue;
        }
        let entity = match erd.entities.get(&name) {
            Some(e) => e,
            None => continue,
        };
        let unmet: Vec<String> = entity
            .foreign_keys
            .iter()
            .filter(|fk| {
                erd.entities.contains_key(&fk.references_entity)
                    && !visited.contains(&fk.references_entity)
                    && fk.references_entity != name
            })
            .map(|fk| fk.references_entity.clone())
            .collect();
        if unmet.is_empty() {
            visited.insert(name.clone());
            order.push(name);
        } else {
            queue.push_back(name);
        }
    }
    order
}

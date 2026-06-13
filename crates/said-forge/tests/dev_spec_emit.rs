use said_forge::dev_spec::borrow::decide_borrows;
use said_forge::dev_spec::types::{Column, Entity, Erd};
use std::collections::BTreeMap;

#[test]
fn borrow_picks_cardholder_table_for_cardholder_entity() {
    use said_forge::sql_catalog::build_catalog;
    let root = std::path::Path::new("../../dtcard");
    if !root.exists() {
        eprintln!("skipping: dtcard not present in this checkout");
        return;
    }
    let catalog = build_catalog(root).unwrap();

    let mut entities = BTreeMap::new();
    entities.insert("Cardholder".to_string(), Entity {
        name: "Cardholder".into(),
        columns: vec![
            Column { name: "Id".into(), ty: "uuid".into(), nullable: false, description: None },
            Column { name: "FirstName".into(), ty: "string(255)".into(), nullable: true, description: None },
            Column { name: "LastName".into(), ty: "string(255)".into(), nullable: true, description: None },
            Column { name: "DateOfBirth".into(), ty: "datetime".into(), nullable: true, description: None },
        ],
        primary_key: "Id".into(),
        foreign_keys: vec![],
        introduced_by: vec!["test".into()],
    });
    let erd = Erd { entities, endpoints: vec![] };

    let decisions = decide_borrows(&erd, &catalog);
    let cardholder = decisions.iter().find(|d| d.entity == "Cardholder").unwrap();
    // Should borrow from cardholder.cpf_Client_Profile or similar.
    assert!(cardholder.borrowed_from.is_some(),
        "expected Cardholder to borrow a TXN table, got {:?}", cardholder);
    assert_eq!(cardholder.schema, "cardholder");
    assert!(cardholder.prefix.len() == 3);
}

#[test]
fn borrow_falls_back_to_fresh_table_when_no_match() {
    use said_forge::sql_catalog::SqlCatalog;
    let catalog = SqlCatalog::default();  // empty

    let mut entities = BTreeMap::new();
    entities.insert("Widget".to_string(), Entity {
        name: "Widget".into(),
        columns: vec![Column { name: "Id".into(), ty: "uuid".into(), nullable: false, description: None }],
        primary_key: "Id".into(),
        foreign_keys: vec![],
        introduced_by: vec!["test".into()],
    });
    let erd = Erd { entities, endpoints: vec![] };

    let decisions = decide_borrows(&erd, &catalog);
    let widget = decisions.iter().find(|d| d.entity == "Widget").unwrap();
    assert!(widget.borrowed_from.is_none());
    // Synthesised: a 3-letter prefix derived from the entity name.
    assert_eq!(widget.prefix.len(), 3);
    assert!(widget.table_name.contains("Widget"));
}

#[test]
fn emits_idempotent_create_table_with_audit_columns() {
    use said_forge::dev_spec::borrow::decide_borrows;
    use said_forge::dev_spec::sql_emit::emit_create_table;
    use said_forge::dev_spec::types::{Column, Entity, Erd};
    use said_forge::sql_catalog::SqlCatalog;
    use std::collections::BTreeMap;

    let mut entities = BTreeMap::new();
    entities.insert("Widget".to_string(), Entity {
        name: "Widget".into(),
        columns: vec![
            Column { name: "Id".into(), ty: "uuid".into(), nullable: false, description: Some("PK".into()) },
            Column { name: "Name".into(), ty: "string(255)".into(), nullable: false, description: None },
            Column { name: "Cost".into(), ty: "decimal(18,4)".into(), nullable: true, description: None },
        ],
        primary_key: "Id".into(),
        foreign_keys: vec![],
        introduced_by: vec!["test".into()],
    });
    let erd = Erd { entities, endpoints: vec![] };
    let decisions = decide_borrows(&erd, &SqlCatalog::default());
    let widget = erd.entities.get("Widget").unwrap();
    let decision = decisions.iter().find(|d| d.entity == "Widget").unwrap();
    let sql = emit_create_table(widget, decision, &decisions);

    // Idempotency guard.
    assert!(sql.contains("IF NOT EXISTS"));
    assert!(sql.contains("CREATE TABLE"));
    // Schema + table.
    assert!(sql.contains(&format!("[{}].", decision.schema)));
    assert!(sql.contains(&decision.table_name));
    // Columns.
    assert!(sql.contains("[Id]"));
    assert!(sql.contains("UNIQUEIDENTIFIER"));
    assert!(sql.contains("[Name]"));
    assert!(sql.contains("NVARCHAR (255)"));
    assert!(sql.contains("[Cost]"));
    assert!(sql.contains("DECIMAL (18,4)"));
    // Audit columns mirroring TXN convention.
    assert!(sql.contains("Created"));
    assert!(sql.contains("Created_UTC"));
    // PK constraint.
    assert!(sql.contains("PRIMARY KEY"));
}

#[test]
fn fk_references_resolve_through_borrow_decisions() {
    use said_forge::dev_spec::borrow::decide_borrows;
    use said_forge::dev_spec::sql_emit::emit_alter_table_fks;
    use said_forge::dev_spec::types::{Column, Entity, Erd, ForeignKey};
    use said_forge::sql_catalog::SqlCatalog;
    use std::collections::BTreeMap;

    let mut entities = BTreeMap::new();
    entities.insert("Owner".to_string(), Entity {
        name: "Owner".into(),
        columns: vec![
            Column { name: "Id".into(), ty: "uuid".into(), nullable: false, description: None },
        ],
        primary_key: "Id".into(),
        foreign_keys: vec![],
        introduced_by: vec!["test".into()],
    });
    entities.insert("Widget".to_string(), Entity {
        name: "Widget".into(),
        columns: vec![
            Column { name: "Id".into(), ty: "uuid".into(), nullable: false, description: None },
            Column { name: "OwnerId".into(), ty: "uuid".into(), nullable: false, description: None },
        ],
        primary_key: "Id".into(),
        foreign_keys: vec![ForeignKey {
            column: "OwnerId".into(),
            references_entity: "Owner".into(),
            references_column: "Id".into(),
        }],
        introduced_by: vec!["test".into()],
    });
    let erd = Erd { entities, endpoints: vec![] };
    let decisions = decide_borrows(&erd, &SqlCatalog::default());
    let widget = erd.entities.get("Widget").unwrap();
    let widget_decision = decisions.iter().find(|d| d.entity == "Widget").unwrap();

    // FK now emitted via ALTER TABLE in deferred pass.
    let alter_sql = emit_alter_table_fks(widget, widget_decision, &decisions, &erd);
    let owner_decision = decisions.iter().find(|d| d.entity == "Owner").unwrap();
    let expected = format!("REFERENCES [{}].[{}]", owner_decision.schema, owner_decision.table_name);
    assert!(alter_sql.contains(&expected),
        "ALTER FK should resolve to {}, got SQL:\n{}", expected, alter_sql);
    assert!(!alter_sql.contains("<resolve>"));
    assert!(alter_sql.contains("ALTER TABLE"));
    assert!(alter_sql.contains("ADD CONSTRAINT"));
}

#[test]
fn emit_all_tables_handles_circular_fks() {
    use said_forge::dev_spec::borrow::decide_borrows;
    use said_forge::dev_spec::sql_emit::emit_all_tables;
    use said_forge::dev_spec::types::{Column, Entity, Erd, ForeignKey};
    use said_forge::sql_catalog::SqlCatalog;
    use std::collections::BTreeMap;

    // Programmanager <-> Settlement circular reference.
    let mut entities = BTreeMap::new();
    entities.insert("Programmanager".to_string(), Entity {
        name: "Programmanager".into(),
        columns: vec![
            Column { name: "Id".into(), ty: "uuid".into(), nullable: false, description: None },
            Column { name: "SettlementId".into(), ty: "uuid".into(), nullable: false, description: None },
        ],
        primary_key: "Id".into(),
        foreign_keys: vec![ForeignKey {
            column: "SettlementId".into(),
            references_entity: "Settlement".into(),
            references_column: "Id".into(),
        }],
        introduced_by: vec!["test".into()],
    });
    entities.insert("Settlement".to_string(), Entity {
        name: "Settlement".into(),
        columns: vec![
            Column { name: "Id".into(), ty: "uuid".into(), nullable: false, description: None },
            Column { name: "ProgrammanagerId".into(), ty: "uuid".into(), nullable: false, description: None },
        ],
        primary_key: "Id".into(),
        foreign_keys: vec![ForeignKey {
            column: "ProgrammanagerId".into(),
            references_entity: "Programmanager".into(),
            references_column: "Id".into(),
        }],
        introduced_by: vec!["test".into()],
    });
    let erd = Erd { entities, endpoints: vec![] };
    let decisions = decide_borrows(&erd, &SqlCatalog::default());
    let sql = emit_all_tables(&erd, &decisions);

    // Both tables MUST be created BEFORE either FK is added.
    let create_pm = sql.find("CREATE TABLE [dbo].[pro_Programmanager]").unwrap();
    let create_set = sql.find("CREATE TABLE [dbo].[set_Settlement]").unwrap();
    let alter_pm = sql.find("ALTER TABLE [dbo].[pro_Programmanager]").unwrap();
    let alter_set = sql.find("ALTER TABLE [dbo].[set_Settlement]").unwrap();
    assert!(create_pm < alter_pm, "Programmanager CREATE must come before its ALTER FK");
    assert!(create_set < alter_pm, "Settlement CREATE must come before Programmanager's ALTER FK");
    assert!(create_pm < alter_set, "Programmanager CREATE must come before Settlement's ALTER FK");
}

#[test]
fn registry_amend_emits_merge_for_each_endpoint() {
    use said_forge::dev_spec::registry_amend::emit_registry_merge;
    use said_forge::dev_spec::types::{DevSpecEndpoint, Erd};
    use std::collections::BTreeMap;

    let endpoints = vec![
        DevSpecEndpoint {
            method: "POST".into(),
            path: "/cardholders".into(),
            summary: "Create".into(),
            source_file: "POST-cardholders-createCardholder.md".into(),
            path_params: vec![],
            request_body: None,
            response_body: None,
        },
        DevSpecEndpoint {
            method: "GET".into(),
            path: "/cardholders/{cardholder_id}".into(),
            summary: "Get".into(),
            source_file: "GET-cardholders-getCardholder.md".into(),
            path_params: vec![],
            request_body: None,
            response_body: None,
        },
    ];
    let erd = Erd { entities: BTreeMap::new(), endpoints };

    let sql = emit_registry_merge(&erd);
    // One MERGE per endpoint, keyed on (path, aml_Code).
    assert!(sql.matches("MERGE").count() >= 2);
    assert!(sql.contains("'/cardholders'") && sql.contains("'POST'"));
    assert!(sql.contains("'/cardholders/{cardholder_id}'") && sql.contains("'GET'"));
    // Defaults from design doc.
    assert!(sql.contains("NEWID()"));
    assert!(sql.contains("GETDATE()"));
    assert!(sql.contains("GETUTCDATE()"));
    // WHEN MATCHED → no-op (do nothing).
    assert!(sql.contains("WHEN MATCHED THEN")
        || sql.contains("/* no-op when matched */"));
}

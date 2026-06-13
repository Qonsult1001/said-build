use said_forge::dev_spec::erd::derive_erd;
use said_forge::dev_spec::parser::parse_endpoint_file;
use said_forge::OpenApiStandard;

#[test]
fn derives_cardholder_entity_from_post_create() {
    let path = "tests/fixtures/dev_spec/POST-cardholders-createCardholder.md";
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(std::path::Path::new(path), &std_def).unwrap();
    let erd = derive_erd(&[ep]);

    // Top-level body becomes the Cardholder entity.
    let cardholder = erd.entities.get("Cardholder")
        .expect("Cardholder entity derived");
    let pk = &cardholder.primary_key;
    assert_eq!(pk, "Id");
    let id_col = cardholder.columns.iter().find(|c| c.name == "Id").unwrap();
    assert_eq!(id_col.ty, "uuid");

    // Nested object groups become child entities with FK back to parent.
    let address = erd.entities.get("BillingAddress")
        .expect("BillingAddress entity derived");
    assert!(address.foreign_keys.iter().any(|fk|
        fk.references_entity == "Cardholder"));

    // PersonalDetails too.
    assert!(erd.entities.contains_key("PersonalDetails"));

    // Endpoints copied through.
    assert_eq!(erd.endpoints.len(), 1);
}

#[test]
fn erd_emits_canonical_json_and_mermaid() {
    use said_forge::dev_spec::erd::{derive_erd, render_mermaid, to_canonical_json};
    use said_forge::dev_spec::parser::parse_endpoint_file;

    let path = "tests/fixtures/dev_spec/POST-cardholders-createCardholder.md";
    let std_def = OpenApiStandard::defaults();
    let ep = parse_endpoint_file(std::path::Path::new(path), &std_def).unwrap();
    let erd = derive_erd(&[ep]);

    // JSON is deterministic — same input two passes produces same bytes.
    let json1 = to_canonical_json(&erd).unwrap();
    let json2 = to_canonical_json(&erd).unwrap();
    assert_eq!(json1, json2);
    // It contains the Cardholder entity.
    assert!(json1.contains("\"Cardholder\""));

    // Mermaid block is valid-ish: starts with the erDiagram fence.
    let md = render_mermaid(&erd);
    assert!(md.starts_with("```mermaid\nerDiagram\n"));
    assert!(md.contains("CARDHOLDER {"));
    assert!(md.ends_with("```\n"));
}

#[test]
fn pascal_field_normalises_all_conventions() {
    use said_forge::dev_spec::erd::pascal_field;

    assert_eq!(pascal_field("userName"), "UserName");
    assert_eq!(pascal_field("username"), "Username");
    assert_eq!(pascal_field("Username"), "Username");
    assert_eq!(pascal_field("user_name"), "UserName");
    assert_eq!(pascal_field("user-name"), "UserName");
    assert_eq!(pascal_field("USER_NAME"), "UserName");

    // Compound: account_owner_id, accountOwnerId, AccountOwnerId all match.
    assert_eq!(pascal_field("account_owner_id"), "AccountOwnerId");
    assert_eq!(pascal_field("accountOwnerId"), "AccountOwnerId");
    assert_eq!(pascal_field("AccountOwnerId"), "AccountOwnerId");

    // Already-canonical pass through.
    assert_eq!(pascal_field("Id"), "Id");
    assert_eq!(pascal_field("FirstName"), "FirstName");
}

#[test]
fn harvest_dedups_columns_across_naming_conventions() {
    // Two endpoints contribute fields with the same logical name in
    // different conventions. The merged entity must collapse them
    // into one canonical PascalCase column — not two.
    use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecSchema};
    use std::collections::BTreeMap;

    fn scalar(ty: &str) -> DevSpecSchema {
        DevSpecSchema {
            ty: ty.into(),
            format: None,
            description: None,
            example: None,
            properties: BTreeMap::new(),
            items: None,
        }
    }
    fn obj(props: Vec<(&str, DevSpecSchema)>) -> DevSpecSchema {
        let mut m = BTreeMap::new();
        for (k, v) in props {
            m.insert(k.to_string(), v);
        }
        DevSpecSchema {
            ty: "object".into(),
            format: None,
            description: None,
            example: None,
            properties: m,
            items: None,
        }
    }

    // Endpoint A: camelCase userName + accountOwnerId.
    let ep_a = DevSpecEndpoint {
        method: "POST".into(),
        path: "/webhooks".into(),
        summary: "create webhook A".into(),
        source_file: "A.md".into(),
        path_params: vec![],
        request_body: Some(obj(vec![
            ("userName", scalar("string")),
            ("accountOwnerId", DevSpecSchema {
                ty: "string".into(),
                format: Some("uuid".into()),
                description: None,
                example: None,
                properties: BTreeMap::new(),
                items: None,
            }),
        ])),
        response_body: None,
    };
    // Endpoint B: lowercase username + snake_case account_owner_id.
    let ep_b = DevSpecEndpoint {
        method: "PUT".into(),
        path: "/webhooks".into(),
        summary: "update webhook B".into(),
        source_file: "B.md".into(),
        path_params: vec![],
        request_body: Some(obj(vec![
            ("username", scalar("string")),
            ("account_owner_id", DevSpecSchema {
                ty: "string".into(),
                format: Some("uuid".into()),
                description: None,
                example: None,
                properties: BTreeMap::new(),
                items: None,
            }),
        ])),
        response_body: None,
    };

    let erd = derive_erd(&[ep_a, ep_b]);
    let webhook = erd.entities.get("Webhook").expect("Webhook entity");

    // Collect column names.
    let names: Vec<&str> = webhook.columns.iter().map(|c| c.name.as_str()).collect();

    // UserName must appear exactly once — not "Username" and "UserName" both.
    let user_count = names
        .iter()
        .filter(|n| n.eq_ignore_ascii_case("UserName"))
        .count();
    assert_eq!(user_count, 1, "expected one UserName-ish column, got {names:?}");

    // AccountOwnerId must appear exactly once.
    let owner_count = names
        .iter()
        .filter(|n| n.eq_ignore_ascii_case("AccountOwnerId"))
        .count();
    assert_eq!(owner_count, 1, "expected one AccountOwnerId-ish column, got {names:?}");

    // And the canonical names must be the PascalCase form.
    assert!(names.contains(&"UserName"), "missing canonical UserName in {names:?}");
    assert!(names.contains(&"AccountOwnerId"), "missing canonical AccountOwnerId in {names:?}");
}

#[test]
fn transition_endpoint_creates_parent_bound_child_entity() {
    use said_forge::dev_spec::erd::derive_erd;
    use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecParam, DevSpecSchema};
    use std::collections::BTreeMap;

    let endpoint = DevSpecEndpoint {
        method: "POST".into(),
        path: "/cardholders/{cardholder_id}/transitions".into(),
        summary: "Transition a cardholder".into(),
        source_file: "POST-cardholders-_cardholder_id_-transitions.md".into(),
        path_params: vec![DevSpecParam {
            name: "cardholder_id".into(),
            ty: "uuid".into(),
            required: true,
            description: String::new(),
            ..Default::default()
        }],
        request_body: Some(DevSpecSchema {
            ty: "object".into(),
            format: None,
            description: None,
            example: None,
            properties: {
                let mut p = BTreeMap::new();
                p.insert("status".into(), DevSpecSchema {
                    ty: "string".into(),
                    format: None,
                    description: None,
                    example: None,
                    properties: BTreeMap::new(),
                    items: None,
                });
                p
            },
            items: None,
        }),
        response_body: None,
    };
    let erd = derive_erd(&[endpoint]);

    assert!(erd.entities.contains_key("Cardholder_Transition"),
        "expected Cardholder_Transition entity, got: {:?}",
        erd.entities.keys().collect::<Vec<_>>());
    assert!(!erd.entities.contains_key("Transition"),
        "should NOT have flat Transition entity");

    let ct = erd.entities.get("Cardholder_Transition").unwrap();
    assert!(ct.foreign_keys.iter().any(|fk|
        fk.references_entity == "Cardholder"),
        "Cardholder_Transition must FK to Cardholder, got FKs: {:?}",
        ct.foreign_keys);
}

#[test]
fn mermaid_uses_upper_snake_and_relationships_first() {
    use said_forge::dev_spec::erd::{derive_erd, render_mermaid};
    use said_forge::dev_spec::types::{DevSpecEndpoint, DevSpecParam, DevSpecSchema};
    use std::collections::BTreeMap;

    let endpoint = DevSpecEndpoint {
        method: "POST".into(),
        path: "/cardholders/{cardholder_id}/transitions".into(),
        summary: "".into(),
        source_file: "POST-cardholders-_cardholder_id_-transitions.md".into(),
        path_params: vec![DevSpecParam {
            name: "cardholder_id".into(),
            ty: "uuid".into(),
            required: true,
            description: String::new(),
            ..Default::default()
        }],
        request_body: Some(DevSpecSchema {
            ty: "object".into(),
            format: None,
            description: None,
            example: None,
            properties: BTreeMap::new(),
            items: None,
        }),
        response_body: None,
    };
    let erd = derive_erd(&[endpoint]);
    let md = render_mermaid(&erd);

    // Upper-snake names.
    assert!(md.contains("CARDHOLDER {"),
        "Cardholder should display as CARDHOLDER, got:\n{md}");
    assert!(md.contains("CARDHOLDER_TRANSITION {"),
        "Cardholder_Transition should display as CARDHOLDER_TRANSITION");

    // Relationships before entity blocks.
    let rel_pos = md.find("CARDHOLDER ||--o{ CARDHOLDER_TRANSITION").unwrap();
    let entity_pos = md.find("CARDHOLDER {").unwrap();
    assert!(rel_pos < entity_pos,
        "relationships must come before entity blocks");

    // Lifecycle label for transition relationships.
    assert!(md.contains(": \"lifecycle\""),
        "transition relationship must use 'lifecycle' label, got:\n{md}");
    // No `: has` for the transition relationship.
    assert!(!md.contains("CARDHOLDER_TRANSITION : \"has\""));
}

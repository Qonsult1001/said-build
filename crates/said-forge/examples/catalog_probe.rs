//! Dump the SQL catalog for a workspace. For smoke-testing Phase 19 on real
//! data.
//!
//! Run: `cargo run -p said-forge --example catalog_probe -- <workspace-root>`

fn main() {
    let root = std::env::args().nth(1).expect("pass workspace root");
    let root = std::path::Path::new(&root);
    let cat = said_forge::sql_catalog::build_catalog(root).expect("build catalog");
    println!("=== Catalog for {} ===", root.display());
    println!("Tables: {}", cat.tables.len());
    for t in &cat.tables {
        println!(
            "  {} ({} cols, {} FKs)",
            t.full_name(),
            t.columns.len(),
            t.foreign_keys.len()
        );
    }
    println!("\nOther SQL objects: {}", cat.objects.len());
    let mut by_kind: std::collections::BTreeMap<String, Vec<&said_forge::sql_catalog::SqlObject>> =
        Default::default();
    for o in &cat.objects {
        by_kind.entry(format!("{:?}", o.kind)).or_default().push(o);
    }
    for (kind, objs) in &by_kind {
        println!("  {}: {}", kind, objs.len());
        for o in objs.iter().take(5) {
            let refs: Vec<String> = o
                .referenced_tables
                .iter()
                .take(3)
                .cloned()
                .collect();
            println!(
                "    - {}  refs: [{}{}]",
                o.full_name(),
                refs.join(", "),
                if o.referenced_tables.len() > 3 {
                    format!(", +{} more", o.referenced_tables.len() - 3)
                } else {
                    String::new()
                }
            );
        }
        if objs.len() > 5 {
            println!("    ... and {} more", objs.len() - 5);
        }
    }
}

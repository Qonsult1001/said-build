//! Step 10 — execution-level testing of the OpenAPI contract.
//!
//! Spins up a synthetic HTTP server (axum) backed by direct stored
//! procedure execution against the running sandbox. The server reads
//! proc bindings from the OpenAPI spec, maps incoming requests to
//! `EXEC <proc>` calls, and returns the `Response_Message` JSON
//! envelope as the HTTP response.
//!
//! A test runner then drives the server with the `1. Local` Bruno
//! collection's bodies (parsed in Rust — no Bruno CLI needed) in
//! L3 lifecycle order: POST → GET-by-id → PUT → GET-by-id → LIST,
//! per entity, in the build order declared by Dev Spec README.
//!
//! Stop-at-first-failure (5A): if any step fails, the run halts.
//! The operator fixes, re-runs.

#![cfg(feature = "forge-sql-verify")]

pub mod build_order;
pub mod bru_parse;
pub mod lifecycle;
pub mod proc_invoke;
pub mod report;
pub mod server;

use crate::sql_verify::SandboxInfo;
use std::path::Path;

/// Run the full test pass.
///
///   1. Parse OpenAPI spec → collect proc bindings per (path, method)
///   2. Read Bruno `1. Local` collection → fixtures per operation
///   3. Spin up axum on `127.0.0.1:5050` with proc-EXEC handlers
///   4. Walk build order, run L3 lifecycle per entity
///   5. Halt on first failure
///   6. Write `5-deliverables/<client>/test-report.md`
pub async fn run_test_pass(
    sandbox: &SandboxInfo,
    workspace_root: &Path,
    deliverables_root: &Path,
    bruno_collection_roots: &[std::path::PathBuf],
) -> Result<report::TestRunSummary, String> {
    let spec_path = deliverables_root.join("api-specification.generated.yml");
    if !spec_path.exists() {
        return Err(format!("spec not found at {}", spec_path.display()));
    }
    let spec_text = std::fs::read_to_string(&spec_path)
        .map_err(|e| format!("read spec: {}", e))?;
    let spec: serde_yaml::Value = serde_yaml::from_str(&spec_text)
        .map_err(|e| format!("parse spec: {}", e))?;

    // 1. Bindings.
    let bindings = proc_invoke::collect_bindings_from_spec(&spec);
    eprintln!("  → {} proc bindings discovered from spec", bindings.len());
    // Spec-paths set used by lifecycle to flag fixtures whose URL has
    // no matching OpenAPI path (catches Bruno-spec drift early instead
    // of silently dropping fixtures).
    let spec_paths = lifecycle::spec_paths_from_yaml(&spec);
    eprintln!("  → {} (method, path) entries harvested from spec", spec_paths.len());

    // 2. Bruno fixtures, merged across every root provided. Earlier
    // roots win on (entity_folder, name) collision — i.e. the
    // hand-authored `dt/<CLIENT>/feapiTxnGlobal/.bruno/...` always
    // wins over the generated `dtcard/.../bruno-generated/...` so
    // operator edits never get masked by stale generator output.
    //
    // Dedup key normalises the folder name to its singular lowercase
    // form so plural/singular variants (`Products` vs `Product`,
    // `Bins` vs `Bin`) are treated as the same entity. Without this,
    // the same `CreateProduct.bru` shows up TWICE in the run when it
    // exists in both roots under different folder casings — the new
    // lifecycle runs every fixture, so the duplicate POST hits the
    // proc twice and the second call errors on duplicate-key.
    let mut fixtures: Vec<bru_parse::BruRequest> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    for root in bruno_collection_roots {
        if !root.exists() { continue; }
        let loaded = bru_parse::load_collection(root).unwrap_or_default();
        for f in loaded {
            let folder_key = crate::dev_spec::parser::singularise(
                &f.entity_folder.to_lowercase(),
            );
            let key = (folder_key, f.name.to_lowercase());
            if seen.insert(key) {
                fixtures.push(f);
            }
        }
        eprintln!("  → loaded fixtures from {}", root.display());
    }
    eprintln!("  → {} Bruno request fixtures loaded (deduped across roots)", fixtures.len());

    // 3. Server.
    let port = 5050u16;
    let server_handle = server::spawn(sandbox.clone(), bindings.clone(), port).await?;
    eprintln!("  → synthetic HTTP server listening on http://127.0.0.1:{}", port);

    // 4. Build order + lifecycle. Walk every entity, skip-and-continue:
    // a single broken entity never blocks the rest of the matrix from
    // running. Each entity's outcome is categorised in the report
    // (green / missing-fixture / broken-step) so the operator sees the
    // full Phase A picture in one pass.
    let order = build_order::load_or_default(workspace_root);
    let mut summary = report::TestRunSummary::default();
    summary.entity_count_total = order.len();
    let base_url = format!("http://127.0.0.1:{}", port);
    // Session-scoped id registry: each entity that successfully POSTs
    // a row drops its captured id here so later entities can reference
    // it (e.g. ProgramManager's body uses `{{binSponsorId}}` populated
    // from BinSponsor's earlier POST in the same run). Build order
    // ensures dependencies are created before they're referenced.
    let mut session_ids: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let seed_dir = workspace_root.join(".forge");
    for entity in &order {
        let result = lifecycle::run_for_entity(
            entity, &fixtures, &base_url, &mut session_ids, &spec_paths,
        ).await;
        // Hook on POST success rather than full-lifecycle pass:
        // downstream entities depend on rows that the POST step created,
        // not on PUT/GET/list succeeding. A broken PUT (e.g. BinRange's
        // deferred bit/varchar bug) shouldn't starve Card POST of acn
        // rows. Bucket-1 (root POST) labels start with "POST create"
        // — match by prefix so the path-hint suffix doesn't break this
        // detection.
        let post_succeeded = result.operations.first()
            .map(|op| op.label.starts_with("POST create") && op.passed)
            .unwrap_or(false);
        summary.entities.push(result);

        // After-entity seed hook: if `.forge/test-data-seed-after-<Entity>.sql`
        // exists AND the entity's POST step passed, apply it. This lets a
        // downstream entity rely on rows created here (e.g. Card POST
        // needs `cards.acn_All_Card_No` rows tied to the fbl_Code/
        // crv_Range_Id produced by BinRange POST). The file is opt-in
        // per entity — absence is fine, no log spam.
        if post_succeeded {
            let seed_path = seed_dir.join(format!("test-data-seed-after-{}.sql", entity));
            if seed_path.exists() {
                match std::fs::read_to_string(&seed_path) {
                    Ok(sql) => {
                        if let Err(e) = crate::sql_verify::apply_sql_script(
                            sandbox.host_port, &sql,
                        ).await {
                            eprintln!(
                                "  ! seed-after-{} failed: {}",
                                entity, e,
                            );
                        } else {
                            eprintln!("  → applied seed-after-{}", entity);
                        }
                    }
                    Err(e) => eprintln!(
                        "  ! could not read {}: {}", seed_path.display(), e,
                    ),
                }
            }
        }
    }

    // 5. Stop server.
    server_handle.shutdown();

    // 6. Reports.
    report::write_artifacts(&summary, deliverables_root, workspace_root)?;
    Ok(summary)
}

//! MEASUREMENT (not pass/fail): can .said's 1-bit fingerprint separate INTENT?
//!
//! The fix-replay ceiling: "add an endpoint" and "document an endpoint" share
//! all nouns, so whole-ticket 1-bit fingerprints can't tell them apart — a docs
//! distractor outscores a real paraphrase.
//!
//! Hypothesis: fingerprinting the ACTION/intent phrase SEPARATELY from the
//! target nouns, then requiring BOTH to match, separates intent — all within the
//! 1-bit substrate (no full floats, no Milvus).
//!
//! This test records fixcases and prints raw rank_by_fingerprint similarities for
//! three query styles, so we can SEE whether separation exists before building
//! the two-field schema. Run with: cargo test -p sca-core --no-default-features
//!   --features "code,static-embed" --test test_intent_separation -- --nocapture

use sca_core::said_file::SaidFile;

use sca_core::ask::action_residue;

fn sim_for_label(brain: &mut SaidFile, query: &str, want_doc: &str) -> f32 {
    let ranked = brain.rank_by_fingerprint(query, 50);
    ranked.into_iter().find(|(d, _)| d == want_doc).map(|(_, s)| s).unwrap_or(0.0)
}

#[test]
fn measure_intent_separation() {
    let path = "test_intent_sep.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load for this measurement");

    // Three fixcases. We store them as plain docs (the fingerprint is over the
    // whole text); doc_id encodes the intent class for readability.
    // WHOLE-TICKET style (the current 0.9.0 approach):
    brain.put("ADD_whole",  "Add a GET /api/cores endpoint that returns ProcessorCount, anonymous", None);
    brain.put("DOC_whole",  "Document the /api/cores endpoint in the OpenAPI spec and route table", None);
    brain.put("REN_whole",  "Rename the ProcessorCount variable in the logging module for clarity", None);

    // ACTION-ONLY style (intent isolated from target nouns):
    brain.put("ADD_action", "add create expose new", None);
    brain.put("DOC_action", "document describe write documentation for", None);
    brain.put("REN_action", "rename refactor change identifier", None);

    brain.build_index().expect("build_index");

    println!("\n===== WHOLE-TICKET fingerprint (current 0.9.0 approach) =====");
    // A real ADD paraphrase vs the queries — does ADD_whole beat DOC_whole?
    let q_add_para = "Expose the CPU processor count via a new public unauthenticated GET route";
    let add_w = sim_for_label(&mut brain, q_add_para, "ADD_whole");
    let doc_w = sim_for_label(&mut brain, q_add_para, "DOC_whole");
    let ren_w = sim_for_label(&mut brain, q_add_para, "REN_whole");
    println!("query (ADD paraphrase): \"{}\"", q_add_para);
    println!("  ADD_whole = {:.4}   DOC_whole = {:.4}   REN_whole = {:.4}", add_w, doc_w, ren_w);
    println!("  -> {} (want ADD_whole highest)",
        if add_w > doc_w && add_w > ren_w { "SEPARATES" } else { "FAILS to separate" });

    println!("\n===== ACTION-ONLY fingerprint (intent isolated) =====");
    // Isolate the action verbs from the same paraphrase.
    let q_add_action = "expose add public";
    let add_a = sim_for_label(&mut brain, q_add_action, "ADD_action");
    let doc_a = sim_for_label(&mut brain, q_add_action, "DOC_action");
    let ren_a = sim_for_label(&mut brain, q_add_action, "REN_action");
    println!("query (ADD action verbs): \"{}\"", q_add_action);
    println!("  ADD_action = {:.4}   DOC_action = {:.4}   REN_action = {:.4}", add_a, doc_a, ren_a);
    println!("  -> {} (want ADD_action highest)",
        if add_a > doc_a && add_a > ren_a { "SEPARATES" } else { "FAILS to separate" });

    // Cross-check: a DOC query on action verbs should pick DOC_action.
    let q_doc_action = "write documentation describe";
    let d_add = sim_for_label(&mut brain, q_doc_action, "ADD_action");
    let d_doc = sim_for_label(&mut brain, q_doc_action, "DOC_action");
    let d_ren = sim_for_label(&mut brain, q_doc_action, "REN_action");
    println!("query (DOC action verbs): \"{}\"", q_doc_action);
    println!("  ADD_action = {:.4}   DOC_action = {:.4}   REN_action = {:.4}", d_add, d_doc, d_ren);
    println!("  -> {} (want DOC_action highest)",
        if d_doc > d_add && d_doc > d_ren { "SEPARATES" } else { "FAILS to separate" });

    // ===== AUTO-SPLIT: can .said derive the action field WITHOUT the user
    // separating it? Target = code-identifier-shaped tokens; action = residue.
    // This is the production split (user passes one plain string).
    println!("\n===== AUTO-SPLIT residue fingerprint (production approach) =====");
    let mut brain2 = SaidFile::create("test_intent_sep2.said");
    assert!(brain2.auto_load_encoder());
    brain2.put("ADD_resid", &action_residue("Add a GET /api/cores endpoint that returns ProcessorCount, anonymous"), None);
    brain2.put("DOC_resid", &action_residue("Document the /api/cores endpoint in the OpenAPI spec and route table"), None);
    brain2.put("REN_resid", &action_residue("Rename the ProcessorCount variable in the logging module for clarity"), None);
    brain2.build_index().expect("build_index2");

    let q_add_resid = action_residue("Expose the CPU processor count via a new public unauthenticated GET route");
    let ra = sim_for_label(&mut brain2, &q_add_resid, "ADD_resid");
    let rd = sim_for_label(&mut brain2, &q_add_resid, "DOC_resid");
    let rr = sim_for_label(&mut brain2, &q_add_resid, "REN_resid");
    println!("residue(ADD paraphrase) = \"{}\"", q_add_resid);
    println!("  ADD_resid = {:.4}   DOC_resid = {:.4}   REN_resid = {:.4}", ra, rd, rr);
    println!("  -> {} (want ADD_resid highest)",
        if ra > rd && ra > rr { "SEPARATES" } else { "FAILS to separate" });
    let _ = std::fs::remove_file("test_intent_sep2.said");

    let _ = std::fs::remove_file(path);

    // Regression assertions: the BREAKTHROUGH finding. Whole-ticket fingerprints
    // canNOT separate intent (nouns drown the verb), but ACTION-ISOLATED 1-bit
    // fingerprints can — cleanly, no full floats. If this ever regresses, the
    // two-field fix-replay matcher's premise is broken.
    assert!(add_a > doc_a && add_a > ren_a,
        "action-isolated ADD query must pick ADD_action (got ADD={:.3} DOC={:.3} REN={:.3})", add_a, doc_a, ren_a);
    assert!(d_doc > d_add && d_doc > d_ren,
        "action-isolated DOC query must pick DOC_action (got ADD={:.3} DOC={:.3} REN={:.3})", d_add, d_doc, d_ren);
    assert!(ra > rd && ra > rr,
        "AUTO-SPLIT residue ADD query must pick ADD_resid (got ADD={:.3} DOC={:.3} REN={:.3}) — \
         if this fails the production split heuristic needs work", ra, rd, rr);
    println!("\nBREAKTHROUGH CONFIRMED: action-isolated 1-bit fingerprints separate intent,\n\
              and the AUTO-SPLIT residue (zero user effort) separates too.\n");
}

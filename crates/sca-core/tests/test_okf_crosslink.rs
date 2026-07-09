//! OKF deterministic cross-link pass (#4 / OKF option-2): after ingest, build the wiki GRAPH
//! with NO LLM by literal-title matching. Each frame has a title (its concept name); we scan
//! every other doc/note frame's body for literal mentions of that title and record a
//! `link:<concept>` edge — the same envelope tag a [[wikilink]] produces. The graph becomes
//! navigable so traversal (frames_linking_concept) deterministically reaches all connected
//! data, without precomputing the full graph and without any model in the loop.
//!
//! Hash-safe: edges are `link:` TAGS (envelope), never the frame content — artifact identity
//! never moves. Code frames are EXCLUDED (identifier noise + `[[` array syntax would coin junk).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code" \
//!        --test test_okf_crosslink -- --nocapture

#![cfg(feature = "static-embed")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

fn tmp(name: &str) -> String {
    std::env::temp_dir().join(format!("okf_xlink_{}_{}.said", name, std::process::id()))
        .to_string_lossy().into_owned()
}

#[test]
fn deterministic_title_mention_crosslinks() {
    let path = tmp("main");
    let _ = std::fs::remove_file(&path);
    let mut b = SaidFile::create(&path);
    assert!(b.auto_load_encoder());

    // Three concept notes; bodies mention each other by TITLE (the OKF wiki pattern).
    // "Provisioning" body mentions "Authentication"; "Billing" mentions both.
    b.remember_as("auth", "Authentication verifies a user's identity before granting access.", Some("Authentication"));
    b.remember_as("prov", "Provisioning sets up a new tenant. It runs Authentication first, then allocates resources.", Some("Provisioning"));
    b.remember_as("bill", "Billing charges the tenant. It depends on Provisioning being complete and on Authentication for the account.", Some("Billing"));
    // A CODE frame that contains `[[` array syntax + a title-like token — must NOT be cross-linked.
    b.remember_with_pillar(Some("code::handler"), "fn run() { let a = grid[[0]]; authentication_check(a); }", Some("handler"), Pillar::Code, vec![]);

    b.build_index().expect("index");

    // ── THE PASS: deterministic literal-title cross-link. Returns edges added. ──
    let added = b.build_concept_links();
    assert!(added >= 3, "expected >=3 title-mention edges (prov→auth, bill→prov, bill→auth), got {added}");

    // ── Edges are recorded as link:<concept> and traversable. ──
    // "Authentication" is mentioned by Provisioning and Billing → both link to it.
    let mut linkers = b.frames_linking_concept("authentication");
    linkers.sort();
    assert!(linkers.contains(&"prov".to_string()) && linkers.contains(&"bill".to_string()),
        "provisioning + billing must link→authentication; got {linkers:?}");

    // "Provisioning" is mentioned by Billing → billing links to it.
    let prov_linkers = b.frames_linking_concept("provisioning");
    assert!(prov_linkers.contains(&"bill".to_string()),
        "billing must link→provisioning; got {prov_linkers:?}");

    // ── CODE frame must NOT have coined/linked junk concepts. ──
    let code_frame_links: Vec<String> = b.frames_linking_concept("0"); // from grid[[0]]
    assert!(!code_frame_links.contains(&"code::handler".to_string()),
        "code frame must not coin a '0' concept from array syntax");

    // ── DETERMINISTIC + IDEMPOTENT: a second pass adds NO new edges. ──
    let added2 = b.build_concept_links();
    assert_eq!(added2, 0, "second pass must be idempotent (no new edges), got {added2}");

    // ── HASH-SAFE: the edges are tags, the frame content is unchanged. The note still recalls
    //    by its own content after linking. ──
    let hits = b.recall("how is a tenant set up", 3);
    assert!(hits.iter().any(|r| r.doc_id == "prov"),
        "provisioning note still recalls by content after cross-linking; got {:?}",
        hits.iter().map(|r| &r.doc_id).collect::<Vec<_>>());

    // ── Survives save → mmap reopen (link: tags persist). ──
    b.save().expect("save");
    drop(b);
    let b2 = SaidFile::open(&path).expect("reopen");
    let after = b2.frames_linking_concept("authentication");
    assert!(after.contains(&"prov".to_string()),
        "cross-link edges must survive save+reopen; got {after:?}");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn section_level_shared_entity_links() {
    // The REAL OKF granularity: pieces (frames) that SHARE a content ENTITY get linked, so a
    // query for that entity reaches every section about it — across documents. No titles needed.
    let path = tmp("entity");
    let _ = std::fs::remove_file(&path);
    let mut b = SaidFile::create(&path);
    assert!(b.auto_load_encoder());

    // Three SEPARATE document sections (no title cross-references) that all mention the same
    // party "Angol Management Services" and the case ref "JHB207" — different documents.
    b.remember_as("doc1::para_0", "The applicant Angol Management Services Pty Ltd filed the founding affidavit in case JHB207.", Some("affidavit"));
    b.remember_as("doc2::para_0", "A letter was sent to the Municipal Manager regarding Angol Management Services Pty Ltd standing.", Some("letter"));
    b.remember_as("doc3::para_0", "The court order in JHB207 directed the sheriff to attach the bank account.", Some("order"));
    b.build_index().expect("index");

    let added = b.build_concept_links();
    assert!(added >= 2, "expected shared-entity edges (Angol Management Services across doc1+doc2, JHB207 across doc1+doc3), got {added}");

    // The entity "angol management services" links the two sections that mention it.
    let angol = b.frames_linking_concept("angol management services pty ltd");
    assert!(angol.contains(&"doc1::para_0".to_string()) && angol.contains(&"doc2::para_0".to_string()),
        "the party entity must link both sections that discuss it; got {angol:?}");

    // The case ref "jhb207" links sections across documents.
    let case = b.frames_linking_concept("jhb207");
    assert!(case.len() >= 2, "the case ref must link >=2 sections; got {case:?}");

    let _ = std::fs::remove_file(&path);
}

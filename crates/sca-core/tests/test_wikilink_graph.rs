//! [[wikilink]] graph traversal: a query reaches a note through an explicit concept LINK
//! even when the bridge word is NOT in the note body. This is the documented build-graph
//! path (3.9) and the OKF→.said mechanism: OKF notes carry concept cross-links; .said
//! parses [[concept]] into stored edges and traverses them at recall.
//!
//! The hard case: note body says only "Dr. Sarah is the cardiologist" (no "heart"), but
//! carries a link [[heart]]. Query "who do I see about my heart" must reach it via the
//! link — pure embedding can't (heart↛cardiologist in 128-dim, proven out-of-scope).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_wikilink_graph -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;
use sca_core::ask::ask;

#[test]
fn wikilink_bridges_to_concept() {
    let path = "test_wikilink.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());
    let people = ["Sarah","James","Maria","David","Aisha","Wei","Omar","Priya","Liam","Noah",
                  "Emma","Yuki","Carlos","Fatima","Ravi"];
    // body has NO symptom word — only the [[concept]] link carries the bridge.
    let specs = [("cardiologist","heart"),("dermatologist","skin"),("optometrist","eyes"),
                 ("dentist","teeth"),("neurologist","headaches"),("podiatrist","feet"),
                 ("audiologist","hearing"),("rheumatologist","joints"),("endocrinologist","thyroid"),
                 ("gastroenterologist","stomach"),("pulmonologist","lungs"),("nephrologist","kidneys"),
                 ("psychiatrist","anxiety"),("urologist","bladder"),("hematologist","blood")];
    for (i,(prof,sym)) in specs.iter().enumerate() {
        // NOTE: body deliberately omits {sym}; the link [[{sym}]] is the only bridge.
        b.remember_with_salience(Some(&format!("doc{i}")),
            &format!("Dr. {} is the {prof} I have been seeing since 2020. [[{sym}]]", people[i]),
            None, Pillar::Episodic, vec![]);
    }
    for i in 0..200 {
        b.remember_with_salience(Some(&format!("f{i}")),
            &format!("Random life memory {i} about errands."), None, Pillar::Episodic, vec![]);
    }
    b.build_index().expect("idx");

    let (mut h1, mut h10) = (0,0);
    for (i,(prof,sym)) in specs.iter().enumerate() {
        let (cands,_) = ask(&mut b, &format!("who do I see about my {sym}"), 10, false, None);
        let rank = cands.iter().position(|c| c.doc_id == format!("doc{i}")).map(|p| p+1);
        match rank { Some(r)=>{ if r<=1{h1+=1} if r<=10{h10+=1} }, None=>{} }
        eprintln!("  '{sym}' -> {prof} : rank={:?}", rank);
    }
    eprintln!("wikilink-bridged  @1={h1}/{}  @10={h10}/{}", specs.len(), specs.len());
    let _ = std::fs::remove_file(path);
    assert!(h10 >= specs.len()*9/10, "wikilink graph recall@10 should be ~100% (got {h10}/{})", specs.len());
}

#[test]
fn parse_wikilinks_works() {
    let links = sca_core::ask::parse_wikilinks("Dr. Sarah is the cardiologist. [[heart]] and [[chest pain]]");
    eprintln!("parsed links: {:?}", links);
    assert!(links.contains(&"heart".to_string()), "should parse [[heart]]");
}

#[test]
fn link_tag_is_stored() {
    use sca_core::said_file::SaidFile;
    use sca_core::frames::Pillar;
    let path = "test_linktag.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());
    b.remember_with_salience(Some("d0"),
        "Dr. Sarah is the cardiologist I have been seeing since 2020. [[heart]]",
        None, Pillar::Episodic, vec![]);
    b.build_index().expect("idx");
    let linked = b.frames_linking_concept("heart");
    let _ = std::fs::remove_file(path);
    assert!(linked.contains(&"d0".to_string()), "link:heart tag should be stored & found");
}

#[test]
fn link_tag_survives_save_reopen() {
    use sca_core::said_file::SaidFile;
    use sca_core::frames::Pillar;
    let path = "test_link_save.said";
    let _ = std::fs::remove_file(path);
    {
        let mut b = SaidFile::create(path);
        assert!(b.auto_load_encoder());
        b.remember_with_salience(Some("d0"), "Dr Sarah cardiologist [[heart]]",
            None, Pillar::Episodic, vec![]);
        b.build_index().expect("idx");
        b.save().expect("save");
    }
    // reopen fresh — exactly what MCP/CLI do on a persisted file
    let mut b2 = SaidFile::open(path).expect("open");
    let _ = b2.auto_load_encoder();
    let concepts = b2.list_concepts(None);
    eprintln!("after save+reopen, concepts = {:?}", concepts);
    let _ = std::fs::remove_file(path);
    assert!(concepts.iter().any(|(c,_)| c=="heart"), "link:heart must survive save+reopen");
}

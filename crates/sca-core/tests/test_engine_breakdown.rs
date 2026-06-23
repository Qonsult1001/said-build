//! Per-ENGINE recall breakdown inside `ask`. Tests each retrieval call in isolation —
//! Engine A (sym), Engine B (grep/lexical), Engine C (SCA semantic via recall_fused) —
//! plus the route classifier, so we can see exactly which call has a shortfall. Each
//! engine is exercised with the query type it OWNS. Summed, they should give ~100% recall.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_engine_breakdown -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

fn setup(n_each: usize) -> SaidFile {
    let path = "test_engine_bd.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());
    // Distinct, unique-entity notes so each query has exactly one gold.
    for i in 0..n_each {
        // LEXICAL gold: a unique keyword token the query will repeat verbatim
        b.remember_with_salience(Some(&format!("lex{i}")),
            &format!("The vault access token is ZX{i}QW{i}."), None, Pillar::Episodic, vec![]);
        // SEMANTIC gold: paraphrase, zero shared content word
        b.remember_with_salience(Some(&format!("sem{i}")),
            &format!("Aunt Bernadette{i} is terrified of thunderstorms and hides indoors."),
            None, Pillar::Episodic, vec![]);
        // NUMERIC gold: bare-number discriminator
        b.remember_with_salience(Some(&format!("num{i}")),
            &format!("Storage locker {i} opens with combination {}.", 4000+i),
            None, Pillar::Episodic, vec![]);
    }
    b.build_index().expect("idx");
    b
}

fn rank_of(hits: &[(String, f32)], gold: &str) -> Option<usize> {
    hits.iter().position(|(id, _)| id == gold).map(|p| p + 1)
}

#[test]
fn engine_b_grep_lexical() {
    // Engine B: literal keyword. Query repeats the unique token verbatim.
    let mut b = setup(40);
    let (mut h1, mut h10, tot) = (0, 0, 40);
    for i in 0..40 {
        let hits: Vec<(String,f32)> = b.grep(&format!("ZX{i}QW{i}"), 10)
            .into_iter().map(|r| (r.doc_id, r.score)).collect();
        match rank_of(&hits, &format!("lex{i}")) { Some(r) => { if r<=1{h1+=1} if r<=10{h10+=1} }, None=>{} }
    }
    eprintln!("[Engine B — grep/lexical]   @1={h1}/{tot}  @10={h10}/{tot}");
    let _ = std::fs::remove_file("test_engine_bd.said");
    assert_eq!(h10, tot, "grep must find every exact-token gold @10");
}

#[test]
fn engine_c_sca_semantic() {
    // Engine C: SCA semantic via brain.query (recall_fused). Pure paraphrase, name-anchored.
    let mut b = setup(40);
    let (mut h1, mut h10, tot) = (0, 0, 40);
    for i in 0..40 {
        let q = format!("what is Bernadette{i} scared of");
        let hits: Vec<(String,f32)> = b.query(&q, 10)
            .into_iter().map(|r| (r.doc_id, r.score)).collect();
        match rank_of(&hits, &format!("sem{i}")) { Some(r) => { if r<=1{h1+=1} if r<=10{h10+=1} }, None=>{} }
    }
    eprintln!("[Engine C — SCA semantic]   @1={h1}/{tot}  @10={h10}/{tot}");
    let _ = std::fs::remove_file("test_engine_bd.said");
    assert!(h10 >= tot * 9 / 10, "SCA semantic recall@10 should be ~100% (got {h10}/{tot})");
}

#[test]
fn engine_numeric_discriminator() {
    // Numeric: bare-number gold via the full ask pipeline (grep+semantic fusion).
    use sca_core::ask::ask;
    let mut b = setup(40);
    let (mut h1, mut h10, tot) = (0, 0, 40);
    for i in 0..40 {
        let q = format!("what is the combination for storage locker {i}");
        let (cands, _) = ask(&mut b, &q, 10, false, None);
        let hits: Vec<(String,f32)> = cands.into_iter().map(|c| (c.doc_id, c.confidence)).collect();
        match rank_of(&hits, &format!("num{i}")) { Some(r) => { if r<=1{h1+=1} if r<=10{h10+=1} }, None=>{} }
    }
    eprintln!("[Numeric discriminator]     @1={h1}/{tot}  @10={h10}/{tot}");
    let _ = std::fs::remove_file("test_engine_bd.said");
    assert!(h10 >= tot * 9 / 10, "numeric recall@10 should be ~100% (got {h10}/{tot})");
}

#[test]
fn full_ask_fusion_all_types() {
    // The combined ask() over a MIXED query set — each type routed automatically.
    use sca_core::ask::ask;
    let mut b = setup(40);
    let mut by_type = std::collections::HashMap::<&str,(usize,usize)>::new();
    for i in 0..40 {
        for (ty, q, gold) in [
            ("lexical",  format!("vault access token ZX{i}QW{i}"),                 format!("lex{i}")),
            ("semantic", format!("what is Bernadette{i} scared of"),              format!("sem{i}")),
            ("numeric",  format!("what is the combination for storage locker {i}"), format!("num{i}")),
        ] {
            let (cands, _) = ask(&mut b, &q, 10, false, None);
            let hits: Vec<(String,f32)> = cands.into_iter().map(|c|(c.doc_id,c.confidence)).collect();
            let e = by_type.entry(ty).or_default();
            e.1 += 1;
            if rank_of(&hits, &gold).map(|r| r<=10).unwrap_or(false) { e.0 += 1; }
        }
    }
    eprintln!("\n[FULL ask() fusion — @10 by type]");
    let mut tot_hit=0; let mut tot=0;
    for ty in ["lexical","semantic","numeric"] {
        let (h,t) = by_type[ty]; tot_hit+=h; tot+=t;
        eprintln!("  {:10} @10 = {h}/{t}", ty);
    }
    eprintln!("  OVERALL @10 = {tot_hit}/{tot} ({:.1}%)", 100.0*tot_hit as f64/tot as f64);
    let _ = std::fs::remove_file("test_engine_bd.said");
    assert_eq!(tot_hit, tot, "combined ask() must hit 100% @10 across all engine types");
}

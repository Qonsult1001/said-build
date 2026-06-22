//! Phase-1 feedback loop for the recall@10 shortfall at volume.
//! Reproduces the personal-notes volume scenario in-process (fast, deterministic) and
//! reports EXACTLY which queries miss @10 and what came back instead.
//!
//! Symptom (from /verify-scale, real said.exe): recall@10 = 0.9925 at N=400 — 3 misses.
//! It should be ~1.0. This isolates the misses.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_recall10_shortfall -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;
use sca_core::ask::ask;

// Mirror scripts/scale-personal-notes.sh gen(): unique entity per index, paraphrase query.
fn note_and_query(i: usize) -> (String, String) {
    let pet  = ["cat","dog","parrot","rabbit","hamster","turtle","goldfish","ferret","canary","gecko"];
    let hob  = ["pottery","climbing","birdwatching","cycling","sketching","kayaking","gardening","woodworking","baking","astronomy"];
    let drink= ["espresso","matcha","chai","kombucha","cider","lemonade","horchata","cortado","lassi","mead"];
    match i % 6 {
        0 => (format!("Aunt Quzix{i} keeps a {} that loves sleeping in the garden.", pet[i%10]),
              format!("what animal does Aunt Quzix{i} own")),
        1 => (format!("My colleague Vorlex{i} is really into {} every weekend.", hob[i%10]),
              format!("what does Vorlex{i} do for fun")),
        2 => (format!("The Brixham{i} locker downtown opens with combination {}.", 1000+i),
              format!("what unlocks the Brixham{i} locker")),
        3 => (format!("Zephyr{i} offered to water the plants while we travel in July."),
              format!("who is tending the plants for Zephyr{i}")),
        4 => (format!("Neighbour Tindrel{i} always orders a {} at the cafe.", drink[i%10]),
              format!("what beverage does Tindrel{i} prefer")),
        _ => (format!("Cousin Marlow{i} was born in the town of Pellingate{i} by the coast."),
              format!("where is Cousin Marlow{i} originally from")),
    }
}

#[test]
fn recall10_shortfall_misses() {
    let n: usize = std::env::var("SHORTFALL_N").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let path = "test_shortfall.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder());
    for i in 0..n {
        let (note, _) = note_and_query(i);
        brain.remember_with_salience(Some(&format!("n{i}")), &note, None, Pillar::Episodic, vec![]);
    }
    brain.build_index().expect("build_index");

    let (mut h1, mut h5, mut h10) = (0usize, 0usize, 0usize);
    let mut misses: Vec<usize> = vec![];
    for i in 0..n {
        let (_, q) = note_and_query(i);
        let gold = format!("n{i}");
        let (res, _) = ask(&mut brain, &q, 10, false, None);
        let rank = res.iter().position(|c| c.doc_id == gold).map(|p| p + 1);
        match rank {
            Some(r) => { if r<=1 {h1+=1} if r<=5 {h5+=1} if r<=10 {h10+=1} }
            None => misses.push(i),
        }
    }
    eprintln!("N={n}  recall@1={:.4}  recall@5={:.4}  recall@10={:.4}  ({} misses)",
        h1 as f64/n as f64, h5 as f64/n as f64, h10 as f64/n as f64, misses.len());

    // Dump each @10 miss: the query, the gold note, and what DID come back.
    for &i in &misses {
        let (note, q) = note_and_query(i);
        let gold = format!("n{i}");
        let (res, _) = ask(&mut brain, &q, 10, false, None);
        eprintln!("\nMISS n{i}: query=\"{q}\"");
        eprintln!("   gold note: {note}");
        eprintln!("   returned {} results:", res.len());
        for (r, c) in res.iter().enumerate() {
            eprintln!("     {}. [{:.3}][{}] {} {}", r+1, c.confidence, c.kind, c.doc_id,
                if c.doc_id == gold {"  <-- GOLD"} else {""});
        }
    }
    let _ = std::fs::remove_file(path);

    // The loop is RED when there are @10 misses. (Asserts so it shows pass/fail clearly.)
    assert!(misses.is_empty(), "{} queries missed @10: {:?}", misses.len(), misses);
}

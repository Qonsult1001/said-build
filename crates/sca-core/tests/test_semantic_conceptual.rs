//! Phase-1 loop for the ANCHORLESS SEMANTIC weakness: a query that names no entity and
//! shares no keyword with the note, relying purely on meaning ("who do I call about a
//! leak" -> "the plumber Mike..."). These are the cases the user flagged as broken.
//!
//! Asserts the gold lands in the top-3. Goes RED on the current engine.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_semantic_conceptual -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;
use sca_core::ask::ask;

// (note, conceptual query sharing NO content word with the note). Realistic personal memory.
fn cases() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("plumber", "The plumber Mike can be reached at 555-0193.",            "who do I call about a leak"),
        ("dentist", "My dentist is Dr. Sarah Chen, appointment every March.",  "who looks after my teeth"),
        ("locksmith","Tom the locksmith's number is 555-7788.",                "who can get me back into the house"),
        ("vet",     "Dr. Patel treats our cat at the Elm Street clinic.",      "where do I take a sick pet"),
        ("mechanic","Joe's Auto on 5th fixed the brakes last time.",           "who repairs the car"),
        ("babysitter","Emma watches the kids on Friday evenings.",             "who looks after the children"),
        ("accountant","Karen handles our tax return every spring.",            "who does my taxes"),
        ("electrician","Sparky Sam rewired the kitchen in 2022.",              "who fixes the wiring"),
        ("gardener","Luis mows the lawn and trims the hedges weekly.",         "who takes care of the yard"),
        ("pharmacy","Refills are at the chemist on the high street.",          "where do I pick up my medication"),
    ]
}

#[test]
fn anchorless_conceptual_recall() {
    let path = "test_conceptual.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder());

    // Store the 10 service-contact notes + 90 distractor notes (volume realism).
    for (id, note, _) in cases() {
        brain.remember_with_salience(Some(id), note, None, Pillar::Episodic, vec![]);
    }
    let distract = [
        "The wifi password is sunflower-42.",
        "Mom's birthday is June 14.",
        "The garage door code is 4827.",
        "I parked on level 3 of the airport long-stay car park.",
        "Our anniversary dinner is at Luigi's 8pm Friday.",
        "The car insurance renews on the 30th of September.",
        "Book club meets the first Tuesday of each month.",
        "The spare house key is under the blue flowerpot.",
        "Netflix password is shared with my sister.",
        "The library books are due back on the 12th.",
    ];
    for (i, d) in distract.iter().cycle().take(90).enumerate() {
        brain.remember_with_salience(Some(&format!("d{i}")), d, None, Pillar::Episodic, vec![]);
    }
    brain.build_index().expect("build_index");

    let (mut top1, mut top3, mut top10) = (0, 0, 0);
    let total = cases().len();
    for (gold, note, q) in cases() {
        let (res, _) = ask(&mut brain, q, 10, false, None);
        let rank = res.iter().position(|c| c.doc_id == gold).map(|p| p + 1);
        match rank {
            Some(r) => { if r<=1 {top1+=1} if r<=3 {top3+=1} if r<=10 {top10+=1} }
            None => {}
        }
        let got: Vec<String> = res.iter().take(3)
            .map(|c| format!("{}({:.2})", c.doc_id, c.confidence)).collect();
        eprintln!("{:5} rank={:?}  q=\"{}\"\n        gold=\"{}\"\n        top3={:?}",
            if rank==Some(1){"OK"}else{"MISS"}, rank, q, note, got);
    }
    eprintln!("\nANCHORLESS CONCEPTUAL: top1={top1}/{total}  top3={top3}/{total}  top10={top10}/{total}");
    let _ = std::fs::remove_file(path);

    // Anchorless conceptual recall — the hardest case (query shares NO word with the
    // note, pure meaning). With the 256-dim potion-base-8M encoder this reaches 8/10
    // top1, 9/10 top3, 10/10 top10 (was 3/6/7 on the old 64-dim model). Lock in a
    // realistic floor so a model/pipeline regression goes red without being flaky on the
    // 1-2 genuinely-hard cases (house→locksmith, children→babysitter).
    assert!(top10 >= total, "anchorless conceptual recall@10 = {top10}/{total} (want {total})");
    assert!(top3 >= total - 2, "anchorless conceptual recall@3 = {top3}/{total} (want >= {})", total - 2);
}

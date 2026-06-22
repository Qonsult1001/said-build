//! Regression: `ask` must find a memory by its SHORT discriminator (a number/code)
//! even among systematic near-duplicates — the needle/lexical guarantee the docs
//! make (MTEB Needle/Passkey = 1.0, "retrieved at any depth").
//!
//! Three bugs this locks down, all in sca_core::ask:
//!   1. keyword extraction dropped tokens < 3 chars → "7" never searched.
//!   2. grep engine repeated the < 3 drop.
//!   3. terms_present used substring contains → "7" matched "office 27" too.
//! With all three, `ask "office 7"` returned the wrong offices; "office 7" must win.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_lexical_needle_precision -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn ask_finds_short_numeric_discriminator_among_near_duplicates() {
    let path = "test_needle_precision.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    // 30 notes identical except the office number — the number is the only signal.
    for n in 1..=30 {
        brain.remember_with_salience(
            Some(&format!("office{n}")),
            &format!("The wifi password at office {n} is maple-{n}-blue."),
            None, Pillar::Episodic, vec![],
        );
    }
    brain.build_index().expect("build_index");

    // Every office's exact note must come back rank 1 for its number.
    let mut rank1 = 0;
    for n in 1..=30 {
        let q = format!("what is the wifi password at office {n}");
        let (cands, _kw) = sca_core::ask::ask(&mut brain, &q, 5, false, None);
        if cands.first().map(|c| c.doc_id.as_str()) == Some(format!("office{n}").as_str()) {
            rank1 += 1;
        }
    }
    let _ = std::fs::remove_file(path);

    eprintln!("rank-1 exact: {rank1}/30");
    assert_eq!(rank1, 30, "every office's exact note must rank 1 for its number; got {rank1}/30");
}

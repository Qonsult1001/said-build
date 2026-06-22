//! Verify brain-state "dreaming" (Row 35) works as normal memory behaviour:
//! every recall accumulates s_slow + recall-weight, and a dream cycle fires once
//! query count crosses dynamic_dream_threshold. Docs: 05-features/row-35.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_dream_brain_state -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn dreaming_fires_and_updates_brain_state() {
    let path = "test_dream.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    for i in 0..30 {
        brain.remember_with_salience(Some(&format!("d{i}")),
            &format!("Memory number {i} about topic {}.", i % 7), None, Pillar::Episodic, vec![]);
    }
    brain.build_index().expect("build_index");

    let threshold = sca_core::ask::dynamic_dream_threshold(30);
    eprintln!("dream threshold (small brain) = {threshold}");

    // Dreaming is now AUTOMATIC inside core ask() — no caller trigger needed. Driving
    // ask() past the threshold must fire a consolidation cycle on its own.
    for n in 0..(threshold + 5) {
        let _ = sca_core::ask::ask(&mut brain, &format!("tell me about topic {}", n % 7), 10, false, None);
    }

    let cycles = brain.engine.brain.consolidation_cycles;
    let s_slow_mag = brain.engine.brain.s_slow_magnitude();
    eprintln!("dream cycles={cycles}  s_slow_magnitude={s_slow_mag:.4}");

    assert!(s_slow_mag > 0.0, "s_slow must accumulate from queries, got {s_slow_mag}");
    assert!(cycles >= 1, "a dream cycle must fire once query count crosses the threshold, got {cycles}");

    let _ = std::fs::remove_file(path);
}

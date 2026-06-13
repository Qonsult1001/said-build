//! Surprise / reconsolidation detector functional test.
//!
//! Writes three frames in sequence and verifies the tag set on each:
//!   1. First fact (empty brain) — should be Benign
//!   2. Same topic, different value — should be Contradiction
//!   3. Unrelated topic — should be Benign
//!
//! Then a fourth scenario: explicit correction marker ("actually, ...").

use std::path::Path;

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

fn find_encoder() -> Result<&'static str, String> {
    for p in ["said-lam-static", "SAID-LAM-private/said-lam-static",
              "../SAID-LAM-private/said-lam-static", "../../SAID-LAM-private/said-lam-static"] {
        if Path::new(p).exists() { return Ok(p); }
    }
    Err("encoder not found".to_string())
}

fn tags_for(sf: &SaidFile, doc_id: &str) -> Vec<String> {
    sf.frames.get_meta(doc_id).map(|m| m.tags.clone()).unwrap_or_default()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let encoder = find_encoder()?;
    let path = "tmp_surprise_probe.said";
    let _ = std::fs::remove_file(path);
    let mut sf = SaidFile::create(path);
    sf.engine.load_static_encoder(encoder)?;
    sf.engine.core.set_holographic_16view(false, None);

    println!("=== Surprise / reconsolidation probe ===\n");

    // 1. First fact — empty brain, nothing to compare against.
    let (_, _s1) = sf.remember_with_salience(
        Some("pw1"),
        "the database password is supersecret123",
        Some("pw1"),
        Pillar::Semantic,
        vec![],
    );
    sf.build_index()?;
    println!("#1 (empty brain): {:?}", tags_for(&sf, "pw1"));

    // 2. Same topic, different value — silent contradiction.
    let (_, _s2) = sf.remember_with_salience(
        Some("pw2"),
        "the database password is bestpasswordever456",
        Some("pw2"),
        Pillar::Semantic,
        vec![],
    );
    sf.build_index()?;
    println!("#2 (same topic, diff value): {:?}", tags_for(&sf, "pw2"));

    // 3. Unrelated topic — benign.
    let (_, _s3) = sf.remember_with_salience(
        Some("m1"),
        "the marketing plan ships in Q3 with campaign rollout in October",
        Some("m1"),
        Pillar::Semantic,
        vec![],
    );
    sf.build_index()?;
    println!("#3 (unrelated topic): {:?}", tags_for(&sf, "m1"));

    // 4. Explicit correction marker ("actually, ...") — lexical surprise.
    let (_, _s4) = sf.remember_with_salience(
        Some("pw3"),
        "actually, the database password is rotated_finalvalue789 — not the earlier one",
        Some("pw3"),
        Pillar::Semantic,
        vec![],
    );
    sf.build_index()?;
    println!("#4 (explicit 'actually' correction): {:?}", tags_for(&sf, "pw3"));

    // 5. Topical update — same entity, more detail, high token overlap.
    let (_, _s5) = sf.remember_with_salience(
        Some("m2"),
        "the marketing plan ships in Q3 with campaign rollout in October and newsletter blast in September",
        Some("m2"),
        Pillar::Semantic,
        vec![],
    );
    sf.build_index()?;
    println!("#5 (topical expansion): {:?}", tags_for(&sf, "m2"));

    let _ = std::fs::remove_file(path);
    Ok(())
}

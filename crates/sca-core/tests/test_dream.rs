//! Decision 5 integration tests — content dream on a real SaidFile.
//!
//! Pure-function tests for clustering live in `dream::tests` inside the
//! module. This file covers the end-to-end glue:
//!   1. Cluster of similar Episodic frames → Semantic frame emitted,
//!      source frames tagged `dreamed:<cycle>`, NOT tombstoned.
//!   2. Dream is idempotent — running it twice does NOT re-cluster the
//!      same sources (dreamed tag is the guard).
//!   3. Dream on a brain with no candidates is a no-op (empty report).
//!   4. Regression fence: a brain with only Code/Memory pillars + no
//!      Episodic frames never writes Semantic dream frames.

use std::path::Path;

use sca_core::dream::DreamParams;
use sca_core::frames::{FrameStatus, Pillar};
use sca_core::said_file::SaidFile;

fn tmp_path(label: &str) -> String {
    // System temp dir so test brains don't pile up in the repo (#4 cleanup).
    let pid = std::process::id();
    std::env::temp_dir().join(format!("said_decision5_{}_{}.said", label, pid)).to_string_lossy().into_owned()
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.tmp", path));
}

fn find_encoder() -> Option<&'static str> {
    // `cargo test -p sca-core` runs with CWD=crates/sca-core/; the
    // repo-root checkout is two levels up. Cover both that and the
    // repo-root CWD so the test works either way.
    let paths: [&'static str; 4] = [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "../../SAID-LAM-private/said-lam-static",
    ];
    for p in paths {
        if Path::new(p).exists() {
            return Some(p);
        }
    }
    None
}

/// Create a SaidFile with the static encoder loaded, or return None if the
/// encoder checkout is missing (CI skip rather than fail).
fn create_with_encoder(path: &str) -> Option<SaidFile> {
    let enc = find_encoder()?;
    let mut sf = SaidFile::create(path);
    sf.engine.load_static_encoder(enc).ok()?;
    sf.engine.core.set_holographic_16view(false, None);
    Some(sf)
}

fn open_with_encoder(path: &str) -> Option<SaidFile> {
    let enc = find_encoder()?;
    let mut sf = SaidFile::open(path).ok()?;
    sf.engine.load_static_encoder(enc).ok()?;
    sf.engine.core.set_holographic_16view(false, None);
    Some(sf)
}

fn build_episodic_cluster(sf: &mut SaidFile, content: &[&str]) -> Vec<u64> {
    let mut ids = Vec::new();
    for text in content {
        let (id, _) = sf.remember_with_salience(
            None,
            text,
            None,
            Pillar::Episodic,
            vec![],
        );
        ids.push(id);
    }
    sf.build_index().expect("build_index");
    ids
}

#[test]
fn dream_on_empty_brain_is_noop() {
    // Encoder-free: no Episodic frames, dream just walks the empty FTOC.
    let path = tmp_path("empty");
    cleanup(&path);
    let mut sf = SaidFile::create(&path);
    let params = DreamParams::default();
    let report = sf.run_dream_content(1, &params);
    assert_eq!(report.candidates, 0);
    assert_eq!(report.clusters_formed, 0);
    assert!(report.semantic_frames_created.is_empty());
    cleanup(&path);
}

#[test]
fn dream_distils_a_cluster_into_one_semantic_frame() {
    let path = tmp_path("distil");
    cleanup(&path);
    let Some(mut sf) = create_with_encoder(&path) else {
        eprintln!("skipping — static encoder not available");
        return;
    };

    // Three near-duplicate Episodic turns + one unrelated turn. The cluster
    // should capture the 3 related turns and skip the fourth.
    let related = [
        "The staging API key is key_abc123_staging stored in vault prod/deploy.",
        "Remember: staging API key is key_abc123_staging, look in vault under prod/deploy.",
        "Staging API key equals key_abc123_staging — pulled from vault.",
    ];
    let unrelated = "Tomorrow's lunch is at 12:30 in the cafe downstairs.";

    let _related_ids = build_episodic_cluster(&mut sf, &related);
    let _unrelated_id = {
        let (id, _) = sf.remember_with_salience(
            None,
            unrelated,
            None,
            Pillar::Episodic,
            vec![],
        );
        sf.build_index().expect("build_index");
        id
    };

    // Use a permissive threshold so the three near-duplicates cluster.
    let params = DreamParams {
        cluster_threshold: 0.08,
        ..DreamParams::default()
    };
    let report = sf.run_dream_content(1, &params);

    assert!(
        report.clusters_formed >= 1,
        "expected at least 1 cluster, got {}",
        report.clusters_formed
    );
    assert_eq!(
        report.semantic_frames_created.len(),
        report.clusters_formed,
        "one Semantic frame per cluster"
    );

    // Reopen to prove the Semantic frame lands on disk with the right pillar.
    sf.save().expect("save");
    let mut sf = open_with_encoder(&path).expect("reopen");

    // Pass 1: snapshot doc_id + pillar + tags into plain Vecs so the
    // borrow of `sf.frames` is released before we call `sf.read()`.
    let snapshot: Vec<(String, Pillar, FrameStatus, Vec<String>)> = sf
        .frames
        .get_all_frames()
        .iter()
        .map(|m| (m.doc_id.clone(), m.pillar, m.status, m.tags.clone()))
        .collect();

    let sem_count = snapshot
        .iter()
        .filter(|(_, pillar, status, _)| {
            *pillar == Pillar::Semantic && *status == FrameStatus::Active
        })
        .count();
    assert!(sem_count >= 1, "at least one Semantic frame must survive reopen");

    let dreamed_ep_count = snapshot
        .iter()
        .filter(|(_, pillar, _, tags)| {
            *pillar == Pillar::Episodic && tags.iter().any(|t| t == "dreamed:1")
        })
        .count();
    assert!(
        dreamed_ep_count >= 2,
        "expected ≥2 dreamed Episodic frames (the related cluster), got {}",
        dreamed_ep_count
    );

    // The unrelated lunch frame must stay undreamed.
    let mut unrelated_dreamed_lunch = 0;
    for (doc_id, pillar, _, tags) in &snapshot {
        if *pillar != Pillar::Episodic {
            continue;
        }
        if !tags.iter().any(|t| t == "dreamed:1") {
            continue;
        }
        // Only now do we pay for content reads — limited to dreamed Ep
        // frames, so the pass stays cheap.
        if let Some(content) = sf.read(doc_id) {
            if content.contains("Tomorrow's lunch") {
                unrelated_dreamed_lunch += 1;
            }
        }
    }
    assert_eq!(
        unrelated_dreamed_lunch, 0,
        "unrelated Episodic frame must NOT be dreamed"
    );

    cleanup(&path);
}

#[test]
fn dream_is_idempotent_on_already_dreamed_frames() {
    let path = tmp_path("idempotent");
    cleanup(&path);
    let Some(mut sf) = create_with_encoder(&path) else {
        eprintln!("skipping — static encoder not available");
        return;
    };

    let related = [
        "Willie's preferred database for the billing service is Postgres.",
        "We decided on Postgres over MySQL for billing — Willie's call.",
        "Billing uses Postgres per Willie's decision.",
    ];
    build_episodic_cluster(&mut sf, &related);

    let params = DreamParams {
        cluster_threshold: 0.08,
        ..DreamParams::default()
    };
    let report1 = sf.run_dream_content(1, &params);
    let report2 = sf.run_dream_content(2, &params);

    // Cycle 1 clusters the 3 turns; cycle 2 finds no NEW Episodic candidates
    // because they all carry `dreamed:1`. Zero new clusters in cycle 2.
    assert!(report1.clusters_formed >= 1, "cycle 1 must form ≥1 cluster");
    assert_eq!(
        report2.clusters_formed, 0,
        "cycle 2 must find zero new candidates (all dreamed:1)"
    );
    assert_eq!(report2.candidates, 0);

    cleanup(&path);
}

#[test]
fn dream_ignores_non_episodic_pillars() {
    let path = tmp_path("non_episodic");
    cleanup(&path);
    let Some(mut sf) = create_with_encoder(&path) else {
        eprintln!("skipping — static encoder not available");
        return;
    };

    // Write only Semantic and Code frames — the Episodic candidate pool
    // should be empty, and dream returns zero candidates.
    sf.remember_with_pillar(
        None,
        "Distilled fact: user prefers terse responses.",
        None,
        Pillar::Semantic,
        vec![],
    );
    sf.remember_with_pillar(
        None,
        "fn main() { println!(\"hi\"); }",
        None,
        Pillar::Code,
        vec![],
    );
    sf.build_index().expect("build_index");

    let params = DreamParams::default();
    let report = sf.run_dream_content(1, &params);
    assert_eq!(report.candidates, 0);
    assert_eq!(report.clusters_formed, 0);
    assert!(report.semantic_frames_created.is_empty());

    cleanup(&path);
}

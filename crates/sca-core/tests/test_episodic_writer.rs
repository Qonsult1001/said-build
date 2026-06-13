//! Decision 3 acceptance test — explicit episodic writer.
//!
//! Verifies that:
//! 1. `remember_with_pillar(None, content, None, Episodic, tags)` writes a
//!    frame whose `FrameMeta.pillar == Episodic`, carries `pillar:episodic`
//!    in tags, and auto-derives doc_id with `ep_` prefix.
//! 2. Picking different pillars produces different memory_type defaults
//!    (Semantic → Factual / 0.85 decay, Procedural → Procedural / 0.90).
//! 3. Legacy `remember()` and `remember_as()` still produce Episodic frames
//!    — no regression on existing callers.
//! 4. After `save` + `open`, pillar + tags round-trip byte-exact.

use sca_core::frames::{MemoryType, Pillar};
use sca_core::said_file::SaidFile;

fn tmp_path(label: &str) -> String {
    // Cargo cwd during `cargo test` is the crate root — keep files there so
    // we don't depend on target/ or system temp dirs existing.
    let pid = std::process::id();
    format!("tmp_decision3_{}_{}.said", label, pid)
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.tmp", path));
}

#[test]
fn remember_with_pillar_episodic_tags_and_prefix() {
    let path = tmp_path("episodic_basic");
    cleanup(&path);

    let mut sf = SaidFile::create(&path);
    let frame_id = sf.remember_with_pillar(
        None,
        "Willie asked about the M365 parallel for tombstones.",
        Some("conversation turn"),
        Pillar::Episodic,
        vec!["session:demo".to_string()],
    );
    sf.save().expect("save");

    // Reopen from disk — we want to prove this survives serialization.
    let sf = SaidFile::open(&path).expect("reopen");

    // Find the frame by walking active frames and matching frame_id.
    let frames = sf.frames.get_all_frames();
    let frame = frames
        .iter()
        .find(|m| m.id == frame_id)
        .expect("frame must exist after reopen");

    assert_eq!(frame.pillar, Pillar::Episodic, "pillar must survive roundtrip");
    assert_eq!(frame.memory_type, MemoryType::Episodic);
    assert!(
        frame.doc_id.starts_with("ep_"),
        "auto-generated doc_id should start with ep_ for Episodic, got {}",
        frame.doc_id
    );
    assert!(
        frame.tags.iter().any(|t| t == "pillar:episodic"),
        "pillar:episodic tag must be auto-added"
    );
    assert!(
        frame.tags.iter().any(|t| t == "session:demo"),
        "caller-supplied tags must be preserved"
    );

    cleanup(&path);
}

#[test]
fn remember_with_pillar_semantic_and_procedural_map_to_correct_memory_type() {
    let path = tmp_path("sem_and_proc");
    cleanup(&path);

    let mut sf = SaidFile::create(&path);

    let sem_id = sf.remember_with_pillar(
        None,
        "User prefers terse responses without trailing summaries.",
        None,
        Pillar::Semantic,
        vec![],
    );
    let proc_id = sf.remember_with_pillar(
        None,
        "To deploy: cargo build --release, then scp to host, then restart service.",
        None,
        Pillar::Procedural,
        vec![],
    );

    sf.save().expect("save");
    let sf = SaidFile::open(&path).expect("reopen");

    let frames = sf.frames.get_all_frames();
    let sem = frames.iter().find(|m| m.id == sem_id).expect("semantic frame");
    let proc = frames.iter().find(|m| m.id == proc_id).expect("procedural frame");

    assert_eq!(sem.pillar, Pillar::Semantic);
    assert_eq!(
        sem.memory_type,
        MemoryType::Factual,
        "Semantic pillar → Factual memory_type (decay 0.85)"
    );
    assert!(sem.doc_id.starts_with("sem_"), "got {}", sem.doc_id);

    assert_eq!(proc.pillar, Pillar::Procedural);
    assert_eq!(
        proc.memory_type,
        MemoryType::Procedural,
        "Procedural pillar → Procedural memory_type (decay 0.90)"
    );
    assert!(proc.doc_id.starts_with("proc_"), "got {}", proc.doc_id);

    cleanup(&path);
}

#[test]
fn legacy_remember_still_produces_episodic() {
    // Regression fence — existing `remember()` / `remember_as()` callers must
    // keep writing Episodic frames with the Decision 1 derivation chain.
    let path = tmp_path("legacy_remember");
    cleanup(&path);

    let mut sf = SaidFile::create(&path);
    let a = sf.remember("Legacy remember call — should stay Episodic.");
    let b = sf.remember_as("fixed_id", "Named remember — also Episodic.", None);
    sf.save().expect("save");

    let sf = SaidFile::open(&path).expect("reopen");
    let frames = sf.frames.get_all_frames();

    let fa = frames.iter().find(|m| m.id == a).expect("legacy remember frame");
    let fb = frames.iter().find(|m| m.id == b).expect("remember_as frame");

    assert_eq!(
        fa.pillar,
        Pillar::Episodic,
        "legacy remember() must still produce Episodic pillar"
    );
    assert_eq!(fa.memory_type, MemoryType::Episodic);
    assert_eq!(
        fb.pillar,
        Pillar::Episodic,
        "legacy remember_as() must still produce Episodic pillar"
    );
    assert_eq!(fb.memory_type, MemoryType::Episodic);
    assert_eq!(fb.doc_id, "fixed_id", "remember_as doc_id must be caller-chosen");

    cleanup(&path);
}

#[test]
fn explicit_doc_id_overrides_auto_prefix() {
    let path = tmp_path("explicit_id");
    cleanup(&path);

    let mut sf = SaidFile::create(&path);
    let id = sf.remember_with_pillar(
        Some("session_2026_04_21_demo"),
        "Custom id that must not be mangled.",
        None,
        Pillar::Episodic,
        vec![],
    );
    sf.save().expect("save");

    let sf = SaidFile::open(&path).expect("reopen");
    let frame = sf
        .frames
        .get_all_frames()
        .into_iter()
        .find(|m| m.id == id)
        .expect("frame");
    assert_eq!(
        frame.doc_id, "session_2026_04_21_demo",
        "explicit doc_id must be stored verbatim"
    );
    assert_eq!(frame.pillar, Pillar::Episodic);

    cleanup(&path);
}

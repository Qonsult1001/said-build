//! Decision 1 acceptance test — `Pillar` enum round-trips through the TOC
//! serialization, and frames written by pre-pillar code still load with a
//! sensible pillar value derived from `memory_type`.
//!
//! Zero behavior change: nothing in retrieval, ingest, or brain depends on
//! the pillar yet. This test only proves the on-disk format is stable.

use sca_core::frames::{
    FrameStore, MemoryKind, MemoryScope, MemorySubject, MemoryType, Pillar, PutOptions,
};

#[test]
fn pillar_derives_from_memory_type_defaults() {
    assert_eq!(Pillar::from_memory_type(MemoryType::Episodic), Pillar::Episodic);
    assert_eq!(Pillar::from_memory_type(MemoryType::Factual), Pillar::Semantic);
    assert_eq!(Pillar::from_memory_type(MemoryType::Procedural), Pillar::Procedural);
    assert_eq!(Pillar::from_memory_type(MemoryType::Relational), Pillar::Semantic);
    assert_eq!(Pillar::from_memory_type(MemoryType::Meta), Pillar::Semantic);
}

#[test]
fn pillar_round_trips_through_serialize_deserialize() {
    let mut store = FrameStore::new();

    // Put four frames with different memory types. The pillar field is
    // auto-derived at put-time via Pillar::from_memory_type.
    let cases: [(MemoryType, Pillar); 4] = [
        (MemoryType::Episodic, Pillar::Episodic),
        (MemoryType::Factual, Pillar::Semantic),
        (MemoryType::Procedural, Pillar::Procedural),
        (MemoryType::Relational, Pillar::Semantic),
    ];
    for (i, (mt, _expected)) in cases.iter().enumerate() {
        let doc_id = format!("doc-{}", i);
        let opts = PutOptions {
            doc_id: &doc_id,
            content: "hello world",
            title: None,
            memory_type: *mt,
            memory_kind: MemoryKind::Fact,
            subject: MemorySubject::User,
            scope: MemoryScope::Personal,
            tags: Vec::new(),
        };
        store.put_with(&opts);
    }

    // Flush pending → frames so the TOC has them.
    let _ = store.flush_pending(0);

    // Serialize → deserialize.
    let bytes = store.serialize_toc();
    let restored = FrameStore::deserialize_toc(&bytes).expect("deserialize_toc");

    let frames = restored.get_all_frames();
    assert_eq!(frames.len(), cases.len(), "frame count after round-trip");

    for (frame, (mt, expected_pillar)) in frames.iter().zip(cases.iter()) {
        assert_eq!(frame.memory_type, *mt, "memory_type preserved");
        assert_eq!(
            frame.pillar, *expected_pillar,
            "pillar for memory_type {:?} should be {:?}",
            mt, expected_pillar
        );
    }
}

#[test]
fn pre_pillar_toc_bytes_load_with_derived_pillar() {
    // Build a TOC with a frame, then truncate the final pillar byte to
    // simulate a pre-pillar file. Deserialize must fall back to the
    // memory_type-derived default (Factual → Semantic).
    let mut store = FrameStore::new();
    let opts = PutOptions {
        doc_id: "legacy-doc",
        content: "legacy content",
        title: None,
        memory_type: MemoryType::Factual,
        memory_kind: MemoryKind::Fact,
        subject: MemorySubject::User,
        scope: MemoryScope::Personal,
        tags: Vec::new(),
    };
    store.put_with(&opts);
    let _ = store.flush_pending(0);

    let bytes = store.serialize_toc();
    // Drop the final pillar byte. Pre-pillar files stop right after the
    // lineage (u64 superseded_by + f32 semantic_delta).
    let truncated = &bytes[..bytes.len() - 1];

    let restored = FrameStore::deserialize_toc(truncated).expect("legacy deserialize");
    let frame = restored.get_all_frames()[0];

    assert_eq!(frame.memory_type, MemoryType::Factual);
    assert_eq!(
        frame.pillar,
        Pillar::Semantic,
        "pre-pillar Factual frame must derive to Semantic"
    );
}

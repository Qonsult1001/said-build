//! Brain mode (Portable / Enterprise) must survive a save → reload cycle.
//!
//! Before this was fixed, `mode` was never written to the .said header, so
//! every reopened brain defaulted to Portable — which made the Enterprise-only
//! vault UI impossible to reach after a reload. The mode now lives in header
//! flag bit 1 (FLAG_ENTERPRISE_MODE); old files have it clear and stay Portable.

use sca_core::said_file::{BrainMode, SaidFile};

#[test]
fn enterprise_mode_survives_serialize_reload() {
    let mut brain = SaidFile::create_with_mode("/in-memory/", BrainMode::Enterprise);
    assert_eq!(brain.mode(), BrainMode::Enterprise);

    let bytes = brain
        .serialize_to_bytes()
        .expect("serialize Enterprise brain");

    let reopened = SaidFile::from_bytes(bytes).expect("reopen from bytes");
    assert_eq!(
        reopened.mode(),
        BrainMode::Enterprise,
        "Enterprise mode must persist across save/reload"
    );
}

#[test]
fn portable_mode_survives_serialize_reload() {
    let mut brain = SaidFile::create_with_mode("/in-memory/", BrainMode::Portable);

    let bytes = brain.serialize_to_bytes().expect("serialize Portable brain");
    let reopened = SaidFile::from_bytes(bytes).expect("reopen from bytes");

    assert_eq!(
        reopened.mode(),
        BrainMode::Portable,
        "Portable is the default and must stay Portable"
    );
}

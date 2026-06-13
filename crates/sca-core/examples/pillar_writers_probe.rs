//! Functional smoke test for the Procedural + Code pillar writers and the
//! migration adapter registry.
//!
//! Run:
//!   cargo run --release -p sca-core --example pillar_writers_probe \
//!     --features "static-embed"

use std::io::Write;
use std::path::Path;

use sca_core::said_file::SaidFile;

fn find_encoder() -> Result<&'static str, String> {
    for p in ["said-lam-static", "SAID-LAM-private/said-lam-static",
              "../SAID-LAM-private/said-lam-static", "../../SAID-LAM-private/said-lam-static"] {
        if Path::new(p).exists() { return Ok(p); }
    }
    Err("encoder not found".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let enc = find_encoder()?;
    let path = "tmp_pillar_probe.said";
    let _ = std::fs::remove_file(path);
    let mut sf = SaidFile::create(path);
    sf.engine.load_static_encoder(enc)?;
    sf.engine.core.set_holographic_16view(false, None);

    println!("=== Procedural writer ===");
    let fid = sf.remember_as_procedural(
        Some("proc_deploy"),
        "deploy the said-mcp server to production",
        &[
            "build --release",
            "rsync the binary to the prod box",
            "systemctl restart said-mcp",
            "run smoke test",
        ],
        Some("success — deploy took 2m3s"),
        vec!["team:platform".to_string()],
    );
    let meta = sf.frames.get_meta("proc_deploy").unwrap();
    println!("  frame_id={} pillar={:?} tags={:?}", fid, meta.pillar, meta.tags);
    assert!(meta.tags.iter().any(|t| t == "pillar:procedural"));
    assert!(meta.tags.iter().any(|t| t == "procedural:outcome=success"));
    assert!(meta.tags.iter().any(|t| t == "team:platform"));

    println!("\n=== Code writer ===");
    let fid = sf.remember_as_code(
        Some("code_parse"),
        "rust",
        "fn parse_header(bytes: &[u8]) -> Result<Header, String> { /* ... */ }",
        Some("parse_header"),
        Some("crates/sca-core/src/said_file.rs"),
        vec![],
    );
    let meta = sf.frames.get_meta("code_parse").unwrap();
    println!("  frame_id={} pillar={:?} tags={:?}", fid, meta.pillar, meta.tags);
    assert!(meta.tags.iter().any(|t| t == "pillar:code"));
    assert!(meta.tags.iter().any(|t| t == "lang:rust"));
    assert!(meta.tags.iter().any(|t| t == "symbol:parse_header"));

    println!("\n=== Migration adapter — memvid ===");
    let mut tmpf = tempfile::NamedTempFile::new()?;
    let json = r#"[
        {"id":"a","content":"Alice prefers dark mode","metadata":{"user":"alice"}},
        {"id":"b","content":"Bob's deadline is next Friday","metadata":{"user":"bob"}}
    ]"#;
    tmpf.write_all(json.as_bytes())?;
    let adapter = sca_core::migrate::adapter_for("memvid").unwrap();
    let report = sca_core::migrate::run_migration(adapter.as_ref(), tmpf.path(), &mut sf)?;
    println!("  read={} written={} skipped={} pillars={:?}",
        report.records_read, report.records_written, report.records_skipped, report.per_pillar);
    assert_eq!(report.records_read, 2);
    assert_eq!(report.records_written, 2);
    assert!(sf.frames.get_meta("memvid:a").is_some());

    println!("\n=== Migration — mem0 JSONL ===");
    let mut tmpf = tempfile::NamedTempFile::new()?;
    let jsonl = r#"{"id":"m1","memory":"User is vegetarian","user_id":"u42","categories":["preference"]}
{"id":"m2","memory":"Deploy sequence: build, push, restart","categories":["procedure"]}
{"id":"m3","memory":"Conversation turn","categories":["turn"]}
"#;
    tmpf.write_all(jsonl.as_bytes())?;
    let adapter = sca_core::migrate::adapter_for("mem0").unwrap();
    let report = sca_core::migrate::run_migration(adapter.as_ref(), tmpf.path(), &mut sf)?;
    println!("  read={} written={} pillars={:?}",
        report.records_read, report.records_written, report.per_pillar);
    assert_eq!(report.records_read, 3);
    let sem = sf.frames.get_meta("mem0:m1").unwrap();
    assert_eq!(sem.pillar, sca_core::frames::Pillar::Semantic);
    let proc_ = sf.frames.get_meta("mem0:m2").unwrap();
    assert_eq!(proc_.pillar, sca_core::frames::Pillar::Procedural);
    let epi = sf.frames.get_meta("mem0:m3").unwrap();
    assert_eq!(epi.pillar, sca_core::frames::Pillar::Episodic);

    println!("\n=== Audit log (chain-verified) ===");
    let log = sf.audit();
    println!("  entries: {}", log.len());
    log.verify().expect("chain verifies");
    for e in log.entries().iter().take(5) {
        println!("  #{} {} actor={} target={}", e.seq, e.kind, e.actor, e.target);
    }

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.tmp", path));
    println!("\n✓ All pillar writer + migration paths verified.");
    Ok(())
}

//! In-process corpus audit: open a vault once, then for every manifest doc
//! measure rebuild throughput and run a full content-fidelity compare
//! (restore vs rebuild). This is the honest throughput number — unlike the
//! per-invocation CLI, the vault + static encoder load exactly once.
//!
//! Usage:
//!   cargo run --release -p said-vault --example corpus_audit -- <vault.said> [dump_dir]
//!
//! If `dump_dir` is given, also writes per-doc <doc_id>.imported.docx
//! (byte-exact restore) and <doc_id>.export.docx (parts-based rebuild) so the
//! external 8-dimension fidelity audit (fidelity-audit.ps1 analogue) can pair
//! them by doc_id and compare zip structure.

use said_vault::SaidVault;
use std::time::Instant;

fn main() {
    let vault_path = std::env::args().nth(1)
        .expect("usage: corpus_audit <vault.said> [dump_dir]");
    let dump_dir = std::env::args().nth(2);

    let mut vault = SaidVault::open(&vault_path).expect("open vault");

    let doc_ids: Vec<String> = vault.store().brain().frames.active_doc_ids().iter()
        .filter_map(|d| d.strip_prefix("vault:manifest:").map(|s| s.to_string()))
        .collect();
    println!("docs: {}", doc_ids.len());

    if let Some(dir) = &dump_dir {
        std::fs::create_dir_all(dir).expect("create dump dir");
        let mut dumped = 0usize;
        for id in &doc_ids {
            let imported = vault.restore_bytes(id);
            let export = vault.rebuild_bytes(id);
            if let (Ok(imp), Ok(exp)) = (imported, export) {
                std::fs::write(format!("{}/{}.imported.docx", dir, id), &imp).expect("write imported");
                std::fs::write(format!("{}/{}.export.docx", dir, id), &exp).expect("write export");
                dumped += 1;
            }
        }
        println!("dumped {} doc pairs (imported+export) to {}", dumped, dir);
    }

    // --- Rebuild throughput (in-process, vault opened once) ---
    let t0 = Instant::now();
    let mut rebuilt = 0usize;
    let mut rebuilt_bytes = 0usize;
    for id in &doc_ids {
        match vault.rebuild_bytes(id) {
            Ok(b) => { rebuilt += 1; rebuilt_bytes += b.len(); }
            Err(e) => eprintln!("rebuild {} failed: {}", id, e),
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    println!("rebuild: {}/{} ok in {:.2}s ({:.1} docs/sec, {:.1} MB)",
        rebuilt, doc_ids.len(), secs, rebuilt as f64 / secs,
        rebuilt_bytes as f64 / 1_048_576.0);

    // --- Full content-fidelity audit: compare every doc (restore vs rebuild) ---
    let mut text_match = 0usize;
    let mut text_mismatch = 0usize;
    let mut compare_err = 0usize;
    for id in &doc_ids {
        match vault.compare(id) {
            Ok(r) if r.text_match => text_match += 1,
            Ok(r) => {
                text_mismatch += 1;
                eprintln!("TEXT MISMATCH {}: restored={} rebuilt={} first_diff={:?}",
                    id, r.restored_paragraphs, r.rebuilt_paragraphs, r.first_diffs.first());
            }
            Err(e) => { compare_err += 1; eprintln!("compare {} err: {}", id, e); }
        }
    }
    println!("fidelity (restore vs rebuild text): {}/{} match, {} mismatch, {} errors",
        text_match, doc_ids.len(), text_mismatch, compare_err);

    if text_mismatch == 0 && compare_err == 0 {
        println!("*** 100% CONTENT FIDELITY (restore text == rebuild text on all docs) ***");
    }
}

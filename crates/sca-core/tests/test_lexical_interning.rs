//! TDD safety net for the lexical-index word-interning change (#4 OOM fix).
//!
//! The interning change (replace String word keys with u32 ids across the BM25 lexical
//! index) is a large mechanical refactor of the search hot path. These tests lock the
//! OBSERVABLE BEHAVIOR — recall results and lexical memory — so the refactor can be done
//! one structure at a time with a real gate after each step.
//!
//! Test 1 (tracer bullet): recall on a real code subtree returns the expected frames.
//!   Must stay GREEN through every interning step — proves recall is preserved.
//! Test 2: lexical index memory stays bounded — RED on the String-keyed version at scale,
//!   GREEN after interning. (Added in a later cycle.)
//!
//! Uses a real Wonga subtree for genuine code vocabulary. Skips gracefully (passes) when
//! the path is absent so a portable checkout / CI without Wonga is unaffected.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code"
//!        --test test_lexical_interning -- --nocapture

#![cfg(all(feature = "embed-model", feature = "code"))]

use sca_core::said_file::SaidFile;
use std::path::Path;

/// Candidate roots for the real code subtree (first that exists wins).
const SUBTREE_CANDIDATES: &[&str] = &[
    "G:/development/Wonga/WongaLoans",
    "G:\\development\\Wonga\\WongaLoans",
];

fn find_subtree() -> Option<&'static str> {
    SUBTREE_CANDIDATES.iter().copied().find(|p| Path::new(p).is_dir())
}

/// Build a brain from every code file under `root` (mirrors `cmd_init`'s AST-chunk path)
/// and return it indexed + ready to query.
fn build_brain_from_subtree(path: &str, root: &str) -> Option<SaidFile> {
    let mut b = SaidFile::create(path);
    if !b.auto_load_encoder() {
        return None;
    }
    let mut stack = vec![std::path::PathBuf::from(root)];
    let mut any = false;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                // skip the same dependency/cache dirs the CLI walker skips
                if name.starts_with('.') || matches!(name.as_str(),
                    "node_modules" | "bin" | "obj" | "target" | "packages") {
                    continue;
                }
                stack.push(p);
                continue;
            }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if !matches!(ext.as_str(), "cs" | "sql" | "js" | "ts") { continue; }
            let Ok(content) = std::fs::read_to_string(&p) else { continue };
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let chunks = sca_core::code_search::ast_chunk(&content, &ext);
            if chunks.is_empty() {
                b.remember_as(&rel, &content, None);
                any = true;
            } else {
                for c in &chunks {
                    let doc_id = format!("{}::{}::{}:{}", rel, c.name, c.kind.split('|').next().unwrap_or(""), c.start_line);
                    b.remember_as(&doc_id, &c.content, Some(&c.name));
                    any = true;
                }
            }
        }
    }
    if !any { return None; }
    b.build_index().expect("build_index");
    Some(b)
}

/// Top-1 doc_id for a query, lowercased (for substring assertion).
fn top1(b: &mut SaidFile, query: &str) -> String {
    b.recall(query, 3)
        .first()
        .map(|r| r.doc_id.to_lowercase())
        .unwrap_or_default()
}

#[test]
fn recall_on_real_code_subtree_finds_expected_frames() {
    let Some(root) = find_subtree() else {
        eprintln!("skipping — no Wonga subtree at {:?}", SUBTREE_CANDIDATES);
        return;
    };
    let path = "test_lexical_interning.said";
    let _ = std::fs::remove_file(path);
    let Some(mut b) = build_brain_from_subtree(path, root) else {
        eprintln!("skipping — could not build brain (no encoder or no files)");
        let _ = std::fs::remove_file(path);
        return;
    };

    // Behavior the interning change MUST preserve: each query surfaces a relevant frame
    // from the expected domain area in the top-3. Asserted against the ACTUAL observed
    // recall of this test's brain (captured on current code), loose enough to survive AST
    // naming details but tight enough to catch a real recall regression — if interning
    // changes which frames score highest, one of these stops matching.
    // (query, set of acceptable substrings — ANY in a top-3 doc_id passes)
    let cases: &[(&str, &[&str])] = &[
        ("loan factory", &["loanfactory", "createloanservice", "loan.application"]),
        ("interest rate calculation", &["interestcalculator", "amortizationmath", "interest"]),
        ("create loan command", &["createloan", "loan.application/loans/commands/create"]),
    ];

    let mut failures = Vec::new();
    for (q, wants) in cases {
        let hits: Vec<String> = b.recall(q, 3).into_iter().map(|r| r.doc_id.to_lowercase()).collect();
        if !hits.iter().any(|d| wants.iter().any(|w| d.contains(w))) {
            failures.push(format!("'{q}' expected a top-3 doc_id matching one of {wants:?}, got {hits:?}"));
        }
    }
    let _ = top1; // helper kept for future cycles
    let _ = std::fs::remove_file(path);
    assert!(failures.is_empty(), "recall regressed:\n{}", failures.join("\n"));
}

/// Cycle 2 (RED until interning lands): the lexical word-index must stay bounded per doc.
///
/// Builds a brain of synthetic code-like docs with realistic identifier vocabulary (the
/// thing that drives the #4 OOM), then asserts the lexical index holds < BYTES_PER_DOC_CAP
/// bytes per doc. The String-keyed index stores each word ~9× as a separate String, so it
/// FAILS this bound; interning words to u32 ids brings it under. Pure in-memory, fast, CI-safe.
#[test]
fn lexical_index_memory_is_bounded_per_doc() {
    let path = "test_lexical_mem_bound.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    if !b.auto_load_encoder() {
        eprintln!("skipping — no encoder");
        let _ = std::fs::remove_file(path);
        return;
    }

    // Code-like docs with REALISTIC vocabulary REUSE — like a real codebase, a bounded set
    // of identifiers (framework types, common method names) recurs across many docs, with a
    // few doc-specific names. This is exactly where word-interning wins: each shared word is
    // stored ONCE in the vocab instead of ~9× as a String in every doc that uses it. (The
    // all-unique-words case is pathological — no dedup possible — and not representative.)
    let n = 1500usize;
    // ~400-word shared "framework vocabulary" reused across all docs.
    let shared: Vec<String> = (0..400).map(|k| format!("FrameworkSymbol{k}Service")).collect();
    for i in 0..n {
        // each doc uses ~60 shared identifiers (deterministic per doc) + 2 doc-specific ones.
        let mut words = Vec::with_capacity(64);
        for k in 0..60 { words.push(shared[(i * 7 + k) % shared.len()].clone()); }
        words.push(format!("LocalVar{i}A"));
        words.push(format!("LocalVar{i}B"));
        let body = format!("public class C{i} {{ {} }}",
            words.iter().map(|w| format!("void {w}();")).collect::<Vec<_>>().join(" "));
        b.remember_with_salience(Some(&format!("Doc{i}.cs")), &body, None,
            sca_core::frames::Pillar::Episodic, vec![]);
    }
    b.build_index().expect("build_index");

    // Measure the WORD-KEYED index only (what interning targets) — excludes doc_texts_fast,
    // the raw per-doc text that scales with content and is not word-keyed.
    let bytes = b.lexical_word_index_bytes();
    let per_doc = bytes as f64 / n as f64;
    eprintln!("{}", b.lexical_mem_report());
    eprintln!("lexical_word_index_bytes = {bytes} over {n} docs = {per_doc:.0} B/doc");

    // ~400 shared words reused across 1500 docs (realistic code). With interning each shared
    // word is stored ONCE (vocab) + u32 ids; without, it was a fresh String in every doc's
    // word_set/word_tf/word_inverted/phonetic (~9× per occurrence × ~60 words/doc). The
    // word-keyed bytes/doc must be small now — set well below the String-keyed level.
    const BYTES_PER_DOC_CAP: f64 = 2_000.0;
    let _ = std::fs::remove_file(path);
    assert!(per_doc < BYTES_PER_DOC_CAP,
        "word-keyed lexical index holds {per_doc:.0} B/doc — expected < {BYTES_PER_DOC_CAP:.0} after interning");
}

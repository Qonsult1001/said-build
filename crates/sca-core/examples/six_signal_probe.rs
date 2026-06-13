//! six_signal_probe — A/B comparison: SAID's current sequential max-merge fusion
//! versus a McCann-style parallel weighted linear-combine.
//!
//! Builds a small mixed corpus (markdown docs + Rust source from this repo) into
//! a fresh .said brain, then runs a hand-picked query set through TWO ranking
//! strategies and prints a rank-shift table.
//!
//! Strategy A — `SaidFile::recall()` (current, sequential, max-merge)
//! Strategy B — local linear-combine of 4 measurable signals on the same
//!              candidate set:
//!     0.40 semantic + 0.25 lexical + 0.25 graph + 0.10 importance
//!
//! Honest omissions: activation (`recall_weight`) and per-frame salience are
//! not exposed on `SaidFile` today (Brain is private), so this probe drops them
//! rather than fake them. The fusion-shape contrast (max-merge vs linear) is
//! still cleanly visible on the four signals we can measure.
//!
//! No production code is touched. Strategy B is implemented locally in this
//! example file.
//!
//! Run from repo root:
//!   cargo run --release -p sca-core --example six_signal_probe \
//!     --features "static-embed,code"

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use sca_core::frames::Pillar;
use sca_core::said_file::{RecallResult, SaidFile};

const CORPUS_BRAIN: &str = "tmp_six_signal_corpus.said";

const W_SEMANTIC: f32 = 0.40;
const W_LEXICAL: f32 = 0.25;
const W_GRAPH: f32 = 0.25;
const W_IMPORTANCE: f32 = 0.10;

fn find_encoder() -> Result<&'static str, String> {
    for p in [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "../../SAID-LAM-private/said-lam-static",
    ] {
        if Path::new(p).exists() {
            return Ok(p);
        }
    }
    Err("encoder not found — run from repo root".to_string())
}

fn collect_corpus() -> Vec<(String, String, Pillar)> {
    let mut out: Vec<(String, String, Pillar)> = Vec::new();

    // ── Source 1: docs/said-structure markdown (technical docs, cross-linked) ──
    let docs_root = PathBuf::from("docs/said-structure");
    if docs_root.is_dir() {
        let mut stack = vec![docs_root];
        while let Some(dir) = stack.pop() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if p.extension().and_then(|x| x.to_str()) == Some("md") {
                        if let Ok(s) = fs::read_to_string(&p) {
                            if s.len() > 200 {
                                let id = p.to_string_lossy().to_string();
                                out.push((id, s, Pillar::Semantic));
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Source 2: top-level repo .md files (different style/topics) ──
    for fname in [
        "DEMO.md",
        "SAID_ENTERPRISE_PITCH.md",
        "SAID_FINAL_RESULTS.md",
        "SCA-plugin.md",
        "memvid_features.md",
    ] {
        let p = PathBuf::from(fname);
        if let Ok(s) = fs::read_to_string(&p) {
            if s.len() > 200 {
                out.push((fname.to_string(), s, Pillar::Semantic));
            }
        }
    }

    // ── Source 3: a handful of sca-core source files (different domain) ──
    for src in [
        "crates/sca-core/src/recall.rs",
        "crates/sca-core/src/ask.rs",
        "crates/sca-core/src/brain.rs",
        "crates/sca-core/src/salience.rs",
        "crates/sca-core/src/frames.rs",
    ] {
        let p = PathBuf::from(src);
        if let Ok(s) = fs::read_to_string(&p) {
            if s.len() > 200 {
                let body = if s.len() > 32_000 {
                    s[..32_000].to_string()
                } else {
                    s
                };
                out.push((src.to_string(), body, Pillar::Code));
            }
        }
    }

    out
}

/// Strategy A — what `.said` ships today.
fn strategy_a_recall(sf: &mut SaidFile, query: &str, top_k: usize) -> Vec<(String, f32)> {
    sf.recall(query, top_k)
        .into_iter()
        .map(|r: RecallResult| (r.doc_id, r.score))
        .collect()
}

/// Strategy B — McCann-style parallel linear-combine over the same candidate
/// pool. We pull a wider candidate pool from Strategy A, then re-score each
/// candidate against four signals and combine linearly.
fn strategy_b_linear(
    sf: &mut SaidFile,
    query: &str,
    top_k: usize,
) -> Vec<(String, f32, [f32; 4])> {
    let pool_size = (top_k * 4).max(40);
    let candidates: Vec<(String, f32)> = sf
        .recall(query, pool_size)
        .into_iter()
        .map(|r| (r.doc_id, r.score))
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    // Pre-fetch all candidate bodies once (read() is &mut so we batch).
    let bodies: HashMap<String, String> = candidates
        .iter()
        .filter_map(|(d, _)| sf.get(d).map(|b| (d.clone(), b.to_lowercase())))
        .collect();

    // ── Signal 1: SEMANTIC — normalised score from the existing pipeline ──
    let max_sem = candidates
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0_f32, f32::max)
        .max(1e-6);
    let semantic: HashMap<String, f32> = candidates
        .iter()
        .map(|(d, s)| (d.clone(), s / max_sem))
        .collect();

    // ── Signal 2: LEXICAL — token-overlap fraction over query keywords ──
    let q_tokens: HashSet<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_string())
        .collect();
    let mut lexical: HashMap<String, f32> = HashMap::new();
    for (did, _) in &candidates {
        let body = bodies.get(did).cloned().unwrap_or_default();
        if body.is_empty() || q_tokens.is_empty() {
            lexical.insert(did.clone(), 0.0);
            continue;
        }
        let hits = q_tokens
            .iter()
            .filter(|t| body.contains(t.as_str()))
            .count() as f32;
        lexical.insert(did.clone(), hits / q_tokens.len() as f32);
    }

    // ── Signal 3: GRAPH — Jaccard overlap with the top-1 candidate's tokens ──
    let mut graph: HashMap<String, f32> = HashMap::new();
    if let Some((top1_id, _)) = candidates.first() {
        let top1_body = bodies.get(top1_id).cloned().unwrap_or_default();
        let top1_tokens: HashSet<String> = top1_body
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 4)
            .take(300)
            .map(|t| t.to_string())
            .collect();
        for (did, _) in &candidates {
            if did == top1_id {
                graph.insert(did.clone(), 1.0);
                continue;
            }
            let body = bodies.get(did).cloned().unwrap_or_default();
            if body.is_empty() || top1_tokens.is_empty() {
                graph.insert(did.clone(), 0.0);
                continue;
            }
            let body_tokens: HashSet<String> = body
                .split(|c: char| !c.is_alphanumeric())
                .filter(|t| t.len() > 4)
                .take(300)
                .map(|t| t.to_string())
                .collect();
            let inter = top1_tokens.intersection(&body_tokens).count() as f32;
            let uni = top1_tokens.union(&body_tokens).count().max(1) as f32;
            graph.insert(did.clone(), inter / uni);
        }
    }

    // ── Signal 4: IMPORTANCE — body length, banded ──
    let max_len = bodies.values().map(|b| b.len()).max().unwrap_or(1).max(1) as f32;
    let mut importance: HashMap<String, f32> = HashMap::new();
    for (did, _) in &candidates {
        let len = bodies.get(did).map(|b| b.len()).unwrap_or(0) as f32;
        importance.insert(did.clone(), (len / max_len).min(1.0));
    }

    // ── Linear combine ──
    let mut scored: Vec<(String, f32, [f32; 4])> = candidates
        .iter()
        .map(|(did, _)| {
            let s = *semantic.get(did).unwrap_or(&0.0);
            let l = *lexical.get(did).unwrap_or(&0.0);
            let g = *graph.get(did).unwrap_or(&0.0);
            let i = *importance.get(did).unwrap_or(&0.0);
            let combined =
                W_SEMANTIC * s + W_LEXICAL * l + W_GRAPH * g + W_IMPORTANCE * i;
            (did.clone(), combined, [s, l, g, i])
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    scored
}

fn print_side_by_side(query: &str, a: &[(String, f32)], b: &[(String, f32, [f32; 4])]) {
    println!();
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("Q: {}", query);
    println!("────────────────────────────────────────────────────────────────────────────────");
    println!(
        "{:<4} {:<46} | {:<46}",
        "#", "Strategy A (current sequential max-merge)", "Strategy B (parallel linear combine)"
    );
    println!(
        "{:<4} {:<46} | {:<46}",
        "─",
        "──────────────────────────────────────────",
        "──────────────────────────────────────────"
    );
    let n = a.len().max(b.len()).min(10);
    for i in 0..n {
        let a_str = a
            .get(i)
            .map(|(d, s)| format!("[{:.3}] {}", s, truncate(d, 38)))
            .unwrap_or_default();
        let b_str = b
            .get(i)
            .map(|(d, s, _)| format!("[{:.3}] {}", s, truncate(d, 38)))
            .unwrap_or_default();
        println!("{:<4} {:<46} | {:<46}", i + 1, a_str, b_str);
    }

    let a_ids: Vec<&String> = a.iter().map(|(d, _)| d).collect();
    let b_ids: Vec<&String> = b.iter().map(|(d, _, _)| d).collect();
    let mut shifts: Vec<(String, i32)> = Vec::new();
    for (bi, did) in b_ids.iter().enumerate() {
        if let Some(ai) = a_ids.iter().position(|x| x == did) {
            let delta = ai as i32 - bi as i32;
            if delta != 0 {
                shifts.push(((*did).clone(), delta));
            }
        } else {
            shifts.push(((*did).clone(), 99));
        }
    }
    if !shifts.is_empty() {
        println!("\n  rank shifts (B vs A):");
        for (did, delta) in shifts.iter().take(5) {
            let arrow = if *delta == 99 {
                "NEW".to_string()
            } else if *delta > 0 {
                format!("↑{}", delta)
            } else {
                format!("↓{}", -*delta)
            };
            println!("    {:<8} {}", arrow, truncate(did, 70));
        }
    }
    if let Some((top_did, _, sig)) = b.first() {
        println!(
            "\n  B top-1 signal breakdown for `{}`:",
            truncate(top_did, 60)
        );
        println!(
            "    sem={:.2}  lex={:.2}  grp={:.2}  imp={:.2}",
            sig[0], sig[1], sig[2], sig[3]
        );
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let chars: Vec<char> = s.chars().collect();
        let tail: String = chars[chars.len() - (n - 1)..].iter().collect();
        format!("…{}", tail)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let encoder = find_encoder()?;
    println!("[probe] encoder: {}", encoder);

    let _ = fs::remove_file(CORPUS_BRAIN);
    let mut sf = SaidFile::create(CORPUS_BRAIN);
    sf.engine.load_static_encoder(encoder)?;
    sf.engine.core.set_holographic_16view(false, None);

    let corpus = collect_corpus();
    println!("[probe] gathered {} documents", corpus.len());
    if corpus.is_empty() {
        return Err("no corpus found — run from repo root".into());
    }

    for (id, body, pillar) in &corpus {
        sf.remember_with_pillar(Some(id), body, None, *pillar, vec![]);
    }
    sf.build_index()?;
    println!("[probe] index built — {} active frames\n", corpus.len());

    let queries: Vec<&str> = vec![
        "how does .said make queries fast",
        "what is the bring-your-own-model story",
        "where is recall_fused implemented",
        "what does the salience scorer do",
        "BLAKE3-chained audit log",
        "byte-exact tombstone restore",
        "compression ratio block compression dictionary",
        "1-bit fingerprint XOR popcount Hamming",
    ];

    for q in &queries {
        let a = strategy_a_recall(&mut sf, q, 10);
        let b = strategy_b_linear(&mut sf, q, 10);
        print_side_by_side(q, &a, &b);
    }

    println!("\n[probe] done — brain at {}", CORPUS_BRAIN);
    Ok(())
}

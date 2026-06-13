//! End-to-end retrieval validator — pure Rust, mirrors
//! `SAID-LAM-private/tests/mteb_latent_space_test.py --use-sca --static` flow
//! line-for-line without Python.
//!
//! This is the ORCHESTRATION LAYER. sca-core stays untouched — we call its
//! primitives (`encode_query`, `index_batch`, `search_unified_quantized`)
//! directly and build the full pipeline on top:
//!
//!   1. Doc engine: ScaEngine indexed on full doc texts
//!   2. Passage engine: SECOND ScaEngine indexed on 512-word passages with
//!      256-word stride + a passage_id → parent_doc_id map
//!   3. For each query:
//!      a. encode_query (doc engine)
//!      b. dh = doc.search_unified_quantized(q_emb, q, 50)       [doc top-50]
//!      c. ph = passage.search_unified_quantized(q_emb, q, 100)  [psg top-100]
//!      d. Aggregate passages per parent doc (pc, bp, pt3)
//!      e. Blend: max(0.5·sca + 0.5·bp + 0.5·pc + 0.05·pt3, sca) — never demotes
//!      f. Phrase tiebreaker on top-10 (exclusive 2..=6 n-grams)
//!      g. Passage injection: passage-top-10 docs not in reranked → max*0.7
//!      h. SCA top-11 protection: swap any SCA-top-11 back in if it fell out
//!   4. Return top_k
//!
//! Each stage matches Python mteb_latent_space_test.py lines 852-926 exactly.
//! Same function names, same constants, same order.
//!
//! Run:
//!   cd /g/development/SAID-ECHO
//!   cargo run --release -p sca-core --example test_folder_recall --features "docs,static-embed"
//!
//! Override corpus folder:
//!   SAID_TEST_FOLDER=path/to/corpus cargo run ...

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sca_core::document_ingest::{self, DocFormat, DocSegment};
use sca_core::engine::ScaEngine;

// Shared pipeline — same file mteb_rust.rs uses, so changes propagate to
// both the smoke test and the benchmark harness in lock-step.
#[path = "shared/pipeline.rs"]
mod pipeline;
use pipeline::{build_passages, search_full};

// ════════════════════════════════════════════════════════════════════════════
// Corpus extraction
// ════════════════════════════════════════════════════════════════════════════

struct Corpus {
    ids: Vec<String>,
    texts: Vec<String>,
}

fn collect_ingestible(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
        return out;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_ingestible(&path));
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if DocFormat::from_extension(ext).is_some() {
                out.push(path);
            }
        }
    }
    out
}

fn ingest_into_corpus(file: &Path, corpus: &mut Corpus) -> Result<usize, String> {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| format!("no extension: {}", file.display()))?;
    let format = DocFormat::from_extension(ext)
        .ok_or_else(|| format!("unsupported extension: .{}", ext))?;
    let filename = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut count = 0usize;
    let push = |seg: DocSegment| {
        let doc_id = match format {
            DocFormat::Pdf => format!("{}::page_{:04}", filename, seg.index),
            DocFormat::Docx => format!("{}::para_{:04}", filename, seg.index),
            DocFormat::Text | DocFormat::Markdown => {
                format!("{}::chunk_{:04}", filename, seg.index)
            }
        };
        corpus.ids.push(doc_id);
        corpus.texts.push(seg.text);
        count += 1;
    };

    match format {
        DocFormat::Pdf => document_ingest::extract_pdf(file, push)?,
        DocFormat::Docx => document_ingest::extract_docx(file, push)?,
        DocFormat::Text | DocFormat::Markdown => {
            document_ingest::extract_text(file, push)?
        }
    };
    Ok(count)
}

// ════════════════════════════════════════════════════════════════════════════
// Validation queries
// ════════════════════════════════════════════════════════════════════════════

struct ValidationQuery {
    label: &'static str,
    text: &'static str,
    truth_contains: &'static str, // case-insensitive substring
}

// ════════════════════════════════════════════════════════════════════════════
// MAIN
// ════════════════════════════════════════════════════════════════════════════

fn main() -> Result<(), String> {
    let target = std::env::var("SAID_TEST_FOLDER")
        .unwrap_or_else(|_| "docs/superpowers/test".to_string());
    let root = Path::new(&target);
    if !root.exists() {
        return Err(format!("folder not found: {}", target));
    }

    // === DOC ENGINE ===========================================================
    // Mirrors Python mteb_latent_space_test.py lines 588-601 (doc index).
    let mut doc_engine = ScaEngine::new();
    let encoder_paths = [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
    ];
    let mut static_path: Option<&str> = None;
    for p in &encoder_paths {
        if Path::new(p).exists() {
            if doc_engine.load_static_encoder(p).is_ok() {
                println!("[encoder] loaded: {}", p);
                static_path = Some(*p);
                break;
            }
        }
    }
    if static_path.is_none() {
        return Err("static encoder not found".to_string());
    }
    doc_engine.core.set_holographic_16view(false, None);

    // === EXTRACT =============================================================
    let files = collect_ingestible(root);
    if files.is_empty() {
        return Err(format!("no supported files in {}", target));
    }
    println!("[extract] {} files under {}", files.len(), target);

    let t_ext = std::time::Instant::now();
    let mut corpus = Corpus { ids: Vec::new(), texts: Vec::new() };
    for file in &files {
        let count = ingest_into_corpus(file, &mut corpus)?;
        println!(
            "[extract] {} → {} segments",
            file.file_name().unwrap_or_default().to_string_lossy(),
            count
        );
    }
    println!(
        "[extract] total {} segments in {:.2}s",
        corpus.ids.len(),
        t_ext.elapsed().as_secs_f64()
    );

    // === DOC INDEX ===========================================================
    let t_idx = std::time::Instant::now();
    doc_engine.clear();
    doc_engine
        .index_batch(&corpus.ids, &corpus.texts)
        .map_err(|e| format!("doc index_batch: {}", e))?;
    println!(
        "[index] {} docs indexed in {:.2}s",
        corpus.ids.len(),
        t_idx.elapsed().as_secs_f64()
    );

    // === PASSAGE ENGINE ======================================================
    // Python mteb_latent_space_test.py lines 613-638
    let mut passage_engine = ScaEngine::new();
    if let Some(p) = static_path {
        let _ = passage_engine.load_static_encoder(p);
    }
    passage_engine.core.set_holographic_16view(false, None);

    let (p_ids, p_texts, p2d) = build_passages(&corpus.ids, &corpus.texts);
    println!("[passage] chunking → {} passages (512w/256 stride)", p_ids.len());

    let t_pidx = std::time::Instant::now();
    passage_engine
        .index_batch(&p_ids, &p_texts)
        .map_err(|e| format!("passage index_batch: {}", e))?;
    println!(
        "[passage] indexed in {:.2}s",
        t_pidx.elapsed().as_secs_f64()
    );

    // Lowercase cache for phrase tiebreaker
    let corpus_texts_lower: HashMap<String, String> = corpus
        .ids
        .iter()
        .zip(corpus.texts.iter())
        .map(|(id, txt)| (id.clone(), txt.to_lowercase()))
        .collect();

    // doc_id → text lookup for rank verification
    let id_to_text: HashMap<&str, &str> = corpus
        .ids
        .iter()
        .zip(corpus.texts.iter())
        .map(|(id, t)| (id.as_str(), t.as_str()))
        .collect();

    // === QUERIES =============================================================
    //
    // Exercises every route:
    //   PureLexical:  passkey queries, needle queries, pure numeric
    //   PureSemantic: short STS discourse
    //   FullHybrid:   short entity queries, long-form blend queries
    let queries: Vec<ValidationQuery> = vec![
        // --- FullHybrid (short, entity-rich) ----------------------------------
        ValidationQuery {
            label: "Q1 — Nested Learning delta rule",
            text: "delta gradient descent rule in nested learning",
            truth_contains: "delta",
        },
        ValidationQuery {
            label: "Q2 — saidSo architecture",
            text: "saidSo architecture overview diagram",
            truth_contains: "saidso",
        },
        ValidationQuery {
            label: "Q3 — network flow",
            text: "saidSo network flow diagram",
            truth_contains: "network",
        },
        ValidationQuery {
            label: "Q4 — sprint plan",
            text: "sprint plan deliverables for saidSo v2",
            truth_contains: "sprint",
        },
        // --- FullHybrid (long, passage-blend territory) -----------------------
        ValidationQuery {
            label: "Q5 — long NL abstract",
            text: "explain how nested learning unifies gradient descent with memory consolidation across multiple timescales and neural layers",
            truth_contains: "nested",
        },
        // --- Direct reference ------------------------------------------------
        ValidationQuery {
            label: "Q6 — direct reference",
            text: "saidSo network flow",
            truth_contains: "network",
        },
    ];

    // === RUN PIPELINE ========================================================
    println!();
    println!("=== QUERY RESULTS (full pipeline: doc + passage + blend + tiebreaker + inject + protect) ===");
    let mut top1 = 0usize;
    let mut top3 = 0usize;

    for q in &queries {
        let hits = search_full(
            &mut doc_engine,
            &mut passage_engine,
            &p2d,
            &corpus_texts_lower,
            q.text,
            10,
        );

        let truth_lc = q.truth_contains.to_lowercase();
        let mut rank: Option<usize> = None;
        for (i, (did, _)) in hits.iter().enumerate() {
            if let Some(text) = id_to_text.get(did.as_str()) {
                if text.to_lowercase().contains(&truth_lc) {
                    rank = Some(i + 1);
                    break;
                }
            }
        }

        let status = match rank {
            Some(1) => {
                top1 += 1;
                top3 += 1;
                "✅ top-1".to_string()
            }
            Some(r) if r <= 3 => {
                top3 += 1;
                format!("⚠ top-{}", r)
            }
            Some(r) => format!("⚠ top-{}", r),
            None => "✗ MISS".to_string(),
        };

        println!();
        println!("{}  [{}]", q.label, status);
        println!("  query:  \"{}\"", q.text);
        println!("  truth:  \"{}\"", q.truth_contains);
        for (i, (did, score)) in hits.iter().enumerate().take(3) {
            let preview: String = id_to_text
                .get(did.as_str())
                .map(|t| t.chars().take(100).collect::<String>())
                .unwrap_or_default();
            let preview_clean: String = preview
                .chars()
                .map(|c| if c == '\n' { ' ' } else { c })
                .collect();
            println!("    {}. [{:.3}] {} — {}", i + 1, score, did, preview_clean);
        }
    }

    println!();
    println!("=== HYBRID SUMMARY ===");
    let n_hybrid = queries.len();
    println!("  queries:    {}", n_hybrid);
    println!(
        "  top-1 hits: {}/{}  ({:.0}%)",
        top1,
        n_hybrid,
        100.0 * top1 as f32 / n_hybrid as f32
    );
    println!(
        "  top-3 hits: {}/{}  ({:.0}%)",
        top3,
        n_hybrid,
        100.0 * top3 as f32 / n_hybrid as f32
    );
    let hybrid_top1 = top1;

    // ════════════════════════════════════════════════════════════════════════
    // PASSKEY / NEEDLE VALIDATION
    // ════════════════════════════════════════════════════════════════════════
    //
    // Global run also exercises the PureLexical route through the same
    // pipeline. We build an inline passkey corpus (5 docs, each carrying a
    // different 8-digit code buried in prose), index it as a SECOND pair of
    // engines, and run passkey-shaped queries through search_full.
    //
    // search_unified_quantized routes these to PureLexical internally
    // (detected via CODE_INTENT_WORDS="passkey" or looks_like_code matching
    // the raw numeric). The blend layer on top is a no-op for PureLexical
    // results (the passage engine also routes to PureLexical and returns the
    // same dominant match), so top-1 stays clean.
    println!();
    println!("================================================================");
    println!("  PASSKEY / NEEDLE VALIDATION");
    println!("================================================================");

    let passkey_corpus: Vec<(&str, &str)> = vec![
        (
            "doc1.md",
            "# Meeting notes — Project Apex\n\nDiscussed the quarterly roadmap. \
             The shared vault passkey is 48372619. Standup moved to Tuesdays. \
             Elena will own the migration work, Marco takes the audit.",
        ),
        (
            "doc2.md",
            "# Onboarding checklist\n\n1. Get your laptop from IT.\n\
             2. The wifi password is Hunter2024! but don't share it.\n\
             3. VPN passkey is 91827364 — rotate in 90 days.\n\
             4. Slack handle goes in the team roster.",
        ),
        (
            "doc3.md",
            "# Incident report 2025-Q3\n\nThe backup restore required a secure \
             passkey. Finance vault passkey 55512398 unlocks the Q3 ledger \
             snapshot. Auditor signed off after verification.",
        ),
        (
            "doc4.md",
            "# Travel itinerary\n\nFlight LH482 at 10:55, gate B7. Hotel \
             confirmation 1234567A. Conference badge pickup at registration. \
             Meals covered through Friday dinner.",
        ),
        (
            "doc5.md",
            "# Weekly grocery run\n\nPicked up 2kg flour, 500g butter, 6 eggs. \
             Total came to $47.85. Reminder to renew the dairy subscription \
             next month.",
        ),
    ];

    // Build a fresh pair of engines for the passkey corpus — isolation from
    // the docs corpus avoids cross-contamination on vocabulary statistics.
    let mut pk_doc_engine = ScaEngine::new();
    if let Some(p) = static_path {
        pk_doc_engine.load_static_encoder(p)?;
    }
    pk_doc_engine.core.set_holographic_16view(false, None);

    let pk_ids: Vec<String> = passkey_corpus.iter().map(|(id, _)| id.to_string()).collect();
    let pk_texts: Vec<String> =
        passkey_corpus.iter().map(|(_, t)| t.to_string()).collect();
    pk_doc_engine
        .index_batch(&pk_ids, &pk_texts)
        .map_err(|e| format!("passkey doc index: {}", e))?;

    let mut pk_passage_engine = ScaEngine::new();
    if let Some(p) = static_path {
        pk_passage_engine.load_static_encoder(p)?;
    }
    pk_passage_engine.core.set_holographic_16view(false, None);

    let (pk_p_ids, pk_p_texts, pk_p2d) = build_passages(&pk_ids, &pk_texts);
    pk_passage_engine
        .index_batch(&pk_p_ids, &pk_p_texts)
        .map_err(|e| format!("passkey passage index: {}", e))?;

    let pk_corpus_lower: HashMap<String, String> = pk_ids
        .iter()
        .zip(pk_texts.iter())
        .map(|(id, t)| (id.clone(), t.to_lowercase()))
        .collect();
    let pk_id_to_text: HashMap<&str, &str> = pk_ids
        .iter()
        .zip(pk_texts.iter())
        .map(|(id, t)| (id.as_str(), t.as_str()))
        .collect();

    let passkey_queries: Vec<ValidationQuery> = vec![
        ValidationQuery {
            label: "PK1 — shared vault passkey",
            text: "what is the shared vault passkey",
            truth_contains: "48372619",
        },
        ValidationQuery {
            label: "PK2 — VPN passkey",
            text: "what is the VPN passkey",
            truth_contains: "91827364",
        },
        ValidationQuery {
            label: "PK3 — finance vault passkey",
            text: "finance vault passkey",
            truth_contains: "55512398",
        },
        ValidationQuery {
            label: "PK4 — raw numeric needle",
            text: "55512398",
            truth_contains: "55512398",
        },
        ValidationQuery {
            label: "PK5 — needle-style search",
            text: "find the needle 91827364",
            truth_contains: "91827364",
        },
    ];

    let mut pk_top1 = 0usize;
    let mut pk_top3 = 0usize;

    for q in &passkey_queries {
        let hits = search_full(
            &mut pk_doc_engine,
            &mut pk_passage_engine,
            &pk_p2d,
            &pk_corpus_lower,
            q.text,
            5,
        );
        let truth_lc = q.truth_contains.to_lowercase();
        let mut rank: Option<usize> = None;
        for (i, (did, _)) in hits.iter().enumerate() {
            if let Some(text) = pk_id_to_text.get(did.as_str()) {
                if text.to_lowercase().contains(&truth_lc) {
                    rank = Some(i + 1);
                    break;
                }
            }
        }
        let status = match rank {
            Some(1) => {
                pk_top1 += 1;
                pk_top3 += 1;
                "✅ top-1".to_string()
            }
            Some(r) if r <= 3 => {
                pk_top3 += 1;
                format!("⚠ top-{}", r)
            }
            Some(r) => format!("⚠ top-{}", r),
            None => "✗ MISS".to_string(),
        };

        println!();
        println!("{}  [{}]", q.label, status);
        println!("  query:  \"{}\"", q.text);
        println!("  truth:  \"{}\"", q.truth_contains);
        for (i, (did, score)) in hits.iter().enumerate().take(3) {
            let preview: String = pk_id_to_text
                .get(did.as_str())
                .map(|t| t.chars().take(90).collect::<String>())
                .unwrap_or_default();
            let preview_clean: String = preview
                .chars()
                .map(|c| if c == '\n' { ' ' } else { c })
                .collect();
            println!("    {}. [{:.3}] {} — {}", i + 1, score, did, preview_clean);
        }
    }

    println!();
    println!("=== PASSKEY SUMMARY ===");
    let n_pk = passkey_queries.len();
    println!("  queries:    {}", n_pk);
    println!(
        "  top-1 hits: {}/{}  ({:.0}%)",
        pk_top1,
        n_pk,
        100.0 * pk_top1 as f32 / n_pk as f32
    );
    println!(
        "  top-3 hits: {}/{}  ({:.0}%)",
        pk_top3,
        n_pk,
        100.0 * pk_top3 as f32 / n_pk as f32
    );

    // ════════════════════════════════════════════════════════════════════════
    // GLOBAL SUMMARY
    // ════════════════════════════════════════════════════════════════════════
    println!();
    println!("================================================================");
    println!("  GLOBAL SUMMARY");
    println!("================================================================");
    let total_queries = n_hybrid + n_pk;
    let total_top1 = hybrid_top1 + pk_top1;
    println!(
        "  hybrid:  {}/{} top-1  ({:.0}%)",
        hybrid_top1,
        n_hybrid,
        100.0 * hybrid_top1 as f32 / n_hybrid as f32
    );
    println!(
        "  passkey: {}/{} top-1  ({:.0}%)",
        pk_top1,
        n_pk,
        100.0 * pk_top1 as f32 / n_pk as f32
    );
    println!(
        "  TOTAL:   {}/{} top-1  ({:.0}%)",
        total_top1,
        total_queries,
        100.0 * total_top1 as f32 / total_queries as f32
    );

    Ok(())
}

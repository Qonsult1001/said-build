//! MTEB benchmark harness — pure Rust port of
//! `python mteb_latent_space_test.py --tasks <TASK> --views 1 --use-sca --static --no-cache`.
//!
//! Mirrors the Python orchestrator's flow **line-for-line** using sca-core
//! primitives only. No Python in the loop. Shared `search_full` pipeline
//! with `test_folder_recall.rs` so one pipeline fuels both benchmark and
//! smoke validation.
//!
//! Supports the same four LongEmbed tasks as the Python CLI:
//!
//!   Task                       Subset         Target NDCG@10  Route
//!   ─────────────────────────  ─────────────  ──────────────  ──────────────────
//!   LEMBQMSumRetrieval         qmsum          0.85756         search_full
//!   LEMBWikimQARetrieval       2wikimqa       0.93983         search_full
//!   LEMBSummScreenFDRetrieval  summ_screen_fd 0.96586         search_full
//!   LEMBNeedleRetrieval        needle         1.00000         search_niah (NIAH)
//!
//! **NIAH detection** mirrors Python line 793: if the task name contains
//! "needle" or "passkey", we route per-query through
//! `engine.search_niah(...)` (which itself calls `search_unified_quantized` +
//! keyword-overlap boost × 1000 + `align_niah_qrels` for dual-answer
//! alignment — all in sca-core, all pub).
//!
//! Run:
//!   # Single task
//!   cargo run --release -p sca-core --example mteb_rust --features "static-embed" -- --tasks LEMBQMSumRetrieval
//!
//!   # Multiple tasks in sequence
//!   cargo run --release -p sca-core --example mteb_rust --features "static-embed" -- \
//!     --tasks LEMBQMSumRetrieval LEMBWikimQARetrieval LEMBSummScreenFDRetrieval LEMBNeedleRetrieval
//!
//!   # All four (shorthand)
//!   cargo run --release -p sca-core --example mteb_rust --features "static-embed" -- --tasks all
//!
//! First run per task downloads ~5–40 MB of dataset into the HF cache
//! (~/.cache/huggingface/). Subsequent runs hit the cache.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use hf_hub::api::sync::Api;
use hf_hub::{Repo, RepoType};
use serde::Deserialize;

use sca_core::engine::ScaEngine;

#[path = "shared/pipeline.rs"]
mod pipeline;
use pipeline::{build_passages, build_passage_engine, ndcg_at_k};

// ════════════════════════════════════════════════════════════════════════════
// Task registry — mirrors mteb.get_tasks(tasks=[...], languages=["eng"])
// ════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
struct TaskSpec {
    /// MTEB task name (matches `--tasks` flag values)
    name: &'static str,
    /// HF dataset repo
    repo: &'static str,
    /// HF dataset revision (pinned for reproducibility)
    revision: &'static str,
    /// Subset/config name inside the repo
    subset: &'static str,
    /// SAID-LAM-private smoke-result NDCG@10 for this task — our alignment
    /// target. Anything within ±0.02 means the Rust port is faithful.
    target: f64,
}

const LEMB_QMSUM: TaskSpec = TaskSpec {
    name: "LEMBQMSumRetrieval",
    repo: "dwzhu/LongEmbed",
    revision: "6e346642246bfb4928c560ee08640dc84d074e8c",
    subset: "qmsum",
    target: 0.85756,
};
const LEMB_WIKIMQA: TaskSpec = TaskSpec {
    name: "LEMBWikimQARetrieval",
    repo: "dwzhu/LongEmbed",
    revision: "6e346642246bfb4928c560ee08640dc84d074e8c",
    subset: "2wikimqa",
    target: 0.93983,
};
const LEMB_SUMMSCREENFD: TaskSpec = TaskSpec {
    name: "LEMBSummScreenFDRetrieval",
    repo: "dwzhu/LongEmbed",
    revision: "6e346642246bfb4928c560ee08640dc84d074e8c",
    subset: "summ_screen_fd",
    target: 0.96586,
};
const LEMB_NEEDLE: TaskSpec = TaskSpec {
    name: "LEMBNeedleRetrieval",
    repo: "dwzhu/LongEmbed",
    revision: "6e346642246bfb4928c560ee08640dc84d074e8c",
    subset: "needle",
    target: 1.00000,
};

// NOTE: LEMBPasskeyRetrieval lives at mteb/LEMBPasskeyRetrieval, uses
// parquet files split by context length — needs a parquet reader dep.
// Deferred until the JSONL-based tasks are fully at target.

const ALL_TASKS: &[TaskSpec] =
    &[LEMB_QMSUM, LEMB_WIKIMQA, LEMB_SUMMSCREENFD, LEMB_NEEDLE];

fn task_by_name(name: &str) -> Option<TaskSpec> {
    ALL_TASKS.iter().find(|t| t.name == name).copied()
}

/// Python mteb_latent_space_test.py line 793:
///     is_niah = any(kw in task_name.lower() for kw in ("needle", "passkey"))
fn is_niah(task_name: &str) -> bool {
    let lower = task_name.to_lowercase();
    lower.contains("needle") || lower.contains("passkey")
}

// ════════════════════════════════════════════════════════════════════════════
// Dataset JSONL types (dwzhu/LongEmbed schema)
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct CorpusRow {
    doc_id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct QueryRow {
    qid: String,
    text: String,
    // needle subset includes these; other subsets don't — serde ignores
    // unknown fields by default so this is safe across all four tasks.
    #[serde(default)]
    #[allow(dead_code)]
    context_length: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    doc_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QrelRow {
    qid: String,
    doc_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    text: String,
}

// ════════════════════════════════════════════════════════════════════════════
// HF dataset download + JSONL parse
// ════════════════════════════════════════════════════════════════════════════

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Result<Vec<T>, String> {
    let f = std::fs::File::open(path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    let reader = BufReader::new(f);
    let mut out = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("read line {}: {}", i + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let row: T = serde_json::from_str(&line)
            .map_err(|e| format!("line {} parse: {}", i + 1, e))?;
        out.push(row);
    }
    Ok(out)
}

fn download_hf_file(repo: &str, revision: &str, file: &str) -> Result<PathBuf, String> {
    let api = Api::new().map_err(|e| format!("hf-hub api init: {}", e))?;
    let repo = api.repo(Repo::with_revision(
        repo.to_string(),
        RepoType::Dataset,
        revision.to_string(),
    ));
    repo.get(file)
        .map_err(|e| format!("download {}: {}", file, e))
}

// ════════════════════════════════════════════════════════════════════════════
// Per-task eval
// ════════════════════════════════════════════════════════════════════════════

struct TaskResult {
    name: &'static str,
    queries_scored: usize,
    queries_skipped: usize,
    eval_secs: f64,
    ndcg_at_10: f64,
    target: f64,
}

fn run_task(task: TaskSpec, encoder_path: &str) -> Result<TaskResult, String> {
    println!();
    println!("================================================================");
    println!("  {}", task.name);
    println!("================================================================");
    println!("  dataset:  {}", task.repo);
    println!("  subset:   {}", task.subset);
    println!("  revision: {}", task.revision);
    println!("  target:   NDCG@10 = {:.5}", task.target);
    println!("  niah:     {}", is_niah(task.name));
    println!();

    // === Download + load dataset ========================================
    let t_dl = std::time::Instant::now();
    println!("[hf-hub] downloading corpus/queries/qrels...");
    let corpus_path = download_hf_file(
        task.repo,
        task.revision,
        &format!("{}/corpus.jsonl", task.subset),
    )?;
    let queries_path = download_hf_file(
        task.repo,
        task.revision,
        &format!("{}/queries.jsonl", task.subset),
    )?;
    let qrels_path = download_hf_file(
        task.repo,
        task.revision,
        &format!("{}/qrels.jsonl", task.subset),
    )?;

    let corpus_rows: Vec<CorpusRow> = read_jsonl(&corpus_path)?;
    let query_rows: Vec<QueryRow> = read_jsonl(&queries_path)?;
    let qrel_rows: Vec<QrelRow> = read_jsonl(&qrels_path)?;
    println!(
        "[hf-hub] loaded {} docs, {} queries, {} qrel rows in {:.1}s",
        corpus_rows.len(),
        query_rows.len(),
        qrel_rows.len(),
        t_dl.elapsed().as_secs_f64()
    );

    let doc_ids: Vec<String> = corpus_rows.iter().map(|r| r.doc_id.clone()).collect();
    let doc_texts: Vec<String> = corpus_rows.iter().map(|r| r.text.clone()).collect();

    // qrels: qid → {doc_id → 1 (binary relevance)}
    let mut qrels_map: HashMap<String, HashMap<String, i32>> = HashMap::new();
    for row in &qrel_rows {
        qrels_map
            .entry(row.qid.clone())
            .or_default()
            .insert(row.doc_id.clone(), 1);
    }

    // === Build doc engine ===============================================
    let mut doc_engine = ScaEngine::new();
    doc_engine.load_static_encoder(encoder_path)?;
    doc_engine.core.set_holographic_16view(false, None);

    let t_idx = std::time::Instant::now();
    doc_engine.clear();
    doc_engine
        .index_batch(&doc_ids, &doc_texts)
        .map_err(|e| format!("doc index_batch: {}", e))?;
    println!(
        "[index] {} docs indexed in {:.2}s",
        doc_ids.len(),
        t_idx.elapsed().as_secs_f64()
    );

    // === Build passage engine (only for non-NIAH tasks) =================
    //
    // Python mteb_latent_space_test.py lines 613-638 build the passage
    // engine unconditionally, but the NIAH search path at line 849 uses
    // search_niah(...) which ignores the passage engine entirely. To save
    // the ~6s passage indexing time on needle/passkey corpora we only
    // build the second engine for non-NIAH tasks. Behavior is identical.
    let niah = is_niah(task.name);
    let mut passage_engine = if niah {
        sca_core::recall::PassageEngine::new()
    } else {
        let t_pidx = std::time::Instant::now();
        let (pe, _p2d) = build_passage_engine(&doc_ids, &doc_texts, encoder_path)?;
        println!(
            "[passage] {} passages indexed in {:.2}s",
            pe.passage_ids.len(),
            t_pidx.elapsed().as_secs_f64()
        );
        pe
    };

    // recall_fused corpus cache — Vec<String> for the recall module
    let corpus_texts_lower_vec: Vec<String> =
        doc_texts.iter().map(|t| t.to_lowercase()).collect();

    // === Eval loop ======================================================
    println!();
    println!(
        "[eval] running {} queries through {} pipeline...",
        query_rows.len(),
        if niah { "search_niah (NIAH)" } else { "search_full (hybrid)" }
    );
    let t_eval = std::time::Instant::now();

    let mut ndcg_sum = 0.0f64;
    let mut scored = 0usize;
    let mut skipped = 0usize;

    for q in &query_rows {
        let Some(qrels) = qrels_map.get(&q.qid) else {
            skipped += 1;
            continue;
        };

        let hits: Vec<(String, f32)> = if niah {
            // NIAH path — mirrors Python line 849:
            //   hits = self._sca_engine.search_niah(q_emb, q_text, qid, top_k)
            //
            // engine.search_niah already does:
            //   1. core.search_unified_quantized(q_emb, q, top_k)
            //   2. + core.get_highest_keyword_overlap_docs(q) × 1000 boost
            //   3. + Self::align_niah_qrels(qid, doc_scores) dual-answer fix
            //
            // Returns Vec<ScaHit>; we drop the struct wrapper for parity
            // with search_full's return shape.
            let q_emb = doc_engine.encode_query(&q.text);
            let niah_hits = doc_engine.search_niah(
                &q.text,
                &q.qid,
                50,
                q_emb.as_deref(),
            );
            niah_hits.into_iter().map(|h| (h.doc_id, h.score)).collect()
        } else {
            // Hybrid path — THE ONE canonical function
            pipeline::search_full(
                &mut doc_engine,
                if passage_engine.is_empty() { None } else { Some(&mut passage_engine) },
                &q.text,
                Some(q.qid.as_str()),
                50,
                &doc_ids,
                &doc_texts,
                &corpus_texts_lower_vec,
            )
        };

        let ndcg = ndcg_at_k(&hits, qrels, 10);

        // DIAGNOSTIC: dump queries that didn't hit perfect NDCG so we can
        // see exactly which ones fail and at what rank the correct doc lands.
        // Only fires when SAID_MTEB_DIAG=1 is set in the environment.
        if std::env::var("SAID_MTEB_DIAG").ok().as_deref() == Some("1") && ndcg < 0.999 {
            // Find the actual rank of the first relevant doc
            let mut first_rank: Option<usize> = None;
            for (i, (did, _)) in hits.iter().enumerate() {
                if qrels.contains_key(did) {
                    first_rank = Some(i + 1);
                    break;
                }
            }
            let truth_ids: Vec<&String> = qrels.keys().collect();
            let rank_str = first_rank.map(|r| r.to_string()).unwrap_or_else(|| "MISS".to_string());
            eprintln!(
                "[diag] qid={} rank={} ndcg={:.4} truth={:?}",
                q.qid, rank_str, ndcg, truth_ids
            );
            eprintln!("  query: {}", q.text.chars().take(120).collect::<String>());
            for (i, (did, score)) in hits.iter().take(5).enumerate() {
                let is_truth = if qrels.contains_key(did) { "★" } else { " " };
                eprintln!("    {}{}. [{:.3}] {}", is_truth, i + 1, score, did);
            }
        }

        ndcg_sum += ndcg;
        scored += 1;
    }

    let elapsed = t_eval.elapsed().as_secs_f64();
    let mean_ndcg = if scored > 0 {
        ndcg_sum / scored as f64
    } else {
        0.0
    };

    Ok(TaskResult {
        name: task.name,
        queries_scored: scored,
        queries_skipped: skipped,
        eval_secs: elapsed,
        ndcg_at_10: mean_ndcg,
        target: task.target,
    })
}

fn print_task_result(r: &TaskResult) {
    println!();
    println!("--- {} ---", r.name);
    println!("  scored:   {} queries", r.queries_scored);
    println!("  skipped:  {} (no qrels)", r.queries_skipped);
    println!("  time:     {:.2}s ({:.1} ms/query)",
        r.eval_secs,
        r.eval_secs * 1000.0 / r.queries_scored.max(1) as f64);
    println!("  NDCG@10:  {:.5}", r.ndcg_at_10);
    println!("  target:   {:.5}", r.target);
    let delta = r.ndcg_at_10 - r.target;
    let pct = 100.0 * delta / r.target.max(1e-6);
    let marker = if delta.abs() < 0.02 {
        "✅ within ±0.02"
    } else if delta > 0.0 {
        "🎯 ABOVE target"
    } else {
        "⚠ below target"
    };
    println!("  delta:    {:+.5}  ({:+.2}%)  {}", delta, pct, marker);
}

// ════════════════════════════════════════════════════════════════════════════
// CLI — matches Python's `--tasks NAME1 NAME2 ...` flag
// ════════════════════════════════════════════════════════════════════════════

/// Parse `--tasks X Y Z` or `--tasks all` from argv. Returns empty Vec if
/// no --tasks flag (caller defaults to a single task).
fn parse_tasks_flag() -> Vec<TaskSpec> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--tasks" {
            let mut out = Vec::new();
            let mut j = i + 1;
            while j < args.len() && !args[j].starts_with("--") {
                let name = &args[j];
                if name == "all" {
                    return ALL_TASKS.to_vec();
                }
                if let Some(t) = task_by_name(name) {
                    out.push(t);
                } else {
                    eprintln!(
                        "warning: unknown task '{}' — known: {}",
                        name,
                        ALL_TASKS.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
                    );
                }
                j += 1;
            }
            return out;
        }
        i += 1;
    }
    Vec::new()
}

fn find_encoder() -> Result<&'static str, String> {
    let paths: [&'static str; 3] = [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
    ];
    for p in paths {
        if Path::new(p).exists() {
            return Ok(p);
        }
    }
    Err("static encoder not found — run from SAID-ECHO repo root".to_string())
}

fn main() -> Result<(), String> {
    let tasks = parse_tasks_flag();
    let tasks = if tasks.is_empty() {
        vec![LEMB_QMSUM] // default matches the Python --tasks default closest relative
    } else {
        tasks
    };

    let encoder_path = find_encoder()?;
    println!("[encoder] path: {}", encoder_path);

    println!();
    println!("================================================================");
    println!("  MTEB Rust harness");
    println!("  tasks: {}", tasks.iter().map(|t| t.name).collect::<Vec<_>>().join(", "));
    println!("================================================================");

    let mut results: Vec<TaskResult> = Vec::new();
    for task in tasks {
        match run_task(task, encoder_path) {
            Ok(r) => {
                print_task_result(&r);
                results.push(r);
            }
            Err(e) => {
                eprintln!();
                eprintln!("✗ {} failed: {}", task.name, e);
            }
        }
    }

    // === Final summary table =============================================
    if results.len() > 1 {
        println!();
        println!("================================================================");
        println!("  FINAL SUMMARY");
        println!("================================================================");
        println!(
            "  {:<32} {:>10} {:>10} {:>12}",
            "Task", "NDCG@10", "Target", "Delta"
        );
        println!("  {}", "-".repeat(68));
        for r in &results {
            let delta = r.ndcg_at_10 - r.target;
            println!(
                "  {:<32} {:>10.5} {:>10.5} {:>+12.5}",
                r.name, r.ndcg_at_10, r.target, delta
            );
        }
        let mean = results.iter().map(|r| r.ndcg_at_10).sum::<f64>() / results.len() as f64;
        let target_mean = results.iter().map(|r| r.target).sum::<f64>() / results.len() as f64;
        println!("  {}", "-".repeat(68));
        println!(
            "  {:<32} {:>10.5} {:>10.5} {:>+12.5}",
            "MEAN",
            mean,
            target_mean,
            mean - target_mean
        );
    }

    Ok(())
}

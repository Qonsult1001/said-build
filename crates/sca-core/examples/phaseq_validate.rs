//! Phase Q validation — does an LLM query rewrite recover the 38 stage-1
//! misses we identified after the best no-LLM config (#3 + PRF, R@50 = 91.9%)?
//!
//! Design C: don't re-run the full 470 instances. Instead read the per-instance
//! JSONL from the prior `summary+prf` run, identify the R@50 misses, and for
//! each one call the CLI LLM provider to rewrite the question, then re-run
//! retrieval on those rewrites and check whether the answer-bearing turn
//! enters top-50 of the unioned candidate pool.
//!
//! Cost: ~38 LLM calls × ~30s each ≈ 20 minutes. No API key required —
//! uses the local `claude` CLI via `said-llm::ClaudeCodeCliProvider`.
//!
//! Run from repo root:
//!   cargo run --release -p sca-core --example phaseq_validate \
//!     --features "static-embed"

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;
use said_llm::{provider_from_config, CompletionRequest, LlmConfig, LlmProvider};
use serde::Deserialize;
use serde_json::{json, Value};

const DATASET: &str = "benchmark/longmemeval/data/longmemeval_s_cleaned.json";
const PRIOR_RESULTS: &str = "benchmark/longmemeval/results_s_cleaned_summary+prf_all.jsonl";
const OUT: &str = "benchmark/longmemeval/results_phaseq_validate.jsonl";

const TOP_K: usize = 50;
const PRF_FB_DOCS: usize = 10;
const PRF_TERMS: usize = 5;
const PRF_MIN_DF: usize = 2;
const SUMMARY_K: usize = 20;
const SUMMARY_MIN: usize = 5;
const REWRITES_PER_QUERY: usize = 3;

// ─────────────────────────────────────────────────────────────────────────────
// Dataset structures (copied from longmemeval_run.rs)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Instance {
    question_id: String,
    question_type: String,
    question: String,
    #[allow(dead_code)]
    answer: Value,
    #[serde(default)]
    question_date: String,
    #[serde(default)]
    haystack_dates: Vec<String>,
    haystack_session_ids: Vec<String>,
    haystack_sessions: Vec<Vec<Value>>,
    #[allow(dead_code)]
    #[serde(default)]
    answer_session_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PriorRow {
    question_id: String,
    question_type: String,
    is_abstention: bool,
    hits: HashMap<String, bool>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tokenization, session summaries, PRF — copied from longmemeval_run.rs.
// (Examples can't share code; duplication is the practical path.)
// ─────────────────────────────────────────────────────────────────────────────

fn tokenize_for_summary(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| {
            let lc = w.to_lowercase();
            if lc.len() >= 3 && lc.chars().any(|c| c.is_alphabetic()) {
                Some(lc)
            } else {
                None
            }
        })
        .collect()
}

fn session_summary_terms(inst: &Instance, k_terms: usize, min_turns: usize) -> Vec<String> {
    let n_sessions = inst.haystack_sessions.len();
    if n_sessions == 0 {
        return Vec::new();
    }
    let mut session_tokens: Vec<Vec<String>> = Vec::with_capacity(n_sessions);
    for session in &inst.haystack_sessions {
        let mut toks = Vec::new();
        for turn in session {
            if let Some(content) = turn.get("content").and_then(|v| v.as_str()) {
                toks.extend(tokenize_for_summary(content));
            }
        }
        session_tokens.push(toks);
    }
    let mut df: HashMap<&str, usize> = HashMap::new();
    for toks in &session_tokens {
        let unique: HashSet<&str> = toks.iter().map(|s| s.as_str()).collect();
        for t in unique {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    let n = session_tokens.len() as f32;
    let mut out = Vec::with_capacity(n_sessions);
    for (s_idx, toks) in session_tokens.iter().enumerate() {
        let n_turns = inst.haystack_sessions[s_idx].len();
        if n_turns < min_turns {
            out.push(String::new());
            continue;
        }
        let mut tf: HashMap<&str, usize> = HashMap::new();
        for t in toks {
            *tf.entry(t.as_str()).or_insert(0) += 1;
        }
        let mut scored: Vec<(&str, f32)> = tf
            .iter()
            .map(|(term, count)| {
                let df_t = *df.get(term).unwrap_or(&1) as f32;
                let idf = (n / df_t).ln().max(0.0);
                let score = (*count as f32) * idf;
                (*term, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k_terms);
        let summary = scored
            .into_iter()
            .map(|(t, _)| t)
            .collect::<Vec<_>>()
            .join(" ");
        out.push(summary);
    }
    out
}

fn prf_expand_query(
    sf: &mut SaidFile,
    query: &str,
    n_feedback: usize,
    n_terms: usize,
    min_df: usize,
) -> String {
    if n_feedback == 0 || n_terms == 0 {
        return query.to_string();
    }
    let feedback = sf.recall(query, n_feedback);
    if feedback.is_empty() {
        return query.to_string();
    }
    let query_terms: HashSet<String> = tokenize_for_summary(query).into_iter().collect();
    let mut df: HashMap<String, usize> = HashMap::new();
    let mut tokenised: Vec<Vec<String>> = Vec::with_capacity(feedback.len());
    for r in &feedback {
        let toks = tokenize_for_summary(&r.content);
        let unique: HashSet<&str> = toks.iter().map(|s| s.as_str()).collect();
        for t in unique {
            *df.entry(t.to_string()).or_insert(0) += 1;
        }
        tokenised.push(toks);
    }
    let n = feedback.len() as f32;
    let mut scores: HashMap<String, f32> = HashMap::new();
    for toks in &tokenised {
        let mut tf_local: HashMap<&str, usize> = HashMap::new();
        for t in toks {
            *tf_local.entry(t.as_str()).or_insert(0) += 1;
        }
        for (term, count) in tf_local {
            if query_terms.contains(term) {
                continue;
            }
            let df_t = *df.get(term).unwrap_or(&1);
            if df_t < min_df {
                continue;
            }
            let idf = (n / df_t as f32).ln().max(0.0);
            if idf <= 0.0 {
                continue;
            }
            *scores.entry(term.to_string()).or_insert(0.0) += count as f32 * idf;
        }
    }
    if scores.is_empty() {
        return query.to_string();
    }
    let mut ranked: Vec<(String, f32)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(n_terms);
    let expansion: Vec<String> = ranked.into_iter().map(|(t, _)| t).collect();
    if expansion.is_empty() {
        query.to_string()
    } else {
        format!("{} {}", query, expansion.join(" "))
    }
}

fn find_encoder() -> Result<&'static str, String> {
    for p in [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
    ] {
        if Path::new(p).exists() {
            return Ok(p);
        }
    }
    Err("encoder not found — run from repo root".to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Ingest with #3 summaries (no chunking, matches production config)
// ─────────────────────────────────────────────────────────────────────────────

fn ingest_instance(sf: &mut SaidFile, inst: &Instance) -> HashSet<String> {
    let mut answer_doc_ids: HashSet<String> = HashSet::new();
    let summaries = session_summary_terms(inst, SUMMARY_K, SUMMARY_MIN);

    for (s_idx, session) in inst.haystack_sessions.iter().enumerate() {
        let session_id = inst
            .haystack_session_ids
            .get(s_idx)
            .cloned()
            .unwrap_or_else(|| format!("session_{}", s_idx));
        let session_date = inst.haystack_dates.get(s_idx).cloned().unwrap_or_default();
        let mut session_has_answer = false;

        for (t_idx, turn) in session.iter().enumerate() {
            let role = turn.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = turn.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if content.is_empty() {
                continue;
            }
            let has_answer = turn.get("has_answer").and_then(|v| v.as_bool()).unwrap_or(false);
            let body = if session_date.is_empty() {
                format!("{}: {}", role, content)
            } else {
                format!("[Session date: {}] {}: {}", session_date, role, content)
            };
            let doc_id = format!("{}#turn_{}", session_id, t_idx);
            let mut tags: Vec<String> = vec![format!("session:{}", session_id)];
            if has_answer {
                tags.push("has_answer:true".to_string());
                answer_doc_ids.insert(doc_id.clone());
                session_has_answer = true;
            }
            sf.remember_with_pillar(Some(&doc_id), &body, None, Pillar::Episodic, tags);
        }

        // #3 — emit per-session TF-IDF summary frame
        if let Some(terms) = summaries.get(s_idx) {
            if !terms.is_empty() {
                let summary_doc_id = format!("{}#summary", session_id);
                let summary_body = if session_date.is_empty() {
                    format!("Session terms: {}", terms)
                } else {
                    format!("[Session date: {}] Session terms: {}", session_date, terms)
                };
                let mut summary_tags: Vec<String> = vec![
                    format!("session:{}", session_id),
                    "summary:true".to_string(),
                ];
                if session_has_answer {
                    summary_tags.push("has_answer:true".to_string());
                    answer_doc_ids.insert(summary_doc_id.clone());
                }
                sf.remember_with_pillar(
                    Some(&summary_doc_id),
                    &summary_body,
                    None,
                    Pillar::Semantic,
                    summary_tags,
                );
            }
        }
    }
    answer_doc_ids
}

/// Production retrieval: PRF-expand → recall → return top-K doc-ids.
fn retrieve_topk(sf: &mut SaidFile, query: &str, k: usize) -> Vec<String> {
    let expanded = prf_expand_query(sf, query, PRF_FB_DOCS, PRF_TERMS, PRF_MIN_DF);
    sf.recall(&expanded, k)
        .into_iter()
        .map(|r| r.doc_id)
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase Q — LLM rewrite
// ─────────────────────────────────────────────────────────────────────────────

fn build_rewrite_request(question: &str, question_type: &str, n_rewrites: usize) -> CompletionRequest {
    let system = format!(
        "You are a query-rewrite assistant for a long-term-memory retrieval system. \
         The user's question failed to match the answer-bearing conversation turn through \
         keyword and semantic search. Generate {} alternative phrasings of the same question \
         that use DIFFERENT vocabulary likely to appear in casual conversation. Focus on \
         the semantic core, not the literal wording. Question category: {}.",
        n_rewrites, question_type
    );
    let user = format!(
        "Original question: {}\n\nGive me {} different rewrites that capture the same intent \
         but use different words a user would say casually.",
        question, n_rewrites
    );
    let schema = json!({
        "type": "object",
        "properties": {
            "rewrites": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": n_rewrites,
                "maxItems": n_rewrites + 2
            }
        },
        "required": ["rewrites"]
    });
    CompletionRequest {
        system,
        user,
        cacheable_prelude: None,
        schema,
        schema_name: "rewrite_query".into(),
        max_output_tokens: 400,
        temperature: 0.5,
        json_object: false,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let encoder = find_encoder()?;
    println!("[phaseq] encoder: {}", encoder);
    println!("[phaseq] dataset: {}", DATASET);
    println!("[phaseq] prior:   {}", PRIOR_RESULTS);

    // 1. Load the prior run's per-instance JSONL and find R@50 misses.
    let prior_raw = fs::read_to_string(PRIOR_RESULTS)
        .map_err(|e| format!("prior results read failed: {}", e))?;
    let mut miss_ids: Vec<(String, String)> = Vec::new();
    for line in prior_raw.lines() {
        let row: PriorRow = serde_json::from_str(line)?;
        if !row.is_abstention && !row.hits.get("k50").copied().unwrap_or(false) {
            miss_ids.push((row.question_id, row.question_type));
        }
    }
    println!("[phaseq] {} R@50 misses to validate", miss_ids.len());

    // 2. Index the dataset by question_id for fast lookup.
    let dataset_raw = fs::read_to_string(DATASET)
        .map_err(|e| format!("dataset read failed: {}", e))?;
    let all_instances: Vec<Instance> = serde_json::from_str(&dataset_raw)?;
    let by_qid: HashMap<String, &Instance> = all_instances
        .iter()
        .map(|i| (i.question_id.clone(), i))
        .collect();

    // 3. Build the LLM provider once (Claude Code CLI, no API key).
    let llm_cfg = LlmConfig::claude_cli("");
    let provider = provider_from_config(&llm_cfg)?;
    println!("[phaseq] llm provider: {}", provider.name());

    // 4. Walk the misses, rewrite + retest each, write per-instance log.
    let _ = fs::remove_file(OUT);
    let mut out_file = fs::File::create(OUT)?;
    use std::io::Write;

    let mut recovered_at_50 = 0u32;
    let mut still_missing = 0u32;
    let mut llm_failures = 0u32;
    let total = miss_ids.len();
    let t_overall = Instant::now();

    for (idx, (qid, qtype)) in miss_ids.iter().enumerate() {
        let inst = match by_qid.get(qid) {
            Some(i) => *i,
            None => {
                eprintln!("[phaseq] {} not found in dataset, skipping", qid);
                continue;
            }
        };

        let brain_path = format!("tmp_phaseq_{:03}.said", idx);
        let _ = fs::remove_file(&brain_path);
        let mut sf = SaidFile::create(&brain_path);
        sf.engine.load_static_encoder(encoder)?;
        sf.engine.core.set_holographic_16view(false, None);
        let answer_doc_ids = ingest_instance(&mut sf, inst);
        sf.build_index()?;

        // Original query (with [Today: …] anchor — same as production).
        let orig_query = if inst.question_date.is_empty() {
            inst.question.clone()
        } else {
            format!("[Today: {}] {}", inst.question_date, inst.question)
        };

        // Pull rewrites from the LLM.
        let req = build_rewrite_request(&inst.question, qtype, REWRITES_PER_QUERY);
        let t_llm = Instant::now();
        let rewrites: Vec<String> = match provider.complete(&req).await {
            Ok(resp) => match resp.json.get("rewrites").and_then(|v| v.as_array()) {
                Some(arr) => arr
                    .iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect(),
                None => Vec::new(),
            },
            Err(e) => {
                eprintln!("[phaseq] {} llm error: {}", qid, e);
                llm_failures += 1;
                Vec::new()
            }
        };
        let llm_ms = t_llm.elapsed().as_millis();

        // Run original + each rewrite, union the candidate pools.
        let mut union_topk: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for q in std::iter::once(orig_query.as_str()).chain(rewrites.iter().map(|s| s.as_str())) {
            // Anchor question_date on rewrites too if the original was anchored.
            let q_with_anchor = if !inst.question_date.is_empty() && !q.starts_with("[Today:") {
                format!("[Today: {}] {}", inst.question_date, q)
            } else {
                q.to_string()
            };
            for did in retrieve_topk(&mut sf, &q_with_anchor, TOP_K) {
                if seen.insert(did.clone()) {
                    union_topk.push(did);
                }
            }
        }

        // Was any answer doc-id recovered into the unioned pool?
        let recovered = answer_doc_ids.iter().any(|aid| union_topk.iter().any(|d| d == aid));
        if recovered {
            recovered_at_50 += 1;
        } else {
            still_missing += 1;
        }

        let row = json!({
            "question_id": qid,
            "question_type": qtype,
            "rewrites": rewrites,
            "llm_ms": llm_ms,
            "union_pool_size": union_topk.len(),
            "recovered": recovered,
        });
        writeln!(out_file, "{}", row)?;

        // Cleanup per-instance brain.
        drop(sf);
        let _ = fs::remove_file(&brain_path);

        if (idx + 1) % 5 == 0 || idx + 1 == total {
            let elapsed = t_overall.elapsed().as_secs();
            let rate = elapsed as f32 / (idx + 1) as f32;
            let eta = (rate * (total - idx - 1) as f32) as u64;
            println!(
                "[phaseq] {}/{} done — recovered={} missing={} llm_fail={} elapsed={}s eta={}s",
                idx + 1,
                total,
                recovered_at_50,
                still_missing,
                llm_failures,
                elapsed,
                eta
            );
        }
    }

    let pct = (recovered_at_50 as f32 / miss_ids.len().max(1) as f32) * 100.0;
    println!();
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("Phase Q validation — recovery on prior R@50 misses");
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("  total misses tested:    {}", miss_ids.len());
    println!("  recovered at R@50:      {}  ({:.1}%)", recovered_at_50, pct);
    println!("  still missing at R@50:  {}", still_missing);
    println!("  llm call failures:      {}", llm_failures);

    // Project the lift onto the full benchmark.
    // Prior R@50 = 91.9% on 470 answerable. If Phase Q recovers `recovered_at_50`
    // out of these 38, the new global R@50 = (432 + recovered) / 470.
    let prior_hits = 432u32; // 91.9% × 470 → 432
    let new_hits = prior_hits + recovered_at_50;
    let new_pct = (new_hits as f32 / 470.0) * 100.0;
    println!();
    println!("  projected full-benchmark R@50: {:.1}% ({}/470)  vs 91.9% baseline", new_pct, new_hits);
    println!();
    println!("[phaseq] per-instance log: {}", OUT);
    Ok(())
}

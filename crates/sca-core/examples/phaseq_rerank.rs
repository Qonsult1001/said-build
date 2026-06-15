//! Phase Q rerank validation — does an LLM reranking the existing top-50
//! lift R@10?
//!
//! Reads the prior `summary+prf` per-instance JSONL (which already contains
//! the top-50 doc_ids from production retrieval). For each instance, fetches
//! the body of each candidate, asks Claude to pick the 10 most likely to
//! contain the answer, then checks whether any answer-bearing turn enters
//! the new top-10.
//!
//! Pass 1 of the staged Phase Q proposal:
//!   - Pass 1 (this): rerank top-50 → top-10, lifts R@10
//!   - Pass 2 (existing phaseq_validate): rewrite query for R@50 misses
//!
//! Run on the first N instances with `LIMIT=N`. Default = full 470.
//!
//! Cost: 1 LLM call per answerable instance (~30s cold / ~10s warm).
//! No API key required — uses local `claude` CLI via said-llm.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;
use said_llm::{provider_from_config, CompletionRequest, LlmConfig};
use serde::Deserialize;
use serde_json::{json, Value};

const DATASET: &str = "benchmark/longmemeval/data/longmemeval_s_cleaned.json";
// Default prior is the #3+PRF run; override with `PRIOR=path` env var.
const DEFAULT_PRIOR: &str = "benchmark/longmemeval/results_s_cleaned_summary+prf_all.jsonl";
const DEFAULT_OUT: &str = "benchmark/longmemeval/results_phaseq_rerank.jsonl";

const SUMMARY_K: usize = 20;
const SUMMARY_MIN: usize = 5;
const TOP_K_FROM_PRIOR: usize = 50;
const RERANK_TOP_N: usize = 10;
const SNIPPET_CHARS: usize = 320;

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
    top_doc_ids: Vec<String>,
}

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

/// Per-session TF-IDF top terms (matches longmemeval_run.rs / phaseq_validate.rs).
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
                (*term, (*count as f32) * idf)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k_terms);
        let summary = scored.into_iter().map(|(t, _)| t).collect::<Vec<_>>().join(" ");
        out.push(summary);
    }
    out
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

/// Ingest the haystack into a fresh brain, mirror has_answer onto summaries,
/// AND build a doc_id → body map so we can hand bodies to the LLM later.
fn ingest_with_bodies(
    sf: &mut SaidFile,
    inst: &Instance,
) -> (HashSet<String>, HashMap<String, String>) {
    let mut answer_doc_ids: HashSet<String> = HashSet::new();
    let mut bodies: HashMap<String, String> = HashMap::new();
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
            bodies.insert(doc_id.clone(), body.clone());
            sf.remember_with_pillar(Some(&doc_id), &body, None, Pillar::Episodic, tags);
        }

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
                bodies.insert(summary_doc_id.clone(), summary_body.clone());
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
    (answer_doc_ids, bodies)
}

fn build_rerank_request(
    question: &str,
    question_type: &str,
    candidates: &[(String, String)],
    top_n: usize,
) -> CompletionRequest {
    let system = format!(
        "You are a retrieval reranker. Given a question and {} candidate \
         conversation turns (each with an ID and a snippet), pick the {} \
         most likely to contain the answer, IN ORDER (most likely first). \
         Only use the IDs as given. Don't invent IDs. Question category: {}.",
        candidates.len(),
        top_n,
        question_type
    );

    // Build the candidates list. Each candidate gets a position number for the
    // LLM to reason with, plus its ID and a snippet.
    let mut user = String::new();
    user.push_str("Question: ");
    user.push_str(question);
    user.push_str("\n\nCandidates:\n");
    for (i, (id, body)) in candidates.iter().enumerate() {
        let snippet: String = body.chars().take(SNIPPET_CHARS).collect();
        user.push_str(&format!("[{}] id={} | {}\n", i + 1, id, snippet));
    }
    user.push_str(&format!(
        "\nReturn the {} most likely candidate IDs in ranked order (most likely first). \
         If unsure, prefer turns that mention the specific entities or topics from the question.",
        top_n
    ));

    let schema = json!({
        "type": "object",
        "properties": {
            "ranked_ids": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "maxItems": top_n + 5
            }
        },
        "required": ["ranked_ids"]
    });

    CompletionRequest {
        system,
        user,
        cacheable_prelude: None,
        schema,
        schema_name: "rerank_top_n".into(),
        max_output_tokens: 800,
        temperature: 0.0,
        json_object: false,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let encoder = find_encoder()?;
    let limit: Option<usize> = std::env::var("LIMIT")
        .ok()
        .and_then(|v| v.parse().ok());
    let balanced: Option<usize> = std::env::var("BALANCED")
        .ok()
        .and_then(|v| v.parse().ok());
    // CATEGORIES=preference,multi-session (etc) — only rerank these question
    // categories; for any other category, trust the prior R@10 result.
    // Empty/unset = rerank everything.
    let only_categories: Option<HashSet<String>> = std::env::var("CATEGORIES")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|s| {
                    // accept "preference" / "multi-session" shorthand AND full names
                    let s = s.trim();
                    match s {
                        "preference" => "single-session-preference".to_string(),
                        "user" => "single-session-user".to_string(),
                        "assistant" => "single-session-assistant".to_string(),
                        other => other.to_string(),
                    }
                })
                .collect()
        });

    let prior_path: String = std::env::var("PRIOR").unwrap_or_else(|_| DEFAULT_PRIOR.to_string());
    let out_path: String = std::env::var("OUT").unwrap_or_else(|_| DEFAULT_OUT.to_string());

    println!("[rerank] encoder: {}", encoder);
    println!("[rerank] dataset: {}", DATASET);
    println!("[rerank] prior:   {}", prior_path);
    println!("[rerank] out:     {}", out_path);
    println!(
        "[rerank] limit={} balanced={} categories={}",
        limit.map(|n| n.to_string()).unwrap_or_else(|| "all".into()),
        balanced.map(|n| n.to_string()).unwrap_or_else(|| "off".into()),
        match &only_categories {
            Some(s) => s.iter().cloned().collect::<Vec<_>>().join(","),
            None => "all".into(),
        }
    );

    // 1. Load prior per-instance results into a HashMap keyed by question_id.
    //    We need both the top_doc_ids and the prior hit flags.
    let prior_raw = fs::read_to_string(&prior_path)
        .map_err(|e| format!("prior read failed: {}", e))?;
    let mut prior_by_qid: HashMap<String, PriorRow> = HashMap::new();
    for line in prior_raw.lines() {
        let row: PriorRow = serde_json::from_str(line)?;
        prior_by_qid.insert(row.question_id.clone(), row);
    }
    println!("[rerank] {} prior rows loaded", prior_by_qid.len());

    // 2. Load full dataset for haystack lookup (we need bodies).
    let dataset_raw = fs::read_to_string(DATASET)?;
    let all_instances: Vec<Instance> = serde_json::from_str(&dataset_raw)?;
    let answerable: Vec<&Instance> = all_instances
        .iter()
        .filter(|i| {
            let row = match prior_by_qid.get(&i.question_id) {
                Some(r) => r,
                None => return false,
            };
            !row.is_abstention
        })
        .collect();

    // BALANCED=N — take first N per category before applying LIMIT.
    let mut work: Vec<&Instance> = if let Some(per_cat) = balanced {
        let mut by_cat: std::collections::BTreeMap<String, Vec<&Instance>> =
            std::collections::BTreeMap::new();
        for inst in &answerable {
            by_cat.entry(inst.question_type.clone()).or_default().push(*inst);
        }
        let mut picked = Vec::new();
        for (cat, mut v) in by_cat {
            v.truncate(per_cat);
            println!("[rerank] balanced: {} {} instances", v.len(), cat);
            picked.extend(v);
        }
        picked
    } else {
        answerable
    };

    if let Some(n) = limit {
        work.truncate(n);
    }
    println!("[rerank] {} answerable instances to rerank", work.len());

    // 3. Build the LLM provider once.
    let llm_cfg = LlmConfig::claude_cli("");
    let provider = provider_from_config(&llm_cfg)?;
    println!("[rerank] llm: {}", provider.name());

    // 4. Resume support — set RESUME=1 to skip already-done question_ids and
    //    append to the existing OUT file. Default behaviour (no RESUME var)
    //    truncates and starts fresh.
    let resume = std::env::var("RESUME").map(|v| v == "1").unwrap_or(false);
    let mut done_qids: HashSet<String> = HashSet::new();
    if resume {
        if let Ok(prior_out) = fs::read_to_string(&out_path) {
            for line in prior_out.lines() {
                if line.trim().is_empty() { continue; }
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    if let Some(qid) = v.get("question_id").and_then(|x| x.as_str()) {
                        done_qids.insert(qid.to_string());
                    }
                }
            }
            println!("[rerank] RESUME: {} qids already done, will skip", done_qids.len());
        }
    } else {
        // Fresh run — truncate.
        let _ = fs::remove_file(&out_path);
    }
    let mut out_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&out_path)?;

    let mut hit_at_10_before = 0u32;
    let mut hit_at_10_after = 0u32;
    let mut hit_at_50_baseline = 0u32;
    let mut llm_failures = 0u32;
    let mut empty_topk = 0u32;
    let mut llm_calls_made = 0u32;
    let mut gated_skipped = 0u32;
    let mut skipped_resume = 0u32;
    // Per-category counters (before, after, n).
    let mut cat_before: HashMap<String, u32> = HashMap::new();
    let mut cat_after: HashMap<String, u32> = HashMap::new();
    let mut cat_n: HashMap<String, u32> = HashMap::new();
    let total = work.len();
    let t_overall = Instant::now();

    for (idx, inst) in work.iter().enumerate() {
        // RESUME: skip qids already in OUT file.
        if done_qids.contains(&inst.question_id) {
            skipped_resume += 1;
            continue;
        }
        let prior = prior_by_qid.get(&inst.question_id).unwrap();
        let prior_top: &[String] = &prior.top_doc_ids;
        let prior_top: Vec<String> = prior_top.iter().take(TOP_K_FROM_PRIOR).cloned().collect();
        if prior_top.is_empty() {
            empty_topk += 1;
            continue;
        }

        // Sanity counters from prior pass.
        let was_at_10 = *prior.hits.get("k10").unwrap_or(&false);
        if was_at_10 {
            hit_at_10_before += 1;
        }
        if *prior.hits.get("k50").unwrap_or(&false) {
            hit_at_50_baseline += 1;
        }
        *cat_n.entry(inst.question_type.clone()).or_insert(0) += 1;
        if was_at_10 {
            *cat_before.entry(inst.question_type.clone()).or_insert(0) += 1;
        }

        // Reingest just enough to look up bodies for the top-50 doc-ids.
        let brain_path = format!("tmp_rerank_{:03}.said", idx);
        let _ = fs::remove_file(&brain_path);
        let mut sf = SaidFile::create(&brain_path);
        sf.engine.load_static_encoder(encoder)?;
        sf.engine.core.set_holographic_16view(false, None);
        let (answer_doc_ids, bodies) = ingest_with_bodies(&mut sf, inst);
        // We don't actually need to build_index for rerank — bodies map is enough.

        // Build candidate list = (doc_id, body) for the prior top-50.
        let candidates: Vec<(String, String)> = prior_top
            .iter()
            .filter_map(|id| bodies.get(id).map(|b| (id.clone(), b.clone())))
            .collect();
        if candidates.is_empty() {
            // Body lookup miss — shouldn't happen but skip safely.
            empty_topk += 1;
            drop(sf);
            let _ = fs::remove_file(&brain_path);
            continue;
        }

        // Rule R3: only fire the LLM rerank for categories where validation
        // showed it helps. For other categories, trust the no-LLM top-10
        // (i.e. the prior R@10 hit becomes the "after" outcome).
        let should_rerank = match &only_categories {
            Some(allowed) => allowed.contains(&inst.question_type),
            None => true,
        };

        let (ranked_ids, llm_ms): (Vec<String>, u128) = if should_rerank {
            let req = build_rerank_request(
                &inst.question,
                &inst.question_type,
                &candidates,
                RERANK_TOP_N,
            );
            let t_llm = Instant::now();
            let ids = match provider.complete(&req).await {
                Ok(resp) => match resp.json.get("ranked_ids").and_then(|v| v.as_array()) {
                    Some(arr) => arr
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect::<Vec<_>>(),
                    None => Vec::new(),
                },
                Err(e) => {
                    eprintln!("[rerank] {} llm error: {}", inst.question_id, e);
                    llm_failures += 1;
                    Vec::new()
                }
            };
            llm_calls_made += 1;
            (ids, t_llm.elapsed().as_millis())
        } else {
            gated_skipped += 1;
            (Vec::new(), 0)
        };

        // Sanitize the LLM's ranked_ids: drop any id that wasn't in the
        // candidate set (Opus occasionally mistypes session ids — e.g.
        // `21b971b8_2` instead of `81b971b8_2` — and downstream consumers
        // trip over the typos). Backfill the remaining slots from the prior
        // order so we don't leave gaps.
        let candidate_set: HashSet<&String> = candidates.iter().map(|(c, _)| c).collect();
        let sanitized_ranked_ids: Vec<String> = if should_rerank {
            let mut kept: Vec<String> = ranked_ids
                .iter()
                .filter(|id| candidate_set.contains(*id))
                .cloned()
                .collect();
            let kept_set: HashSet<String> = kept.iter().cloned().collect();
            for id in prior_top.iter() {
                if kept.len() >= TOP_K_FROM_PRIOR { break; }
                if !kept_set.contains(id) {
                    kept.push(id.clone());
                }
            }
            kept
        } else {
            Vec::new()
        };

        // Compute new R@10:
        //  - If we reranked: did the LLM put any answer doc-id in its top-10?
        //  - If we gated (skipped): trust the no-LLM result — was the answer
        //    in the original top-10? Use prior_top[..10] as the proxy.
        let new_top10: Vec<String> = if should_rerank {
            sanitized_ranked_ids.iter().take(RERANK_TOP_N).cloned().collect()
        } else {
            prior_top.iter().take(RERANK_TOP_N).cloned().collect()
        };
        let lifted_to_10 = answer_doc_ids
            .iter()
            .any(|aid| new_top10.iter().any(|d| d == aid));
        if lifted_to_10 {
            hit_at_10_after += 1;
            *cat_after.entry(inst.question_type.clone()).or_insert(0) += 1;
        }

        let row = json!({
            "question_id": inst.question_id,
            "question_type": inst.question_type,
            "had_at_10_before": prior.hits.get("k10").copied().unwrap_or(false),
            "had_at_50_before": prior.hits.get("k50").copied().unwrap_or(false),
            "lifted_to_10_after": lifted_to_10,
            "ranked_ids": sanitized_ranked_ids,
            "llm_ms": llm_ms,
        });
        writeln!(out_file, "{}", row)?;

        // Cleanup brain.
        drop(sf);
        let _ = fs::remove_file(&brain_path);

        if (idx + 1) % 5 == 0 || idx + 1 == total {
            let elapsed = t_overall.elapsed().as_secs();
            let rate = elapsed as f32 / (idx + 1) as f32;
            let eta = (rate * (total - idx - 1) as f32) as u64;
            println!(
                "[rerank] {}/{} done — before@10={} after@10={} llm_fail={} elapsed={}s eta={}s",
                idx + 1,
                total,
                hit_at_10_before,
                hit_at_10_after,
                llm_failures,
                elapsed,
                eta
            );
        }
    }

    println!();
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("Phase Q rerank — Session totals");
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("  this session — newly processed: {}", work.len() - skipped_resume as usize);
    println!("  resume-skipped (already done):   {}", skipped_resume);
    println!("  empty top-K skipped:             {}", empty_topk);
    println!("  llm calls made (this session):   {}", llm_calls_made);
    println!("  gated (no LLM, this session):    {}", gated_skipped);
    println!("  llm call failures (this session):{}", llm_failures);

    // Always re-aggregate the FULL OUT file at the end so totals reflect
    // every instance ever processed (covers resume-with-multiple-sessions).
    println!();
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("Cumulative results (from {})", out_path);
    println!("════════════════════════════════════════════════════════════════════════════════");
    let final_raw = fs::read_to_string(&out_path).unwrap_or_default();
    let mut total_b10 = 0u32;
    let mut total_a10 = 0u32;
    let mut total_b50 = 0u32;
    let mut total_count = 0u32;
    let mut cum_cat_b: HashMap<String, u32> = HashMap::new();
    let mut cum_cat_a: HashMap<String, u32> = HashMap::new();
    let mut cum_cat_n: HashMap<String, u32> = HashMap::new();
    for line in final_raw.lines() {
        if line.trim().is_empty() { continue; }
        let v: Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => continue };
        let qtype = v.get("question_type").and_then(|x| x.as_str()).unwrap_or("?").to_string();
        let b10 = v.get("had_at_10_before").and_then(|x| x.as_bool()).unwrap_or(false);
        let a10 = v.get("lifted_to_10_after").and_then(|x| x.as_bool()).unwrap_or(false);
        let b50 = v.get("had_at_50_before").and_then(|x| x.as_bool()).unwrap_or(false);
        total_count += 1;
        if b10 { total_b10 += 1; }
        if a10 { total_a10 += 1; }
        if b50 { total_b50 += 1; }
        *cum_cat_n.entry(qtype.clone()).or_insert(0) += 1;
        if b10 { *cum_cat_b.entry(qtype.clone()).or_insert(0) += 1; }
        if a10 { *cum_cat_a.entry(qtype).or_insert(0) += 1; }
    }
    let n = total_count.max(1) as f32;
    println!("  total processed:         {}", total_count);
    println!();
    println!("  R@10 before rerank:      {} / {} = {:.1}%", total_b10, total_count, total_b10 as f32 / n * 100.0);
    println!("  R@10 after rerank:       {} / {} = {:.1}%", total_a10, total_count, total_a10 as f32 / n * 100.0);
    println!("  R@50 baseline (ceiling): {} / {} = {:.1}%", total_b50, total_count, total_b50 as f32 / n * 100.0);
    println!("  ΔR@10:                   {:+.1}pts", (total_a10 as f32 - total_b10 as f32) / n * 100.0);
    println!();
    println!("  Per-category R@10:");
    println!("    {:<28} {:>6} {:>10} {:>10} {:>8}", "category", "n", "before", "after", "Δ");
    println!("    {:<28} {:>6} {:>10} {:>10} {:>8}",
        "────────────────────────────", "────", "──────────", "──────────", "────────");
    let mut cats: Vec<&String> = cum_cat_n.keys().collect();
    cats.sort();
    for cat in cats {
        let nc = *cum_cat_n.get(cat).unwrap_or(&0);
        let b = *cum_cat_b.get(cat).unwrap_or(&0);
        let a = *cum_cat_a.get(cat).unwrap_or(&0);
        let bp = if nc == 0 { 0.0 } else { b as f32 / nc as f32 * 100.0 };
        let ap = if nc == 0 { 0.0 } else { a as f32 / nc as f32 * 100.0 };
        let dp = ap - bp;
        println!("    {:<28} {:>6} {:>9.1}% {:>9.1}% {:>+7.1}pt", cat, nc, bp, ap, dp);
    }
    // Suppress unused warnings — the per-session counters are now informational.
    let _ = (hit_at_10_before, hit_at_10_after, hit_at_50_baseline, &cat_before, &cat_after, &cat_n);
    println!();
    println!("[rerank] per-instance log: {}", out_path);
    Ok(())
}

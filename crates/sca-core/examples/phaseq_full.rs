//! Phase Q full pipeline validation — rewrite + final rerank, on the prior
//! R@50 misses. Measures whether recovered answers actually land in top-10
//! (the production-relevant cut), not just top-50.
//!
//! Per instance:
//!   1. Reingest haystack + #3 summaries
//!   2. LLM rewrites the question (3 alternatives)
//!   3. Run original + rewrites through PRF → union the top-50 candidate pools
//!   4. LLM reranks the unioned pool → top-10
//!   5. Check whether any answer-bearing doc-id is in the new top-10
//!      (and also top-50, for parity with `phaseq_validate.rs`)
//!
//! Cost: ~76 LLM calls (38 misses × 2 calls each: rewrite + rerank).
//! Wall-clock at ~12s per call ≈ ~15 minutes. No API key required.

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
const PRIOR_RESULTS: &str = "benchmark/longmemeval/results_s_cleaned_summary+prf_all.jsonl";
const OUT: &str = "benchmark/longmemeval/results_phaseq_full.jsonl";

const TOP_K: usize = 50;
const RERANK_TOP_N: usize = 10;
const SNIPPET_CHARS: usize = 320;
const PRF_FB_DOCS: usize = 10;
const PRF_TERMS: usize = 5;
const PRF_MIN_DF: usize = 2;
const SUMMARY_K: usize = 20;
const SUMMARY_MIN: usize = 5;
const REWRITES_PER_QUERY: usize = 3;

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

fn retrieve_topk(sf: &mut SaidFile, query: &str, k: usize) -> Vec<String> {
    let expanded = prf_expand_query(sf, query, PRF_FB_DOCS, PRF_TERMS, PRF_MIN_DF);
    sf.recall(&expanded, k).into_iter().map(|r| r.doc_id).collect()
}

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
    println!("[phaseq-full] encoder: {}", encoder);
    println!("[phaseq-full] dataset: {}", DATASET);
    println!("[phaseq-full] prior:   {}", PRIOR_RESULTS);

    let prior_raw = fs::read_to_string(PRIOR_RESULTS)?;
    let mut miss_ids: Vec<(String, String)> = Vec::new();
    for line in prior_raw.lines() {
        let row: PriorRow = serde_json::from_str(line)?;
        if !row.is_abstention && !row.hits.get("k50").copied().unwrap_or(false) {
            miss_ids.push((row.question_id, row.question_type));
        }
    }
    println!("[phaseq-full] {} R@50 misses to validate", miss_ids.len());

    let dataset_raw = fs::read_to_string(DATASET)?;
    let all_instances: Vec<Instance> = serde_json::from_str(&dataset_raw)?;
    let by_qid: HashMap<String, &Instance> = all_instances
        .iter()
        .map(|i| (i.question_id.clone(), i))
        .collect();

    let llm_cfg = LlmConfig::claude_cli("");
    let provider = provider_from_config(&llm_cfg)?;
    println!("[phaseq-full] llm: {}", provider.name());

    let _ = fs::remove_file(OUT);
    let mut out_file = fs::File::create(OUT)?;

    let mut recovered_at_50 = 0u32;
    let mut recovered_at_10 = 0u32;
    let mut still_missing = 0u32;
    let mut llm_failures = 0u32;
    let total = miss_ids.len();
    let t_overall = Instant::now();

    for (idx, (qid, qtype)) in miss_ids.iter().enumerate() {
        let inst = match by_qid.get(qid) {
            Some(i) => *i,
            None => continue,
        };

        let brain_path = format!("tmp_phaseq_full_{:03}.said", idx);
        let _ = fs::remove_file(&brain_path);
        let mut sf = SaidFile::create(&brain_path);
        sf.engine.load_static_encoder(encoder)?;
        sf.engine.core.set_holographic_16view(false, None);
        let (answer_doc_ids, bodies) = ingest_with_bodies(&mut sf, inst);
        sf.build_index()?;

        let orig_query = if inst.question_date.is_empty() {
            inst.question.clone()
        } else {
            format!("[Today: {}] {}", inst.question_date, inst.question)
        };

        // --- LLM CALL 1: rewrite ---
        let req = build_rewrite_request(&inst.question, qtype, REWRITES_PER_QUERY);
        let rewrites: Vec<String> = match provider.complete(&req).await {
            Ok(resp) => match resp.json.get("rewrites").and_then(|v| v.as_array()) {
                Some(arr) => arr.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                None => Vec::new(),
            },
            Err(e) => {
                eprintln!("[phaseq-full] {} rewrite error: {}", qid, e);
                llm_failures += 1;
                Vec::new()
            }
        };

        // Run original + rewrites, union the candidate pools.
        let mut union_topk: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for q in std::iter::once(orig_query.as_str()).chain(rewrites.iter().map(|s| s.as_str())) {
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

        let recovered_50 = answer_doc_ids.iter().any(|aid| union_topk.iter().any(|d| d == aid));
        if recovered_50 {
            recovered_at_50 += 1;
        }

        // --- LLM CALL 2: rerank the unioned pool down to top-10 ---
        let candidates: Vec<(String, String)> = union_topk
            .iter()
            .filter_map(|id| bodies.get(id).map(|b| (id.clone(), b.clone())))
            .collect();
        let new_top10: Vec<String> = if recovered_50 && !candidates.is_empty() {
            let req = build_rerank_request(&inst.question, qtype, &candidates, RERANK_TOP_N);
            match provider.complete(&req).await {
                Ok(resp) => match resp.json.get("ranked_ids").and_then(|v| v.as_array()) {
                    Some(arr) => arr
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .filter(|id| candidates.iter().any(|(c, _)| c == id))
                        .take(RERANK_TOP_N)
                        .collect(),
                    None => Vec::new(),
                },
                Err(e) => {
                    eprintln!("[phaseq-full] {} rerank error: {}", qid, e);
                    llm_failures += 1;
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let recovered_10 = answer_doc_ids.iter().any(|aid| new_top10.iter().any(|d| d == aid));
        if recovered_10 {
            recovered_at_10 += 1;
        }
        if !recovered_50 {
            still_missing += 1;
        }

        let row = json!({
            "question_id": qid,
            "question_type": qtype,
            "rewrites_count": rewrites.len(),
            "union_pool_size": union_topk.len(),
            "rerank_top10_size": new_top10.len(),
            "recovered_at_50": recovered_50,
            "recovered_at_10": recovered_10,
        });
        writeln!(out_file, "{}", row)?;

        drop(sf);
        let _ = fs::remove_file(&brain_path);

        if (idx + 1) % 5 == 0 || idx + 1 == total {
            let elapsed = t_overall.elapsed().as_secs();
            let rate = elapsed as f32 / (idx + 1) as f32;
            let eta = (rate * (total - idx - 1) as f32) as u64;
            println!(
                "[phaseq-full] {}/{} done — rec@50={} rec@10={} miss={} llm_fail={} elapsed={}s eta={}s",
                idx + 1,
                total,
                recovered_at_50,
                recovered_at_10,
                still_missing,
                llm_failures,
                elapsed,
                eta
            );
        }
    }

    let n = miss_ids.len().max(1) as f32;
    println!();
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("Phase Q full pipeline — rewrite + rerank, on prior R@50 misses");
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("  total misses tested:    {}", miss_ids.len());
    println!("  recovered at R@50:      {}  ({:.1}%)", recovered_at_50, recovered_at_50 as f32 / n * 100.0);
    println!("  recovered at R@10:      {}  ({:.1}%)", recovered_at_10, recovered_at_10 as f32 / n * 100.0);
    println!("  still missing:          {}", still_missing);
    println!("  llm failures:           {}", llm_failures);
    let prior_at_10 = (0.836_f32 * 470.0) as u32;
    let prior_at_50 = 432u32;
    let projected_r10 = (prior_at_10 + recovered_at_10) as f32 / 470.0 * 100.0;
    let projected_r50 = (prior_at_50 + recovered_at_50) as f32 / 470.0 * 100.0;
    println!();
    println!("  projected full R@10: {:.1}%  vs 83.6% baseline (rerank step in production)", projected_r10);
    println!("  projected full R@50: {:.1}%  vs 91.9% baseline", projected_r50);
    println!();
    println!("[phaseq-full] per-instance log: {}", OUT);
    Ok(())
}

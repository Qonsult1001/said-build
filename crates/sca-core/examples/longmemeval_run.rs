//! longmemeval_run — Phase 0 retrieval-only harness against the LongMemEval
//! oracle split. No LLM calls. Measures recall@K — does SAID's top-K window
//! include at least one turn marked `has_answer: true`?
//!
//! Per instance:
//!   1. Fresh in-memory .said brain
//!   2. Ingest every turn of every haystack session, with `[Session date: …]`
//!      prefix (the McCann temporal-anchoring trick) and a per-turn doc_id
//!      `{session_id}#turn_{idx}` so we can identify hits later.
//!   3. Tag each turn with the session id; if the original turn carries
//!      `has_answer: true`, mark it via tag.
//!   4. Run sf.recall(question, top_k=K) for K in {3, 5, 10, 20}.
//!   5. For each K, check whether any returned doc_id corresponds to a turn
//!      that was tagged `has_answer:true`. Aggregate per question_type.
//!
//! Outputs:
//!   - `benchmark/longmemeval/results_oracle_phase0.jsonl` — per-instance log
//!     with question_id, question_type, hits at K3/5/10/20.
//!   - Console summary: overall + per-category R@K table.
//!
//! Run from repo root:
//!   cargo run --release -p sca-core --example longmemeval_run \
//!     --features "static-embed"

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

const DATASET: &str = "benchmark/longmemeval/data/longmemeval_s_cleaned.json";
const K_VALUES: &[usize] = &[3, 5, 10, 20, 50, 100];

// Env-var toggles:
//   LIMIT=N            — run only the first N instances (smoke test)
//   BALANCED=N         — run first N answerable per category
//   SLICE_FILE=path    — JSON file with a list of question_ids to keep
//   TEMPORAL=1         — enable Phase 2 Gaussian temporal scorer
//   TEMPORAL_SIGMA=N   — Gaussian σ in days (default 14)
//   TEMPORAL_ALPHA=N   — boost weight (default 0.30)
//   SESSION_SUMMARY=1  — enable #3 TF-IDF session summary frames
//   SUMMARY_K=N        — top-K terms per session summary (default 20)
//   SUMMARY_MIN=N      — min turns required to emit a summary (default 5)
//   PRF=1              — enable #1 RM3-style pseudo-relevance feedback
//   PRF_FB_DOCS=N      — top-K of 1st retrieval used as feedback (default 10)
//   PRF_TERMS=N        — number of expansion terms appended (default 20)
//   PRF_MIN_DF=N       — min doc-frequency for an expansion term (default 2)
//   EXPAND=1           — enable per-term query expansion (#4: term-neighbour)
//   EXPAND_HEAD=N      — top-N highest-IDF query terms to expand (default 3)
//   EXPAND_NEIGH=N     — neighbours per expanded term (default 2)
//   EXPAND_MIN_DF=N    — min df for an expansion neighbour (default 2)
//   TAG=name           — append _name to results filename (for sweep tracking)
fn results_path(
    temporal: bool,
    summary: bool,
    prf: bool,
    expand: bool,
    limit: Option<usize>,
    balanced: Option<usize>,
    slice: bool,
    tag: Option<&str>,
) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if temporal { parts.push("temporal"); }
    if summary  { parts.push("summary"); }
    if prf      { parts.push("prf"); }
    if expand   { parts.push("expand"); }
    let mode = if parts.is_empty() { "baseline".to_string() } else { parts.join("+") };
    let scope = if slice {
        "slice".to_string()
    } else {
        match (balanced, limit) {
            (Some(n), _)    => format!("bal{}", n),
            (None, Some(n)) => format!("first{}", n),
            (None, None)    => "all".to_string(),
        }
    };
    let tag_suffix = tag.map(|t| format!("_{}", t)).unwrap_or_default();
    format!("benchmark/longmemeval/results_s_cleaned_{}_{}{}.jsonl", mode, scope, tag_suffix)
}

// L1 chunking was tested and abandoned (balanced 60-instance A/B showed it
// hurt single-session-preference by -10pts because chunks compete with their
// parent turn for top-K slots). Removed 2026-04-26.

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2 — Gaussian temporal rerank.
//
// Pure rerank pass over the candidate pool returned by `sf.recall()`. Same
// candidates, just reordered by combining the base recall score with a
// Gaussian decay around the question's date.
//
// Non-destructive: candidates without a parseable date contribute 0 boost,
// so the rerank score equals the base score — worst case = baseline.
//
// Language-agnostic: date parsing handles the LongMemEval format
// "YYYY/MM/DD (Day) HH:MM" but the mechanism works for any date format we
// add to `parse_lme_date`.
// ─────────────────────────────────────────────────────────────────────────────

/// Naive (year, month, day) → Julian-style ordinal day number, no leap-year
/// hair-splitting. Good enough for Δdays at the scale LongMemEval cares about
/// (a few days to a few hundred). Treats every month as 30.4 days.
fn date_to_day_ordinal(y: i32, m: u32, d: u32) -> i32 {
    y * 365 + ((m as i32 - 1) as f32 * 30.4) as i32 + d as i32
}

/// Parse "2023/04/10 (Mon) 23:07" or "2023/04/10" → day ordinal. Returns None
/// if the string doesn't match the expected shape — caller treats None as
/// "no temporal signal," yielding zero boost.
fn parse_lme_date(s: &str) -> Option<i32> {
    let date_part = s.split_whitespace().next()?;
    let mut parts = date_part.split('/');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    Some(date_to_day_ordinal(y, m, d))
}

/// Gaussian decay around the query date. σ in days.
/// Returns 1.0 at Δ=0, drops to ~0.61 at Δ=σ, ~0.14 at Δ=2σ.
fn gaussian_temporal_score(query_day: i32, frame_day: i32, sigma_days: f32) -> f32 {
    let delta = (query_day - frame_day) as f32;
    (-(delta * delta) / (2.0 * sigma_days * sigma_days)).exp()
}

// ─────────────────────────────────────────────────────────────────────────────
// #3 — Statistical session-level fact expansion (TF-IDF, no LLM, no regex).
//
// For each session in an instance: tokenise (whitespace, lowercase, drop short
// tokens), compute TF-IDF where IDF is across the OTHER sessions in the same
// instance, take the top-K terms, emit one extra "Session terms: …" frame.
//
// The summary frame is keyword-dense and short, so it surfaces in candidate
// pools where the original 50-turn session was too diffuse to rank.
//
// Pure structure. Works on any whitespace-tokenised language.
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

/// Compute top-K TF-IDF terms for each session in an instance.
/// Returns: Vec aligned with `inst.haystack_sessions` — each entry is a
/// space-joined string of top-K terms (or empty if session is below
/// `min_turns`).
fn session_summary_terms(
    inst: &Instance,
    k_terms: usize,
    min_turns: usize,
) -> Vec<String> {
    let n_sessions = inst.haystack_sessions.len();
    if n_sessions == 0 {
        return Vec::new();
    }

    // Build per-session token lists.
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

    // Document frequency over sessions: in how many sessions does term appear?
    let mut df: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for toks in &session_tokens {
        let unique: HashSet<&str> = toks.iter().map(|s| s.as_str()).collect();
        for t in unique {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    let n = session_tokens.len() as f32;

    // For each session, score terms by TF-IDF and pick top-K.
    let mut out = Vec::with_capacity(n_sessions);
    for (s_idx, toks) in session_tokens.iter().enumerate() {
        let n_turns = inst.haystack_sessions[s_idx].len();
        if n_turns < min_turns {
            out.push(String::new());
            continue;
        }
        let mut tf: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
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

// ─────────────────────────────────────────────────────────────────────────────
// #1 — RM3-style pseudo-relevance feedback (PRF).
//
// Per query:
//   1. First retrieval over top-N pseudo-relevant feedback docs.
//   2. Tokenise their bodies, score each non-stopword term by
//        score(term) = Σ_d∈feedback  tf(term, d)  ·  ln(N / df(term))
//      where N is the size of the feedback set and df is computed within it.
//   3. Take top-K expansion terms (filtered by min_df), append them to the
//      original query, re-run retrieval. Returns the second retrieval's
//      top-K — which may now contain frames that didn't make the first pass.
//
// Pure structure. Works on any whitespace-tokenised language. No LLM.
// ─────────────────────────────────────────────────────────────────────────────

// No hardcoded stopword list. Function words have low IDF naturally — the
// `idf = ln(N/df)` term in the scoring will already drive them toward zero,
// regardless of language. This keeps PRF language-agnostic.

/// Run pseudo-relevance feedback. Returns the expanded query string (original
/// + space-joined expansion terms). Empty input or no feedback frames just
/// returns the original.
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

    // Don't pull expansion terms from words already in the query.
    let query_terms: HashSet<String> = tokenize_for_summary(query).into_iter().collect();

    // df: in how many feedback docs does the term appear?
    let mut df: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
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

    // RM-like score: Σ_d  tf(term, d) · ln(N / df(term)).
    let mut scores: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    for toks in &tokenised {
        let mut tf_local: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for t in toks {
            *tf_local.entry(t.as_str()).or_insert(0) += 1;
        }
        for (term, count) in tf_local {
            // Don't pull terms already in the query — they don't expand anything.
            if query_terms.contains(term) {
                continue;
            }
            let df_t = *df.get(term).unwrap_or(&1);
            if df_t < min_df {
                continue;
            }
            // ln(N/df) goes to zero when a term appears in every feedback doc,
            // which is the language-agnostic stopword filter.
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

// ─────────────────────────────────────────────────────────────────────────────
// #4 — Per-term query expansion (term-neighbour, no LLM).
//
// For each of the N highest-IDF tokens in the query, run a tiny retrieval
// using that term alone as the query. From the bodies of those hits, harvest
// the highest-IDF co-occurring terms (excluding tokens already in the query),
// take top-K per term, append all to the original query.
//
// Targets the diagnosed VOCAB-MISMATCH gap: when a question asks "what breed
// is my dog?", the evidence says "Golden Retriever like Max". Per-term
// expansion of "dog" surfaces "max", "retriever", "golden", "collar" etc.
// from frames where "dog" appears, bridging the conceptual gap.
//
// Pure structure. Works on any whitespace-tokenised language. No LLM.
// ─────────────────────────────────────────────────────────────────────────────

/// Build a corpus-wide DF map for tokens of length ≥ 3, for IDF scoring.
/// One pass over the recall pool; `n_corpus_sample` controls the breadth.
fn corpus_df_sample(
    sf: &mut SaidFile,
    sample_query: &str,
    n_sample: usize,
) -> (std::collections::HashMap<String, usize>, usize) {
    let pool = sf.recall(sample_query, n_sample);
    let mut df: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in &pool {
        let unique: HashSet<String> = tokenize_for_summary(&r.content).into_iter().collect();
        for t in unique {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    (df, pool.len())
}

/// Per-term expansion. Returns the original query with appended expansion
/// neighbours.
fn expand_query_per_term(
    sf: &mut SaidFile,
    query: &str,
    head_terms: usize,
    neighbours_per_term: usize,
    min_df: usize,
) -> String {
    if head_terms == 0 || neighbours_per_term == 0 {
        return query.to_string();
    }

    let q_tokens: Vec<String> = tokenize_for_summary(query);
    if q_tokens.is_empty() {
        return query.to_string();
    }
    let q_set: HashSet<String> = q_tokens.iter().cloned().collect();

    // Sample the corpus to build a rough DF table for scoring head IDF.
    let (df, n_sample) = corpus_df_sample(sf, query, 50);
    if n_sample == 0 || df.is_empty() {
        return query.to_string();
    }
    let n = n_sample as f32;

    // Score query terms by IDF — pick the rare ones (the "specific" terms).
    let mut query_idf: Vec<(String, f32)> = q_set
        .iter()
        .map(|t| {
            let df_t = *df.get(t).unwrap_or(&1) as f32;
            let idf = (n / df_t).ln().max(0.0);
            (t.clone(), idf)
        })
        .collect();
    query_idf.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    query_idf.truncate(head_terms);

    let mut all_neighbours: Vec<(String, f32)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (head_term, _) in query_idf {
        // Skip near-stopword terms (idf == 0 means appears in every sampled doc).
        if df.get(&head_term).copied().unwrap_or(0) >= n_sample.max(1) {
            continue;
        }
        // Mini-retrieval using just this one term.
        let micro_pool = sf.recall(&head_term, 5);
        if micro_pool.is_empty() {
            continue;
        }
        // Harvest co-occurring high-IDF terms.
        let mut local_scores: std::collections::HashMap<String, f32> =
            std::collections::HashMap::new();
        for r in &micro_pool {
            let toks = tokenize_for_summary(&r.content);
            let mut tf_local: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for t in toks {
                *tf_local.entry(t).or_insert(0) += 1;
            }
            for (t, count) in tf_local {
                if q_set.contains(&t) || t == head_term {
                    continue;
                }
                let df_t = *df.get(&t).unwrap_or(&1);
                if df_t < min_df {
                    continue;
                }
                let idf = (n / df_t as f32).ln().max(0.0);
                if idf <= 0.0 {
                    continue;
                }
                *local_scores.entry(t).or_insert(0.0) += count as f32 * idf;
            }
        }
        let mut ranked: Vec<(String, f32)> = local_scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (t, s) in ranked.into_iter().take(neighbours_per_term) {
            if seen.insert(t.clone()) {
                all_neighbours.push((t, s));
            }
        }
    }

    if all_neighbours.is_empty() {
        return query.to_string();
    }
    let expansion: Vec<String> = all_neighbours.into_iter().map(|(t, _)| t).collect();
    format!("{} {}", query, expansion.join(" "))
}

/// Rerank a top-K result list by combining base recall score with the
/// Gaussian temporal score. α controls how much temporal influences ranking;
/// 0 = pure baseline, larger = more temporal bias.
fn rerank_temporal(
    results: Vec<sca_core::said_file::RecallResult>,
    doc_dates: &std::collections::HashMap<String, i32>,
    query_day: Option<i32>,
    sigma_days: f32,
    alpha: f32,
) -> Vec<sca_core::said_file::RecallResult> {
    let Some(qd) = query_day else { return results };
    let mut scored: Vec<(sca_core::said_file::RecallResult, f32)> = results
        .into_iter()
        .map(|r| {
            let g = doc_dates
                .get(&r.doc_id)
                .map(|d| gaussian_temporal_score(qd, *d, sigma_days))
                .unwrap_or(0.0);
            let final_score = r.score * (1.0 + alpha * g);
            (r, final_score)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(r, _)| r).collect()
}

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

#[derive(Debug, Serialize)]
struct ResultRow {
    question_id: String,
    question_type: String,
    is_abstention: bool,
    hits: BTreeMap<String, bool>, // "k3" -> true/false
    top_doc_ids: Vec<String>,
    elapsed_ms: u128,
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

/// Returns (answer_doc_ids, doc_id → day_ordinal map for temporal rerank).
fn ingest_instance(
    sf: &mut SaidFile,
    inst: &Instance,
    summary_enabled: bool,
    summary_k: usize,
    summary_min: usize,
) -> (HashSet<String>, std::collections::HashMap<String, i32>) {
    let mut answer_doc_ids: HashSet<String> = HashSet::new();
    let mut doc_dates: std::collections::HashMap<String, i32> = std::collections::HashMap::new();

    // Pre-compute session summaries if enabled.
    let summaries: Vec<String> = if summary_enabled {
        session_summary_terms(inst, summary_k, summary_min)
    } else {
        Vec::new()
    };

    for (s_idx, session) in inst.haystack_sessions.iter().enumerate() {
        let session_id = inst
            .haystack_session_ids
            .get(s_idx)
            .cloned()
            .unwrap_or_else(|| format!("session_{}", s_idx));
        let session_date = inst
            .haystack_dates
            .get(s_idx)
            .cloned()
            .unwrap_or_default();

        let mut session_has_answer = false;

        for (t_idx, turn) in session.iter().enumerate() {
            let role = turn
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user");
            let content = turn
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if content.is_empty() {
                continue;
            }
            let has_answer = turn
                .get("has_answer")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // McCann-style session-date anchoring + role label
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

            if let Some(d) = parse_lme_date(&session_date) {
                doc_dates.insert(doc_id, d);
            }
        }

        // ── #3 — emit per-session TF-IDF summary frame ───────────────────
        if summary_enabled {
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
                        // Mirror the has_answer signal onto the summary so a
                        // summary hit credits the same answer.
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
                    if let Some(d) = parse_lme_date(&session_date) {
                        doc_dates.insert(summary_doc_id, d);
                    }
                }
            }
        }
    }

    (answer_doc_ids, doc_dates)
}

fn run_one(
    sf: &mut SaidFile,
    inst: &Instance,
    answer_doc_ids: &HashSet<String>,
    doc_dates: &std::collections::HashMap<String, i32>,
    temporal_enabled: bool,
    sigma_days: f32,
    alpha: f32,
    prf_enabled: bool,
    prf_fb_docs: usize,
    prf_terms: usize,
    prf_min_df: usize,
    expand_enabled: bool,
    expand_head: usize,
    expand_neigh: usize,
    expand_min_df: usize,
) -> ResultRow {
    let start = Instant::now();
    let max_k = *K_VALUES.iter().max().unwrap_or(&20);

    // Prepend the question's own date so temporal-reasoning queries can anchor
    // "today" / "now" properly. Doesn't change retrieval much for non-temporal
    // questions — small lift for temporal ones.
    let qtext = if inst.question_date.is_empty() {
        inst.question.clone()
    } else {
        format!("[Today: {}] {}", inst.question_date, inst.question)
    };

    // #4 — Per-term query expansion (term-neighbour). Runs BEFORE PRF so
    // PRF's seed retrieval sees the wider query.
    let qtext = if expand_enabled {
        expand_query_per_term(sf, &qtext, expand_head, expand_neigh, expand_min_df)
    } else {
        qtext
    };

    // #1 — PRF expansion (run a 1st retrieval to harvest expansion terms,
    // then build an expanded query for the actual retrieval).
    let qtext = if prf_enabled {
        prf_expand_query(sf, &qtext, prf_fb_docs, prf_terms, prf_min_df)
    } else {
        qtext
    };

    let raw_results = sf.recall(&qtext, max_k);

    // Phase 2 — temporal Gaussian rerank (no-op if disabled or no question_date).
    let results = if temporal_enabled {
        let qd = parse_lme_date(&inst.question_date);
        rerank_temporal(raw_results, doc_dates, qd, sigma_days, alpha)
    } else {
        raw_results
    };

    let elapsed_ms = start.elapsed().as_millis();

    let top_doc_ids: Vec<String> =
        results.iter().map(|r| r.doc_id.clone()).collect();

    let mut hits: BTreeMap<String, bool> = BTreeMap::new();
    for &k in K_VALUES {
        let window: HashSet<&String> = top_doc_ids.iter().take(k).collect();
        let hit = answer_doc_ids
            .iter()
            .any(|aid| window.contains(aid));
        hits.insert(format!("k{}", k), hit);
    }

    let is_abstention = inst.question_id.ends_with("_abs");

    ResultRow {
        question_id: inst.question_id.clone(),
        question_type: inst.question_type.clone(),
        is_abstention,
        hits,
        top_doc_ids: top_doc_ids.into_iter().take(max_k).collect(),
        elapsed_ms,
    }
}

fn print_summary(rows: &[ResultRow]) {
    println!("\n════════════════════════════════════════════════════════════════════════════════");
    println!("LongMemEval oracle — Phase 0 retrieval-only R@K");
    println!("════════════════════════════════════════════════════════════════════════════════");

    // Overall (excluding abstention — those have no answer to retrieve)
    let answerable: Vec<&ResultRow> = rows.iter().filter(|r| !r.is_abstention).collect();
    let n_answer = answerable.len();
    let n_abs = rows.len() - n_answer;
    println!(
        "Total: {} instances ({} answerable, {} abstention)",
        rows.len(),
        n_answer,
        n_abs
    );
    println!();

    // Per-K overall
    println!("Overall R@K (answerable only, n={}):", n_answer);
    for &k in K_VALUES {
        let key = format!("k{}", k);
        let hits = answerable
            .iter()
            .filter(|r| *r.hits.get(&key).unwrap_or(&false))
            .count();
        let pct = (hits as f32 / n_answer.max(1) as f32) * 100.0;
        println!("  R@{:<3} = {:>5.1}%  ({}/{})", k, pct, hits, n_answer);
    }

    // Per-category
    println!("\nPer-category R@K (answerable only):");
    let mut by_cat: BTreeMap<String, Vec<&ResultRow>> = BTreeMap::new();
    for r in &answerable {
        by_cat
            .entry(r.question_type.clone())
            .or_default()
            .push(*r);
    }
    println!(
        "  {:<28} {:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "category", "n", "R@3", "R@5", "R@10", "R@20", "R@50", "R@100"
    );
    println!(
        "  {:<28} {:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "────────────────────────────",
        "────",
        "───────",
        "───────",
        "───────",
        "───────",
        "───────",
        "───────"
    );
    for (cat, rs) in &by_cat {
        let n = rs.len();
        let pct = |key: &str| -> f32 {
            let h = rs.iter().filter(|r| *r.hits.get(key).unwrap_or(&false)).count();
            (h as f32 / n.max(1) as f32) * 100.0
        };
        println!(
            "  {:<28} {:>5} {:>7.1}% {:>7.1}% {:>7.1}% {:>7.1}% {:>7.1}% {:>7.1}%",
            cat, n, pct("k3"), pct("k5"), pct("k10"), pct("k20"), pct("k50"), pct("k100")
        );
    }

    // Latency stats
    let mut times: Vec<u128> = rows.iter().map(|r| r.elapsed_ms).collect();
    times.sort_unstable();
    let total: u128 = times.iter().sum();
    let p50 = times.get(times.len() / 2).copied().unwrap_or(0);
    let p95 = times.get((times.len() as f32 * 0.95) as usize).copied().unwrap_or(0);
    println!(
        "\nLatency (ingest+index+recall, per instance): total={}ms  p50={}ms  p95={}ms",
        total, p50, p95
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let encoder = find_encoder()?;

    let temporal_enabled = std::env::var("TEMPORAL")
        .map(|v| v == "1")
        .unwrap_or(false);
    let temporal_sigma: f32 = std::env::var("TEMPORAL_SIGMA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(14.0);
    let temporal_alpha: f32 = std::env::var("TEMPORAL_ALPHA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.30);
    let summary_enabled = std::env::var("SESSION_SUMMARY")
        .map(|v| v == "1")
        .unwrap_or(false);
    let summary_k: usize = std::env::var("SUMMARY_K")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let summary_min: usize = std::env::var("SUMMARY_MIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let prf_enabled = std::env::var("PRF").map(|v| v == "1").unwrap_or(false);
    let prf_fb_docs: usize = std::env::var("PRF_FB_DOCS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let prf_terms: usize = std::env::var("PRF_TERMS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let prf_min_df: usize = std::env::var("PRF_MIN_DF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let expand_enabled = std::env::var("EXPAND").map(|v| v == "1").unwrap_or(false);
    let expand_head: usize = std::env::var("EXPAND_HEAD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let expand_neigh: usize = std::env::var("EXPAND_NEIGH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let expand_min_df: usize = std::env::var("EXPAND_MIN_DF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let limit: Option<usize> = std::env::var("LIMIT")
        .ok()
        .and_then(|v| v.parse().ok());
    let balanced_for_path: Option<usize> = std::env::var("BALANCED")
        .ok()
        .and_then(|v| v.parse().ok());
    let slice_file: Option<String> = std::env::var("SLICE_FILE").ok();
    let tag: Option<String> = std::env::var("TAG").ok();
    let results = results_path(
        temporal_enabled,
        summary_enabled,
        prf_enabled,
        expand_enabled,
        limit,
        balanced_for_path,
        slice_file.is_some(),
        tag.as_deref(),
    );

    println!("[harness] encoder: {}", encoder);
    println!("[harness] dataset: {}", DATASET);
    println!(
        "[harness] mode: temporal={}{} summary={}{} prf={}{} expand={}{} slice={} limit={}",
        temporal_enabled,
        if temporal_enabled {
            format!(" σ={:.1}d α={:.2}", temporal_sigma, temporal_alpha)
        } else { String::new() },
        summary_enabled,
        if summary_enabled {
            format!(" K={} min={}", summary_k, summary_min)
        } else { String::new() },
        prf_enabled,
        if prf_enabled {
            format!(" fb={} terms={} mindf={}", prf_fb_docs, prf_terms, prf_min_df)
        } else { String::new() },
        expand_enabled,
        if expand_enabled {
            format!(" head={} neigh={} mindf={}", expand_head, expand_neigh, expand_min_df)
        } else { String::new() },
        slice_file.as_deref().unwrap_or("-"),
        limit.map(|n| n.to_string()).unwrap_or_else(|| "all".into())
    );
    println!("[harness] results → {}\n", results);

    let raw = fs::read_to_string(DATASET)
        .map_err(|e| format!("dataset read failed: {} — run from repo root", e))?;
    let all_instances: Vec<Instance> = serde_json::from_str(&raw)?;

    // BALANCED=N — take first N answerable instances of each question_type
    // (gives a category-balanced slice for honest A/B without running 500).
    let balanced_n: Option<usize> = std::env::var("BALANCED")
        .ok()
        .and_then(|v| v.parse().ok());

    // Optional question_id filter from a JSON file (e.g. "hard temporal" slice).
    let slice_qids: Option<HashSet<String>> = slice_file.as_ref().map(|p| {
        let raw = fs::read_to_string(p).expect("SLICE_FILE read failed");
        let v: Vec<String> = serde_json::from_str(&raw).expect("SLICE_FILE not a JSON list");
        v.into_iter().collect()
    });

    let all_instances: Vec<Instance> = match &slice_qids {
        Some(qids) => all_instances
            .into_iter()
            .filter(|i| qids.contains(&i.question_id))
            .collect(),
        None => all_instances,
    };

    let mut instances: Vec<Instance> = if let Some(per_cat) = balanced_n {
        let mut by_cat: BTreeMap<String, Vec<Instance>> = BTreeMap::new();
        for inst in all_instances {
            if inst.question_id.ends_with("_abs") {
                continue; // abstention has no answer to retrieve
            }
            by_cat.entry(inst.question_type.clone()).or_default().push(inst);
        }
        let mut picked = Vec::new();
        for (cat, mut v) in by_cat {
            v.truncate(per_cat);
            println!("[harness] balanced: {} {} instances", v.len(), cat);
            picked.extend(v);
        }
        picked
    } else {
        all_instances
    };

    if let Some(n) = limit {
        instances.truncate(n);
    }
    println!("[harness] {} instances loaded\n", instances.len());

    let _ = fs::remove_file(&results);
    let _ = fs::create_dir_all(PathBuf::from(&results).parent().unwrap_or(Path::new(".")));
    let mut out = BufWriter::new(File::create(&results)?);

    let mut rows: Vec<ResultRow> = Vec::new();
    let total = instances.len();
    let t_overall = Instant::now();

    for (idx, inst) in instances.iter().enumerate() {
        let brain_path = format!("tmp_lme_inst_{:04}.said", idx);
        let _ = fs::remove_file(&brain_path);
        let mut sf = SaidFile::create(&brain_path);
        sf.engine.load_static_encoder(encoder)?;
        sf.engine.core.set_holographic_16view(false, None);

        let (answer_doc_ids, doc_dates) =
            ingest_instance(&mut sf, inst, summary_enabled, summary_k, summary_min);
        sf.build_index()?;

        let row = run_one(
            &mut sf,
            inst,
            &answer_doc_ids,
            &doc_dates,
            temporal_enabled,
            temporal_sigma,
            temporal_alpha,
            prf_enabled,
            prf_fb_docs,
            prf_terms,
            prf_min_df,
            expand_enabled,
            expand_head,
            expand_neigh,
            expand_min_df,
        );
        writeln!(out, "{}", serde_json::to_string(&row)?)?;
        rows.push(row);

        // Cleanup the per-instance brain
        drop(sf);
        let _ = fs::remove_file(&brain_path);

        if (idx + 1) % 25 == 0 || idx + 1 == total {
            let elapsed = t_overall.elapsed().as_secs();
            let pace = elapsed as f32 / (idx + 1) as f32;
            let eta = (pace * (total - idx - 1) as f32) as u64;
            println!(
                "[harness] {}/{} done — {}s elapsed, ~{}s remaining",
                idx + 1,
                total,
                elapsed,
                eta
            );
        }
    }
    out.flush()?;

    print_summary(&rows);

    println!("\n[harness] per-instance log: {}", results);
    Ok(())
}

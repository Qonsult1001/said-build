//! MorphBM25 smoke test on a hand-picked 14-instance slice.
//!
//! Direct Rust port of `test_said_final.py`'s MorphBM25 (the lexical engine
//! that beat BM25 on BEIR). Pure-lexical: no encoder, no fingerprints, no
//! LLM. Used as a drop-in *replacement* for the no-LLM retrieval path on
//! this slice — clean A/B vs the existing #3+PRF baseline.
//!
//! Slice (14 single-session questions):
//!   - 7 R@10 misses (shortest first) — should be lifted
//!   - 7 R@10 hits (random) — regression check, must not be hurt
//!
//! Run:
//!   cargo run --release -p sca-core --example morphbm25_smoke \
//!     --features static-embed

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

const DATASET: &str = "benchmark/longmemeval/data/longmemeval_s_cleaned.json";
const PRIOR: &str = "benchmark/longmemeval/results_s_cleaned_summary+prf_all.jsonl";
const SLICE: &str = "benchmark/longmemeval/morphbm25_smoke_qids.json";

const BM25_K1: f32 = 1.2;
const BM25_B: f32 = 0.75;
const ANDPAIR_BONUS: f32 = 1.0;
const MAX_PAIR_DF: usize = 10;
const SUMMARY_K: usize = 20;
const SUMMARY_MIN: usize = 5;

// Same English stopword set as test_said_final.py (`STOP`).
const STOPWORDS: &[&str] = &[
    "the","a","an","is","are","was","were","be","been","being","have","has","had",
    "do","does","did","will","would","shall","should","can","could","may","might","must",
    "to","of","in","for","on","at","by","with","from","as","into","through","during",
    "before","after","above","below","between","under","not","no","nor","but","or","and",
    "so","yet","both","either","neither","each","every","all","any","few","many","some",
    "most","much","such","own","other","another","only","very","also","back","just",
    "about","out","up","over","down","off","still","again","further","then","once",
    "here","there","when","where","why","how","more","these","those","his","her","he",
    "she","they","their","it","its","this","that","what","who","which","you","your",
    "we","our","them",
];

fn is_stop(s: &str) -> bool {
    STOPWORDS.binary_search(&s).is_ok()
        || STOPWORDS.iter().any(|w| *w == s)
}

/// Tokenise like test_said_final.py: lowercase, [a-z]+ only, len >= 3, drop stops.
fn tokenize(text: &str) -> Vec<String> {
    text.chars()
        .map(|c| if c.is_ascii_alphabetic() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.len() >= 3 && !is_stop(w))
        .map(|s| s.to_string())
        .collect()
}

/// Direct port of `morph_variants` from test_said_final.py.
fn morph_variants(word: &str) -> Vec<String> {
    if word.len() < 4 {
        return vec![word.to_string()];
    }
    let mut v: HashSet<String> = HashSet::new();
    v.insert(word.to_string());
    let n = word.len();

    if word.ends_with("ies") && n > 5 {
        v.insert(format!("{}y", &word[..n - 3]));
    }
    if word.ends_with("es") && n > 4 {
        v.insert(word[..n - 2].to_string());
    }
    if word.ends_with('s') && n > 4 && !word.ends_with("ss") {
        v.insert(word[..n - 1].to_string());
    }
    if word.ends_with("ing") && n > 5 {
        v.insert(word[..n - 3].to_string());
        v.insert(format!("{}e", &word[..n - 3]));
    }
    if word.ends_with("ed") && n > 4 {
        v.insert(word[..n - 2].to_string());
        v.insert(word[..n - 1].to_string());
    }
    if word.ends_with("ers") && n > 5 {
        v.insert(word[..n - 3].to_string());
        v.insert(format!("{}er", &word[..n - 3]));
    }
    v.into_iter().collect()
}

struct MorphBM25 {
    k1: f32,
    b: f32,
    n: usize,
    /// doc_id → {term → count}
    tf: HashMap<String, HashMap<String, u32>>,
    /// doc_id → doc length (in tokens after stopword filtering)
    dl: HashMap<String, usize>,
    /// term → IDF
    idf: HashMap<String, f32>,
    /// term → set of doc_ids containing it
    inv: HashMap<String, HashSet<String>>,
    avgdl: f32,
}

impl MorphBM25 {
    fn build(docs: &[(String, String)]) -> Self {
        let mut tf: HashMap<String, HashMap<String, u32>> = HashMap::new();
        let mut dl: HashMap<String, usize> = HashMap::new();
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut total_len: usize = 0;
        for (did, txt) in docs {
            let toks = tokenize(txt);
            let mut tfm: HashMap<String, u32> = HashMap::new();
            for t in &toks {
                *tfm.entry(t.clone()).or_insert(0) += 1;
            }
            for t in tfm.keys() {
                *df.entry(t.clone()).or_insert(0) += 1;
            }
            total_len += toks.len();
            dl.insert(did.clone(), toks.len());
            tf.insert(did.clone(), tfm);
        }
        let n = docs.len();
        let avgdl = if n > 0 { total_len as f32 / n as f32 } else { 1.0 };
        let mut idf: HashMap<String, f32> = HashMap::new();
        for (t, d) in &df {
            let val = ((n as f32 - *d as f32 + 0.5) / (*d as f32 + 0.5) + 1.0).ln();
            idf.insert(t.clone(), val);
        }
        let mut inv: HashMap<String, HashSet<String>> = HashMap::new();
        for (did, tfm) in &tf {
            for t in tfm.keys() {
                inv.entry(t.clone()).or_default().insert(did.clone());
            }
        }
        Self {
            k1: BM25_K1,
            b: BM25_B,
            n,
            tf,
            dl,
            idf,
            inv,
            avgdl,
        }
    }

    fn morph_tf(&self, did: &str, q_term: &str) -> u32 {
        let variants = morph_variants(q_term);
        let mut total: u32 = 0;
        if let Some(tfm) = self.tf.get(did) {
            for v in &variants {
                total = total.saturating_add(*tfm.get(v).unwrap_or(&0));
            }
        }
        total
    }

    fn morph_idf(&self, q_term: &str) -> f32 {
        let variants = morph_variants(q_term);
        let mut s: HashSet<String> = HashSet::new();
        for v in &variants {
            if let Some(docs) = self.inv.get(v) {
                for d in docs {
                    s.insert(d.clone());
                }
            }
        }
        let d = if !s.is_empty() {
            s.len()
        } else {
            // fall back to df of the original
            return *self.idf.get(q_term).unwrap_or(&0.0);
        };
        if d == 0 {
            return 0.0;
        }
        ((self.n as f32 - d as f32 + 0.5) / (d as f32 + 0.5) + 1.0).ln()
    }

    fn morph_docs(&self, q_term: &str) -> HashSet<String> {
        let variants = morph_variants(q_term);
        let mut s: HashSet<String> = HashSet::new();
        for v in &variants {
            if let Some(docs) = self.inv.get(v) {
                for d in docs {
                    s.insert(d.clone());
                }
            }
        }
        s
    }

    fn search(&self, query: &str, top_k: usize) -> Vec<(String, f32)> {
        let q_toks = tokenize(query);
        if q_toks.is_empty() {
            return Vec::new();
        }
        let mut scores: HashMap<String, f32> = HashMap::new();

        // Standard BM25 with morph variants summed into TF/IDF.
        for t in &q_toks {
            let idf = self.morph_idf(t);
            if idf <= 0.0 {
                continue;
            }
            for did in self.morph_docs(t) {
                let tf = self.morph_tf(&did, t) as f32;
                if tf == 0.0 {
                    continue;
                }
                let dl = *self.dl.get(&did).unwrap_or(&0) as f32;
                let denom = tf + self.k1 * (1.0 - self.b + self.b * dl / self.avgdl);
                if denom > 0.0 {
                    let contrib = idf * tf * (self.k1 + 1.0) / denom;
                    *scores.entry(did).or_insert(0.0) += contrib;
                }
            }
        }

        // AND-pair bonus: pairs of rare query terms that co-occur in <=3 docs
        // get a big bonus (signal of a specific match).
        let rare: Vec<(String, HashSet<String>)> = q_toks
            .iter()
            .map(|t| (t.clone(), self.morph_docs(t)))
            .filter(|(_, docs)| !docs.is_empty() && docs.len() <= MAX_PAIR_DF)
            .collect();
        for i in 0..rare.len() {
            for j in (i + 1)..rare.len() {
                let inter: HashSet<&String> =
                    rare[i].1.intersection(&rare[j].1).collect();
                if inter.len() <= 3 {
                    let b_val =
                        ANDPAIR_BONUS * (self.morph_idf(&rare[i].0) + self.morph_idf(&rare[j].0));
                    for did in inter {
                        *scores.entry(did.clone()).or_insert(0.0) += b_val;
                    }
                }
            }
        }

        let mut sorted: Vec<(String, f32)> = scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(top_k);
        sorted
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Dataset loading + ingest helpers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Instance {
    question_id: String,
    question_type: String,
    question: String,
    #[allow(dead_code)]
    answer: Value,
    #[serde(default)]
    haystack_dates: Vec<String>,
    haystack_session_ids: Vec<String>,
    haystack_sessions: Vec<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct PriorRow {
    question_id: String,
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

/// Build (doc_id, body) list AND the answer-doc-id set, mirroring the
/// production ingest path that the LongMemEval harness uses (#3 summaries
/// included so this is a fair A/B).
fn build_corpus(inst: &Instance) -> (Vec<(String, String)>, HashSet<String>) {
    let mut docs: Vec<(String, String)> = Vec::new();
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
            if has_answer {
                answer_doc_ids.insert(doc_id.clone());
                session_has_answer = true;
            }
            docs.push((doc_id, body));
        }

        // #3 — per-session TF-IDF summary frame (same as production).
        if let Some(terms) = summaries.get(s_idx) {
            if !terms.is_empty() {
                let summary_doc_id = format!("{}#summary", session_id);
                let summary_body = if session_date.is_empty() {
                    format!("Session terms: {}", terms)
                } else {
                    format!("[Session date: {}] Session terms: {}", session_date, terms)
                };
                if session_has_answer {
                    answer_doc_ids.insert(summary_doc_id.clone());
                }
                docs.push((summary_doc_id, summary_body));
            }
        }
    }
    (docs, answer_doc_ids)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("[smoke] dataset: {}", DATASET);
    println!("[smoke] prior:   {}", PRIOR);
    println!("[smoke] slice:   {}", SLICE);

    let slice_qids: Vec<String> = serde_json::from_str(&fs::read_to_string(SLICE)?)?;
    let slice_set: HashSet<String> = slice_qids.iter().cloned().collect();
    println!("[smoke] {} qids in slice", slice_qids.len());

    let prior_raw = fs::read_to_string(PRIOR)?;
    let mut prior_by_qid: HashMap<String, PriorRow> = HashMap::new();
    for line in prior_raw.lines() {
        let row: PriorRow = serde_json::from_str(line)?;
        prior_by_qid.insert(row.question_id.clone(), row);
    }

    let dataset_raw = fs::read_to_string(DATASET)?;
    let all_instances: Vec<Instance> = serde_json::from_str(&dataset_raw)?;
    let work: Vec<&Instance> = all_instances
        .iter()
        .filter(|i| slice_set.contains(&i.question_id))
        .collect();
    println!("[smoke] {} instances loaded\n", work.len());

    println!("{:>3} {:<26} {:<10} {:<8} {:<8} {:<8}", "#", "qid", "cat", "prior@10", "morph@10", "result");
    println!("{:-<3} {:-<26} {:-<10} {:-<8} {:-<8} {:-<8}", "", "", "", "", "", "");

    let mut prior_hit_at_10 = 0;
    let mut morph_hit_at_10 = 0;
    let mut prior_hit_at_50 = 0;
    let mut morph_hit_at_50 = 0;
    let mut lifted = 0;
    let mut hurt = 0;

    for (i, inst) in work.iter().enumerate() {
        let prior = prior_by_qid
            .get(&inst.question_id)
            .ok_or_else(|| format!("prior missing for {}", inst.question_id))?;
        let prior_at_10 = prior.hits.get("k10").copied().unwrap_or(false);
        let prior_at_50 = prior.hits.get("k50").copied().unwrap_or(false);

        let (docs, answer_doc_ids) = build_corpus(inst);
        let bm = MorphBM25::build(&docs);
        let results = bm.search(&inst.question, 50);

        let top10: Vec<&String> = results.iter().take(10).map(|(d, _)| d).collect();
        let morph_at_10 = answer_doc_ids.iter().any(|aid| top10.iter().any(|d| *d == aid));
        let morph_at_50 = answer_doc_ids
            .iter()
            .any(|aid| results.iter().any(|(d, _)| d == aid));

        if prior_at_10 { prior_hit_at_10 += 1; }
        if morph_at_10 { morph_hit_at_10 += 1; }
        if prior_at_50 { prior_hit_at_50 += 1; }
        if morph_at_50 { morph_hit_at_50 += 1; }

        let result = match (prior_at_10, morph_at_10) {
            (false, true) => { lifted += 1; "LIFTED" }
            (true, false) => { hurt += 1; "HURT" }
            (true, true) => "kept",
            (false, false) => "still miss",
        };

        let cat_short: String = inst
            .question_type
            .replace("single-session-", "ss-")
            .replace("multi-session", "ms")
            .replace("knowledge-update", "ku")
            .replace("temporal-reasoning", "tr");
        println!(
            "{:>3} {:<26} {:<10} {:<8} {:<8} {:<8}",
            i + 1,
            inst.question_id,
            cat_short,
            if prior_at_10 { "✓" } else { "✗" },
            if morph_at_10 { "✓" } else { "✗" },
            result
        );
    }

    let n = work.len();
    println!();
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("MorphBM25 smoke — pure lexical, no SCA, no PRF, no LLM");
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("  total:                 {}", n);
    println!("  prior R@10:            {}/{}", prior_hit_at_10, n);
    println!("  MorphBM25 R@10:        {}/{}", morph_hit_at_10, n);
    println!("  Δ R@10:                {:+} (lifted={} hurt={})", (morph_hit_at_10 as i32 - prior_hit_at_10 as i32), lifted, hurt);
    println!();
    println!("  prior R@50:            {}/{}", prior_hit_at_50, n);
    println!("  MorphBM25 R@50:        {}/{}", morph_hit_at_50, n);

    // Suppress unused warnings cleanly
    let _ = (Path::new("/"),);
    Ok(())
}

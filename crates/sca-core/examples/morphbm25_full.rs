//! Full LongMemEval _s_cleaned run with **pure MorphBM25** as the only
//! retrieval engine. No SCA, no PRF, no LLM. Measures whether the +7/+0
//! smoke-test result generalises across all 470 answerable instances.
//!
//! Compares against the prior `#3+PRF` baseline (R@10=83.6%, R@50=91.9%).
//!
//! Run:
//!   cargo run --release -p sca-core --example morphbm25_full \
//!     --features static-embed
//!
//! Outputs:
//!   benchmark/longmemeval/results_morphbm25_all.jsonl

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Both overridable via env vars (DATASET=path, OUT=path) so the same
// harness binary works on _s_cleaned, _m_cleaned, or any future split
// without recompiling.
const DEFAULT_DATASET: &str = "benchmark/longmemeval/data/longmemeval_s_cleaned.json";
const DEFAULT_OUT: &str = "benchmark/longmemeval/results_morphbm25_all.jsonl";

const K_VALUES: &[usize] = &[3, 5, 10, 20, 50, 100];
const TOP_K_FETCH: usize = 100;

const BM25_K1: f32 = 1.2;
const BM25_B: f32 = 0.75;
const ANDPAIR_BONUS: f32 = 1.0;
const MAX_PAIR_DF: usize = 10;
const SUMMARY_K: usize = 20;
const SUMMARY_MIN: usize = 5;

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
    STOPWORDS.iter().any(|w| *w == s)
}

fn tokenize(text: &str) -> Vec<String> {
    text.chars()
        .map(|c| if c.is_ascii_alphabetic() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.len() >= 3 && !is_stop(w))
        .map(|s| s.to_string())
        .collect()
}

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
    tf: HashMap<String, HashMap<String, u32>>,
    dl: HashMap<String, usize>,
    idf: HashMap<String, f32>,
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
        Self { k1: BM25_K1, b: BM25_B, n, tf, dl, idf, inv, avgdl }
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
                for d in docs { s.insert(d.clone()); }
            }
        }
        let d = if !s.is_empty() {
            s.len()
        } else {
            return *self.idf.get(q_term).unwrap_or(&0.0);
        };
        if d == 0 { return 0.0; }
        ((self.n as f32 - d as f32 + 0.5) / (d as f32 + 0.5) + 1.0).ln()
    }

    fn morph_docs(&self, q_term: &str) -> HashSet<String> {
        let variants = morph_variants(q_term);
        let mut s: HashSet<String> = HashSet::new();
        for v in &variants {
            if let Some(docs) = self.inv.get(v) {
                for d in docs { s.insert(d.clone()); }
            }
        }
        s
    }

    fn search(&self, query: &str, top_k: usize) -> Vec<(String, f32)> {
        let q_toks = tokenize(query);
        if q_toks.is_empty() { return Vec::new(); }
        let mut scores: HashMap<String, f32> = HashMap::new();
        for t in &q_toks {
            let idf = self.morph_idf(t);
            if idf <= 0.0 { continue; }
            for did in self.morph_docs(t) {
                let tf = self.morph_tf(&did, t) as f32;
                if tf == 0.0 { continue; }
                let dl = *self.dl.get(&did).unwrap_or(&0) as f32;
                let denom = tf + self.k1 * (1.0 - self.b + self.b * dl / self.avgdl);
                if denom > 0.0 {
                    *scores.entry(did).or_insert(0.0) += idf * tf * (self.k1 + 1.0) / denom;
                }
            }
        }
        let rare: Vec<(String, HashSet<String>)> = q_toks
            .iter()
            .map(|t| (t.clone(), self.morph_docs(t)))
            .filter(|(_, docs)| !docs.is_empty() && docs.len() <= MAX_PAIR_DF)
            .collect();
        for i in 0..rare.len() {
            for j in (i + 1)..rare.len() {
                let inter: HashSet<&String> = rare[i].1.intersection(&rare[j].1).collect();
                if inter.len() <= 3 {
                    let b_val = ANDPAIR_BONUS * (self.morph_idf(&rare[i].0) + self.morph_idf(&rare[j].0));
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

#[derive(Debug, Serialize)]
struct ResultRow {
    question_id: String,
    question_type: String,
    is_abstention: bool,
    hits: BTreeMap<String, bool>,
    top_doc_ids: Vec<String>,
    elapsed_ms: u128,
}

fn tokenize_for_summary(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| {
            let lc = w.to_lowercase();
            if lc.len() >= 3 && lc.chars().any(|c| c.is_alphabetic()) { Some(lc) } else { None }
        })
        .collect()
}

fn session_summary_terms(inst: &Instance, k_terms: usize, min_turns: usize) -> Vec<String> {
    let n_sessions = inst.haystack_sessions.len();
    if n_sessions == 0 { return Vec::new(); }
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
        for t in unique { *df.entry(t).or_insert(0) += 1; }
    }
    let n = session_tokens.len() as f32;
    let mut out = Vec::with_capacity(n_sessions);
    for (s_idx, toks) in session_tokens.iter().enumerate() {
        let n_turns = inst.haystack_sessions[s_idx].len();
        if n_turns < min_turns { out.push(String::new()); continue; }
        let mut tf: HashMap<&str, usize> = HashMap::new();
        for t in toks { *tf.entry(t.as_str()).or_insert(0) += 1; }
        let mut scored: Vec<(&str, f32)> = tf.iter()
            .map(|(term, count)| {
                let df_t = *df.get(term).unwrap_or(&1) as f32;
                let idf = (n / df_t).ln().max(0.0);
                (*term, (*count as f32) * idf)
            }).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k_terms);
        out.push(scored.into_iter().map(|(t, _)| t).collect::<Vec<_>>().join(" "));
    }
    out
}

fn build_corpus(inst: &Instance) -> (Vec<(String, String)>, HashSet<String>) {
    let mut docs: Vec<(String, String)> = Vec::new();
    let mut answer_doc_ids: HashSet<String> = HashSet::new();
    let summaries = session_summary_terms(inst, SUMMARY_K, SUMMARY_MIN);
    for (s_idx, session) in inst.haystack_sessions.iter().enumerate() {
        let session_id = inst.haystack_session_ids.get(s_idx).cloned()
            .unwrap_or_else(|| format!("session_{}", s_idx));
        let session_date = inst.haystack_dates.get(s_idx).cloned().unwrap_or_default();
        let mut session_has_answer = false;
        for (t_idx, turn) in session.iter().enumerate() {
            let role = turn.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = turn.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if content.is_empty() { continue; }
            let has_answer = turn.get("has_answer").and_then(|v| v.as_bool()).unwrap_or(false);
            let body = if session_date.is_empty() {
                format!("{}: {}", role, content)
            } else {
                format!("[Session date: {}] {}: {}", session_date, role, content)
            };
            let doc_id = format!("{}#turn_{}", session_id, t_idx);
            if has_answer { answer_doc_ids.insert(doc_id.clone()); session_has_answer = true; }
            docs.push((doc_id, body));
        }
        if let Some(terms) = summaries.get(s_idx) {
            if !terms.is_empty() {
                let summary_doc_id = format!("{}#summary", session_id);
                let summary_body = if session_date.is_empty() {
                    format!("Session terms: {}", terms)
                } else {
                    format!("[Session date: {}] Session terms: {}", session_date, terms)
                };
                if session_has_answer { answer_doc_ids.insert(summary_doc_id.clone()); }
                docs.push((summary_doc_id, summary_body));
            }
        }
    }
    (docs, answer_doc_ids)
}

fn print_summary(rows: &[ResultRow]) {
    println!();
    println!("════════════════════════════════════════════════════════════════════════════════");
    println!("MorphBM25 (pure) — full 470, no SCA, no PRF, no LLM");
    println!("════════════════════════════════════════════════════════════════════════════════");
    let answerable: Vec<&ResultRow> = rows.iter().filter(|r| !r.is_abstention).collect();
    let n_a = answerable.len();
    let n_abs = rows.len() - n_a;
    println!("Total: {} ({} answerable, {} abstention)", rows.len(), n_a, n_abs);
    println!();
    println!("Overall R@K (answerable, n={}):", n_a);
    for &k in K_VALUES {
        let key = format!("k{}", k);
        let hits = answerable.iter().filter(|r| *r.hits.get(&key).unwrap_or(&false)).count();
        let pct = (hits as f32 / n_a.max(1) as f32) * 100.0;
        println!("  R@{:<3} = {:>5.1}%  ({}/{})", k, pct, hits, n_a);
    }
    println!();
    println!("Per-category R@K:");
    let mut by_cat: BTreeMap<String, Vec<&ResultRow>> = BTreeMap::new();
    for r in &answerable { by_cat.entry(r.question_type.clone()).or_default().push(*r); }
    println!("  {:<28} {:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "category", "n", "R@3", "R@5", "R@10", "R@20", "R@50", "R@100");
    println!("  {:<28} {:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "----------------------------", "-----", "-------", "-------",
        "-------", "-------", "-------", "-------");
    for (cat, rs) in &by_cat {
        let n = rs.len();
        let pct = |key: &str| -> f32 {
            let h = rs.iter().filter(|r| *r.hits.get(key).unwrap_or(&false)).count();
            (h as f32 / n.max(1) as f32) * 100.0
        };
        println!("  {:<28} {:>5} {:>7.1}% {:>7.1}% {:>7.1}% {:>7.1}% {:>7.1}% {:>7.1}%",
            cat, n, pct("k3"), pct("k5"), pct("k10"), pct("k20"), pct("k50"), pct("k100"));
    }
    let mut times: Vec<u128> = rows.iter().map(|r| r.elapsed_ms).collect();
    times.sort_unstable();
    let total: u128 = times.iter().sum();
    let p50 = times.get(times.len() / 2).copied().unwrap_or(0);
    let p95 = times.get((times.len() as f32 * 0.95) as usize).copied().unwrap_or(0);
    println!();
    println!("Latency (build+search per instance): total={}ms p50={}ms p95={}ms", total, p50, p95);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dataset: String = std::env::var("DATASET").unwrap_or_else(|_| DEFAULT_DATASET.to_string());
    let out_path: String = std::env::var("OUT").unwrap_or_else(|_| DEFAULT_OUT.to_string());

    println!("[morph-full] dataset: {}", dataset);
    println!("[morph-full] output:  {}", out_path);

    let raw = fs::read_to_string(&dataset)?;
    let instances: Vec<Instance> = serde_json::from_str(&raw)?;
    println!("[morph-full] {} instances loaded\n", instances.len());

    let _ = fs::remove_file(&out_path);
    let _ = fs::create_dir_all(PathBuf::from(&out_path).parent().unwrap_or(std::path::Path::new(".")));
    let mut out = BufWriter::new(File::create(&out_path)?);

    let mut rows: Vec<ResultRow> = Vec::new();
    let total = instances.len();
    let t_overall = Instant::now();

    for (idx, inst) in instances.iter().enumerate() {
        let start = Instant::now();
        let (docs, answer_doc_ids) = build_corpus(inst);
        let bm = MorphBM25::build(&docs);
        let results = bm.search(&inst.question, TOP_K_FETCH);
        let elapsed_ms = start.elapsed().as_millis();

        let top_doc_ids: Vec<String> = results.iter().map(|(d, _)| d.clone()).collect();
        let mut hits: BTreeMap<String, bool> = BTreeMap::new();
        for &k in K_VALUES {
            let window: HashSet<&String> = top_doc_ids.iter().take(k).collect();
            let hit = answer_doc_ids.iter().any(|aid| window.contains(aid));
            hits.insert(format!("k{}", k), hit);
        }

        // Abstention id convention used by harness — "_abs" suffix on question_id
        let is_abstention = inst.question_id.ends_with("_abs");

        let row = ResultRow {
            question_id: inst.question_id.clone(),
            question_type: inst.question_type.clone(),
            is_abstention,
            hits,
            top_doc_ids,
            elapsed_ms,
        };
        writeln!(out, "{}", serde_json::to_string(&row)?)?;
        rows.push(row);

        if (idx + 1) % 50 == 0 || idx + 1 == total {
            let elapsed = t_overall.elapsed().as_secs();
            let pace = elapsed as f32 / (idx + 1) as f32;
            let eta = (pace * (total - idx - 1) as f32) as u64;
            println!("[morph-full] {}/{} done — {}s elapsed, ~{}s remaining",
                idx + 1, total, elapsed, eta);
        }
    }
    out.flush()?;
    print_summary(&rows);
    println!("\n[morph-full] per-instance log: {}", out_path);
    Ok(())
}

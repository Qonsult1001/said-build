//! Build a LongMemEval-format input JSON for the upstream `run_generation.py`,
//! using SAID's MorphBM25 + optional Phase Q rerank as the retriever.
//!
//! Output schema (matches `--in_file` shape that upstream expects):
//!   list of original dataset entries, each with an extra
//!   `retrieval_results.ranked_items` field of the form:
//!     [{"corpus_id": "<session_or_turn_id>", "text": "<body>"}, ...]
//!
//! The corpus_id format follows upstream's convention. SAID indexes at TURN
//! granularity (`<session_id>#turn_<i>`) but upstream's `flat-session` mode
//! collapses ids back to sessions; we emit at SESSION granularity here, in
//! retrieval rank order, deduped to unique sessions.
//!
//! Run:
//!   GROQ_API_KEY ignored — this binary makes NO LLM calls.
//!   ./target/release/examples/qa_smoke.exe
//!     OUT=benchmark/longmemeval/lme_input_with_retrieval.json
//!     SLICE_ALL=1   # default; for a slice set SLICE=<qids.json>

use std::collections::{HashMap, HashSet};
use std::fs;

use serde::Deserialize;
use serde_json::{json, Value};

const DATASET: &str = "benchmark/longmemeval/data/longmemeval_s_cleaned.json";
const RERANK:  &str = "benchmark/longmemeval/results_morphbm25_rerank.jsonl";
const MORPH:   &str = "benchmark/longmemeval/results_morphbm25_all.jsonl";
const OUT:     &str = "benchmark/longmemeval/lme_input_with_retrieval.json";
const TOP_K:   usize = 50;

#[derive(Debug, Deserialize)]
struct RerankRow {
    question_id: String,
    #[allow(dead_code)]
    question_type: String,
    #[serde(default)]
    ranked_ids: Vec<String>,
}

fn parent_session_id(doc_id: &str) -> &str {
    doc_id.split('#').next().unwrap_or(doc_id)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dataset_raw = fs::read_to_string(DATASET)?;
    let mut dataset: Vec<Value> = serde_json::from_str(&dataset_raw)?;

    let optional_slice: Option<HashSet<String>> = match std::env::var("SLICE") {
        Ok(p) => Some(serde_json::from_str::<Vec<String>>(&fs::read_to_string(p)?)?
            .into_iter().collect()),
        Err(_) => None,
    };
    println!("[shim] dataset: {} entries", dataset.len());
    if let Some(s) = &optional_slice {
        println!("[shim] slice filter active: {} qids", s.len());
    }

    let rerank_raw = fs::read_to_string(RERANK)?;
    let mut rerank_by_qid: HashMap<String, RerankRow> = HashMap::new();
    for line in rerank_raw.lines() {
        let r: RerankRow = serde_json::from_str(line)?;
        rerank_by_qid.insert(r.question_id.clone(), r);
    }

    let morph_raw = fs::read_to_string(MORPH)?;
    let mut morph_top: HashMap<String, Vec<String>> = HashMap::new();
    for line in morph_raw.lines() {
        let v: Value = serde_json::from_str(line)?;
        let qid = v.get("question_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if let Some(arr) = v.get("top_doc_ids").and_then(|x| x.as_array()) {
            morph_top.insert(qid, arr.iter().filter_map(|x| x.as_str().map(String::from)).collect());
        }
    }
    println!("[shim] rerank rows: {}, morphbm25 rows: {}", rerank_by_qid.len(), morph_top.len());

    let mut written = 0usize;
    let mut filtered_dataset: Vec<Value> = Vec::new();
    for entry in dataset.iter_mut() {
        let qid = entry.get("question_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if let Some(s) = &optional_slice {
            if !s.contains(&qid) { continue; }
        }

        // Pick top-K turn ids: prefer rerank, else MorphBM25.
        let r = rerank_by_qid.get(&qid);
        let top_ids: Vec<String> = match r.and_then(|r| (!r.ranked_ids.is_empty()).then(|| r.ranked_ids.clone())) {
            Some(ids) => ids,
            None => morph_top.get(&qid).cloned().unwrap_or_default(),
        };

        // The set of session ids actually present in this entry's haystack —
        // upstream's flat-session mode KeyErrors on anything outside this set.
        // (SAID's index occasionally emits typo'd ids; safer to drop them
        //  than to crash the whole 470 run.)
        let valid_sids: HashSet<String> = entry.get("haystack_session_ids")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();

        // Roll up to unique parent sessions (preserve rank order).
        let mut seen: HashSet<String> = HashSet::new();
        let mut session_order: Vec<String> = Vec::new();
        let mut dropped_unknown = 0usize;
        for id in &top_ids {
            let sid = parent_session_id(id).to_string();
            // Upstream auto-converts noans_ → answer_, so accept either form.
            let sid_norm = sid.replace("noans_", "answer_");
            if !valid_sids.contains(&sid) && !valid_sids.contains(&sid_norm) {
                dropped_unknown += 1;
                continue;
            }
            if seen.insert(sid.clone()) {
                session_order.push(sid);
                if session_order.len() >= TOP_K { break; }
            }
        }
        if dropped_unknown > 0 {
            eprintln!("[shim] qid={} dropped {} unknown ids", qid, dropped_unknown);
        }

        // Emit `retrieval_results.ranked_items` — text is empty since upstream
        // `flat-session` mode looks up the session body from `haystack_sessions`
        // by corpus_id; the `text` field is only consulted in expansion modes.
        let ranked_items: Vec<Value> = session_order.iter().map(|sid| {
            json!({ "corpus_id": sid, "text": "" })
        }).collect();

        if let Some(obj) = entry.as_object_mut() {
            obj.insert("retrieval_results".into(), json!({ "ranked_items": ranked_items }));
        }
        filtered_dataset.push(entry.clone());
        written += 1;
    }

    let out_path: String = std::env::var("OUT").unwrap_or_else(|_| OUT.to_string());
    fs::write(&out_path, serde_json::to_string(&filtered_dataset)?)?;
    println!("[shim] wrote {} entries to {}", written, out_path);
    Ok(())
}

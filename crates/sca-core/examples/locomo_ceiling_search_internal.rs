//! Alternative ceiling diagnostic using `search_internal` directly
//! (bypasses SaidFile::recall's custom grep-rerank + injection layer,
//! goes straight to the MTEB `recall::search_full_scoped` pipeline).
//!
//! If this ceiling is HIGHER than the `recall()` ceiling, the custom
//! layer is hurting us on conversational data. If it's LOWER, our
//! injection logic is actually helping.
//!
//! Run:
//!   cargo run --release -p sca-core --example locomo_ceiling_search_internal \
//!     --features "static-embed" -- --conv all

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use sca_core::engine::ScaEngine;
use sca_core::recall::{search_full, PassageEngine};

#[derive(Debug, Deserialize)]
struct Sample {
    sample_id: String,
    conversation: Value,
    qa: Vec<QaItem>,
}

#[derive(Debug, Deserialize)]
struct QaItem {
    question: String,
    #[serde(default)]
    evidence: Option<Value>,
    category: u8,
    #[allow(dead_code)]
    answer: Option<Value>,
}

struct Turn { dia_id: String, speaker: String, text: String, session_date: String }

fn extract_turns(conversation: &Value) -> Vec<Turn> {
    let Some(obj) = conversation.as_object() else { return Vec::new() };
    let mut turns = Vec::new();
    for (k, v) in obj {
        if !k.starts_with("session_") || k.ends_with("date_time") { continue; }
        let date_key = format!("{}_date_time", k);
        let session_date = obj.get(&date_key).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let Some(arr) = v.as_array() else { continue };
        for t in arr {
            let dia_id = t.get("dia_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let speaker = t.get("speaker").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = t.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if !dia_id.is_empty() && !text.is_empty() {
                turns.push(Turn { dia_id, speaker, text, session_date: session_date.clone() });
            }
        }
    }
    turns
}

fn parse_evidence(ev: &Option<Value>) -> Vec<String> {
    match ev {
        Some(Value::Array(arr)) => arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn find_encoder() -> Result<&'static str, String> {
    for p in ["said-lam-static", "SAID-LAM-private/said-lam-static", "../SAID-LAM-private/said-lam-static", "../../SAID-LAM-private/said-lam-static"] {
        if Path::new(p).exists() { return Ok(p); }
    }
    Err("encoder not found".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut conv_filter: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--conv" {
            if i + 1 < args.len() && args[i + 1] != "all" {
                conv_filter = Some(args[i + 1].parse()?);
            }
            i += 2;
        } else { i += 1; }
    }

    let encoder_path = find_encoder()?;
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(
        File::open(PathBuf::from("research/locomo/data/locomo10.json"))?))?;

    let ladder: [usize; 9] = [1, 5, 10, 20, 50, 100, 200, 500, 1000];
    let mut hits_at: Vec<u32> = vec![0; ladder.len()];
    let mut total: u32 = 0;
    let mut miss: u32 = 0;

    for (idx, sample) in samples.iter().enumerate() {
        if let Some(f) = conv_filter { if idx != f { continue; } }
        let turns = extract_turns(&sample.conversation);
        if turns.is_empty() { continue; }

        let mut engine = ScaEngine::new();
        engine.load_static_encoder(encoder_path)?;
        engine.core.set_holographic_16view(false, None);

        let mut doc_ids = Vec::new();
        let mut doc_texts = Vec::new();
        for t in &turns {
            let body = if t.session_date.is_empty() {
                format!("{}: {}", t.speaker, t.text)
            } else {
                format!("[{}] {}: {}", t.session_date, t.speaker, t.text)
            };
            doc_ids.push(t.dia_id.clone());
            doc_texts.push(body);
        }
        engine.index_batch(&doc_ids, &doc_texts)?;
        let corpus_lower: Vec<String> = doc_texts.iter().map(|t| t.to_lowercase()).collect();
        let mut passage_engine = PassageEngine::new();

        let mut conv_miss = 0u32;
        let mut conv_total = 0u32;
        for qa in &sample.qa {
            let gold_vec = parse_evidence(&qa.evidence);
            if gold_vec.is_empty() { continue; }
            let gold: HashSet<String> = gold_vec.into_iter().collect();

            let hits = search_full(
                &mut engine,
                Some(&mut passage_engine),
                &qa.question,
                None,
                turns.len(),
                &doc_ids,
                &doc_texts,
                &corpus_lower,
            );

            total += 1;
            conv_total += 1;
            let rank = hits.iter().position(|(d, _)| gold.contains(d)).map(|i| i + 1);
            match rank {
                Some(r) => {
                    for (i, &k) in ladder.iter().enumerate() {
                        if r <= k { hits_at[i] += 1; }
                    }
                }
                None => { miss += 1; conv_miss += 1; }
            }
            let _ = qa.category; // silence unused
        }
        println!("[{}/{}] {} | QA={} miss={}", idx+1, samples.len(), sample.sample_id, conv_total, conv_miss);
    }

    println!("\n════════════════════════════════════════════════════════════════");
    println!("RAW search_full (MTEB path, no recall() custom layer)");
    println!("OVERALL — {} scored, {} missing", total, miss);
    println!("────────────────────────────────────────────────────────────────");
    for (i, &k) in ladder.iter().enumerate() {
        let r = hits_at[i] as f64 / total.max(1) as f64;
        let bar = "█".repeat((r * 40.0) as usize);
        println!("R@{:<5} = {:.4}  ({:>4}/{})  {}", k, r, hits_at[i], total, bar);
    }
    println!("\nCompare to recall() ceiling: 0.8199 (R@100)");
    Ok(())
}

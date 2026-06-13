//! LoCoMo pre-pillar baseline harness.
//!
//! Runs the snap-research/locomo QA benchmark on the CURRENT .said document-
//! retrieval pipeline (no pillars, no dream function, no episodic writer).
//! This is the honest "before pillars" number — it measures how far our
//! document-shaped pipeline gets on a conversation-memory task.
//!
//! Metric: Recall@k (k = 1, 5, 10, 20) on gold `evidence` dialog IDs. A hit =
//! any of the top-k retrieved passages maps to a gold dia_id.
//!
//! Does NOT measure generation quality — that would need an LLM in the loop.
//! Recall@k is the retrieval-only proxy. A pillar-based agent is expected to
//! dominate this on temporal/causal questions (categories 2, 3, 5).
//!
//! Input:  research/locomo/data/locomo10.json  (10 conversations, ~1986 QA)
//!
//! Run:
//!   cargo run --release -p sca-core --example locomo_baseline \
//!     --features "static-embed" -- --conv all

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use sca_core::engine::ScaEngine;

#[path = "shared/pipeline.rs"]
mod pipeline;
use pipeline::{build_passage_engine, search_full};

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

struct Turn {
    dia_id: String,
    speaker: String,
    text: String,
}

fn extract_turns(conversation: &Value) -> Vec<Turn> {
    let Some(obj) = conversation.as_object() else {
        return Vec::new();
    };
    let mut turns = Vec::new();
    for (k, v) in obj {
        if !k.starts_with("session_") || k.ends_with("date_time") {
            continue;
        }
        let Some(arr) = v.as_array() else { continue };
        for t in arr {
            let dia_id = t.get("dia_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let speaker = t.get("speaker").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = t.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if dia_id.is_empty() || text.is_empty() {
                continue;
            }
            turns.push(Turn { dia_id, speaker, text });
        }
    }
    turns
}

fn parse_evidence(ev: &Option<Value>) -> Vec<String> {
    match ev {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn hit_at_k(
    ranked: &[(String, f32)],
    p2dia: &HashMap<String, String>,
    gold: &HashSet<String>,
    k: usize,
) -> bool {
    for (pid, _) in ranked.iter().take(k) {
        // ranked entries come back with the INDEXED id; PassageEngine indexes
        // passage_ids, so `pid` here is a passage id → map via p2dia.
        // But when we index at the DOC level (turns), pid == dia_id already.
        if let Some(dia) = p2dia.get(pid) {
            if gold.contains(dia) {
                return true;
            }
        } else if gold.contains(pid) {
            // Doc-level hit: pid is the dia_id itself.
            return true;
        }
    }
    false
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
        } else {
            i += 1;
        }
    }

    let encoder_path = find_encoder()?;
    println!("[encoder] {}", encoder_path);

    let dataset_path = PathBuf::from("research/locomo/data/locomo10.json");
    let f = File::open(&dataset_path)
        .map_err(|e| format!("cannot open {}: {}", dataset_path.display(), e))?;
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(f))?;

    println!("LoCoMo baseline — {} conversations", samples.len());
    println!("Pipeline: sca-core search_full (document-retrieval, pre-pillar)");
    println!("Metric: Recall@k on gold evidence dia_ids\n");

    let ks = [1usize, 5, 10, 20];
    let mut overall: HashMap<usize, (u32, u32)> = HashMap::new();
    let mut by_cat: HashMap<u8, HashMap<usize, (u32, u32)>> = HashMap::new();

    for (idx, sample) in samples.iter().enumerate() {
        if let Some(f) = conv_filter {
            if idx != f {
                continue;
            }
        }

        let turns = extract_turns(&sample.conversation);
        if turns.is_empty() {
            continue;
        }

        let doc_ids: Vec<String> = turns.iter().map(|t| t.dia_id.clone()).collect();
        let doc_texts: Vec<String> = turns
            .iter()
            .map(|t| format!("{}: {}", t.speaker, t.text))
            .collect();

        // Build doc engine (matches mteb_rust.rs pattern).
        let mut doc_engine = ScaEngine::new();
        doc_engine.load_static_encoder(encoder_path)?;
        doc_engine.core.set_holographic_16view(false, None);
        doc_engine
            .index_batch(&doc_ids, &doc_texts)
            .map_err(|e| format!("doc index_batch: {}", e))?;

        // Passage engine — for LoCoMo most turns are short, so passages ≈ docs.
        // Keeping it for parity with how search_full expects to dispatch.
        let (mut passage_engine, p2dia) =
            build_passage_engine(&doc_ids, &doc_texts, encoder_path)?;

        // Lower-cased texts for recall_fused injection.
        let corpus_texts_lower: Vec<String> = doc_texts.iter().map(|t| t.to_lowercase()).collect();

        use std::io::Write;
        print!(
            "[{}/{}] {} | turns={} qa={} ... ",
            idx + 1,
            samples.len(),
            sample.sample_id,
            turns.len(),
            sample.qa.len()
        );
        std::io::stdout().flush().ok();

        let mut local_hits_at_10: u32 = 0;
        let mut local_total: u32 = 0;

        for (qi, qa) in sample.qa.iter().enumerate() {
            let gold_vec = parse_evidence(&qa.evidence);
            if gold_vec.is_empty() {
                continue;
            }
            let gold: HashSet<String> = gold_vec.into_iter().collect();

            let qid = format!("{}_{}", sample.sample_id, qi);
            let ranked = search_full(
                &mut doc_engine,
                Some(&mut passage_engine),
                &qa.question,
                Some(qid.as_str()),
                20,
                &doc_ids,
                &doc_texts,
                &corpus_texts_lower,
            );

            local_total += 1;
            for &k in &ks {
                let hit = hit_at_k(&ranked, &p2dia, &gold, k);
                let e = overall.entry(k).or_insert((0, 0));
                e.1 += 1;
                if hit {
                    e.0 += 1;
                    if k == 10 {
                        local_hits_at_10 += 1;
                    }
                }
                let ce = by_cat.entry(qa.category).or_default().entry(k).or_insert((0, 0));
                ce.1 += 1;
                if hit {
                    ce.0 += 1;
                }
            }
        }

        let r10 = local_hits_at_10 as f64 / local_total.max(1) as f64;
        println!("R@10 = {:.4}", r10);
    }

    println!("\n════════════════════════════════════════════════════════════════");
    println!("OVERALL");
    println!("────────────────────────────────────────────────────────────────");
    for &k in &ks {
        let (h, t) = overall.get(&k).copied().unwrap_or((0, 0));
        let r = h as f64 / t.max(1) as f64;
        println!("Recall@{:<3} = {:.4}  ({} / {})", k, r, h, t);
    }

    println!("\nBy category:");
    let mut cats: Vec<&u8> = by_cat.keys().collect();
    cats.sort();
    for cat in cats {
        print!("  cat {}: ", cat);
        for &k in &ks {
            let (h, t) = by_cat[cat].get(&k).copied().unwrap_or((0, 0));
            let r = h as f64 / t.max(1) as f64;
            print!("R@{}={:.3} ", k, r);
        }
        println!();
    }

    Ok(())
}

//! Trace why specific LoCoMo QA whose gold evidence is NEVER returned by
//! `SaidFile::recall` fail. Compares recall() vs raw SCA query() on the
//! same corpus for the same question, prints where gold sits in each.
//!
//! Run:
//!   cargo run --release -p sca-core --example locomo_miss_trace \
//!     --features "static-embed" -- --conv 0 --limit 5

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

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
    #[allow(dead_code)]
    category: u8,
    #[serde(default)]
    answer: Option<Value>,
}

struct Turn {
    dia_id: String,
    speaker: String,
    text: String,
    session_date: String,
}

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
            if dia_id.is_empty() || text.is_empty() { continue; }
            turns.push(Turn { dia_id, speaker, text, session_date: session_date.clone() });
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
    let paths: [&'static str; 4] = [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "../../SAID-LAM-private/said-lam-static",
    ];
    for p in paths { if Path::new(p).exists() { return Ok(p); } }
    Err("encoder not found".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut conv_idx: usize = 0;
    let mut limit: usize = 5;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--conv" => { if i + 1 < args.len() { conv_idx = args[i + 1].parse()?; } i += 2; }
            "--limit" => { if i + 1 < args.len() { limit = args[i + 1].parse()?; } i += 2; }
            _ => i += 1,
        }
    }

    let encoder_path = find_encoder()?;
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(
        File::open(PathBuf::from("research/locomo/data/locomo10.json"))?))?;
    let sample = &samples[conv_idx];
    let turns = extract_turns(&sample.conversation);
    println!("[conv {}] {} turns, {} QA", sample.sample_id, turns.len(), sample.qa.len());

    let brain_path = format!("tmp_miss_trace_{}.said", sample.sample_id);
    let _ = std::fs::remove_file(&brain_path);
    let mut sf = SaidFile::create(&brain_path);
    sf.engine.load_static_encoder(encoder_path)?;
    sf.engine.core.set_holographic_16view(false, None);

    // Keep a gold-content map to print what the gold turn actually says.
    let mut dia_content: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for t in &turns {
        let body = if t.session_date.is_empty() {
            format!("{}: {}", t.speaker, t.text)
        } else {
            format!("[{}] {}: {}", t.session_date, t.speaker, t.text)
        };
        sf.remember_with_pillar(Some(&t.dia_id), &body, None, Pillar::Episodic, vec![]);
        dia_content.insert(t.dia_id.clone(), body);
    }
    sf.build_index()?;

    let mut found_misses = 0usize;
    for (qi, qa) in sample.qa.iter().enumerate() {
        let gold_vec = parse_evidence(&qa.evidence);
        if gold_vec.is_empty() { continue; }
        let gold: HashSet<String> = gold_vec.iter().cloned().collect();

        // Full recall (same as F1 harness).
        let hits = sf.recall(&qa.question, 10_000);
        let ranked: Vec<String> = hits.iter().map(|h| h.doc_id.clone()).collect();

        let rank = ranked.iter().position(|d| gold.contains(d)).map(|i| i + 1);

        if rank.is_none() {
            // This is a MISS — gold not in ranked list at all.
            println!("\n══════════════════════════════════════════════════════════════");
            println!("MISS qa#{}  Q: {}", qi, qa.question);
            println!("GOLD: {:?}", gold_vec);
            for gid in &gold_vec {
                if let Some(c) = dia_content.get(gid) {
                    let preview: String = c.chars().take(150).collect();
                    println!("  [{}] {}", gid, preview);
                }
            }
            println!("recall() returned {} hits (not in list)", ranked.len());
            println!("Top-10 that were returned:");
            for (i, did) in ranked.iter().take(10).enumerate() {
                let preview: String = dia_content.get(did)
                    .map(|c| c.chars().take(100).collect())
                    .unwrap_or_default();
                println!("  #{} {} — {}", i + 1, did, preview);
            }

            // Also run RAW SCA via grep (no recall pipeline) as sanity check.
            let grep_hits = sf.grep(&qa.question, 10);
            println!("grep() top-10:");
            for (i, r) in grep_hits.iter().enumerate() {
                let marker = if gold.contains(&r.doc_id) { " ← GOLD!" } else { "" };
                println!("  #{} {} score={:.3}{}", i + 1, r.doc_id, r.score, marker);
            }

            found_misses += 1;
            if found_misses >= limit { break; }
        }
    }

    let _ = std::fs::remove_file(&brain_path);
    let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    println!("\ntotal misses examined: {}", found_misses);
    Ok(())
}

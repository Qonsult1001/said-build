//! Compare RAW search_full (MTEB path) vs SaidFile::recall (with grep +
//! injection layers) on LoCoMo MISS queries. Tells us where the degrade
//! happens — if search_full finds the gold turn but recall() loses it,
//! the custom layers are the bug. If search_full ALSO misses, then the
//! SCA fingerprint itself is the problem on short conversational turns.

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use sca_core::engine::ScaEngine;
use sca_core::frames::Pillar;
use sca_core::recall::{search_full, PassageEngine};
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
    let mut conv_idx = 0usize;
    let mut qa_limit = 3usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--conv" => { if i+1 < args.len() { conv_idx = args[i+1].parse()?; } i += 2; }
            "--limit" => { if i+1 < args.len() { qa_limit = args[i+1].parse()?; } i += 2; }
            _ => i += 1,
        }
    }

    let encoder_path = find_encoder()?;
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(
        File::open(PathBuf::from("research/locomo/data/locomo10.json"))?))?;
    let sample = &samples[conv_idx];
    let turns = extract_turns(&sample.conversation);

    // Build SaidFile (for recall()) AND separately build ScaEngine
    // (for raw search_full — the MTEB path).
    let brain_path = format!("tmp_sca_trace_{}.said", sample.sample_id);
    let _ = std::fs::remove_file(&brain_path);
    let mut sf = SaidFile::create(&brain_path);
    sf.engine.load_static_encoder(encoder_path)?;
    sf.engine.core.set_holographic_16view(false, None);

    // Raw engine mirrors MTEB harness setup exactly.
    let mut raw_engine = ScaEngine::new();
    raw_engine.load_static_encoder(encoder_path)?;
    raw_engine.core.set_holographic_16view(false, None);

    let mut doc_ids = Vec::new();
    let mut doc_texts = Vec::new();
    for t in &turns {
        let body = if t.session_date.is_empty() {
            format!("{}: {}", t.speaker, t.text)
        } else {
            format!("[{}] {}: {}", t.session_date, t.speaker, t.text)
        };
        doc_ids.push(t.dia_id.clone());
        doc_texts.push(body.clone());
        sf.remember_with_pillar(Some(&t.dia_id), &body, None, Pillar::Episodic, vec![]);
    }
    sf.build_index()?;
    raw_engine.index_batch(&doc_ids, &doc_texts)?;

    let corpus_lower: Vec<String> = doc_texts.iter().map(|t| t.to_lowercase()).collect();
    let mut passage_engine = PassageEngine::new();

    let mut found = 0usize;
    for qa in sample.qa.iter() {
        let gold_vec = parse_evidence(&qa.evidence);
        if gold_vec.is_empty() { continue; }
        let gold: HashSet<String> = gold_vec.iter().cloned().collect();

        let recall_hits = sf.recall(&qa.question, 10_000);
        let recall_has = recall_hits.iter().any(|h| gold.contains(&h.doc_id));

        // Skip if recall ALREADY finds it — we want misses.
        if recall_has { continue; }

        // Now try raw MTEB path.
        let raw_hits = search_full(
            &mut raw_engine,
            Some(&mut passage_engine),
            &qa.question,
            None,
            100,
            &doc_ids,
            &doc_texts,
            &corpus_lower,
        );
        let raw_rank = raw_hits.iter().position(|(d, _)| gold.contains(d)).map(|i| i + 1);

        println!("\n══════════════════════════════════════════════════════════════");
        println!("Q: {}", qa.question);
        println!("GOLD: {:?}", gold_vec);
        for gid in &gold_vec {
            if let Some(pos) = doc_ids.iter().position(|d| d == gid) {
                let preview: String = doc_texts[pos].chars().take(140).collect();
                println!("  [{}] {}", gid, preview);
            }
        }
        println!("SaidFile::recall: MISS (not in ranked list)");
        match raw_rank {
            Some(r) => println!("Raw search_full:  rank #{} ✓ (MTEB path finds it)", r),
            None    => println!("Raw search_full:  MISS too"),
        }
        // Show raw top-5 so we see what the engine actually thinks ranks highest.
        println!("Raw top-5:");
        for (i, (did, score)) in raw_hits.iter().take(5).enumerate() {
            let marker = if gold.contains(did) { " ← GOLD" } else { "" };
            let preview: String = doc_ids.iter().position(|d| d == did)
                .map(|pos| doc_texts[pos].chars().take(90).collect::<String>())
                .unwrap_or_default();
            println!("  #{} {} score={:.3}{}  {}", i+1, did, score, marker, preview);
        }

        found += 1;
        if found >= qa_limit { break; }
    }

    let _ = std::fs::remove_file(&brain_path);
    let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    Ok(())
}

//! Real-world recall probe — simulates a user asking train-of-thought
//! multi-hop questions about their own months of conversation history.
//!
//! Unlike locomo_f1.rs which feeds gold-labelled benchmark questions through
//! an LLM, this example just runs `SaidFile::recall()` and prints the top-10
//! hits with their session dates and a snippet. We eyeball whether a real
//! person would find those results useful.
//!
//! The test uses LoCoMo conv-26 as the corpus (a real multi-session chat
//! between two speakers spread across ~19 sessions / 3+ months) and a mix
//! of hand-written queries that resemble what a real user might ask:
//!   - "what did Alice decide about X"  → multi-hop, entity + topic
//!   - "when did I last talk about Y"   → temporal, topic
//!   - "what's the status of Z"         → aggregation across turns
//!
//! Run:
//!   cargo run --release -p sca-core --example realworld_recall_probe \
//!     --features "static-embed"

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
}

struct Turn {
    dia_id: String,
    speaker: String,
    text: String,
    session_date: String,
}

fn extract_turns(conversation: &Value) -> Vec<Turn> {
    let Some(obj) = conversation.as_object() else { return Vec::new() };
    let mut keys: Vec<String> = obj.keys()
        .filter(|k| k.starts_with("session_") && !k.ends_with("date_time")).cloned().collect();
    keys.sort_by_key(|k| k.trim_start_matches("session_").parse::<u32>().unwrap_or(u32::MAX));
    let mut out = Vec::new();
    for k in keys {
        let date_key = format!("{}_date_time", k);
        let session_date = obj.get(&date_key).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let Some(arr) = obj.get(&k).and_then(|v| v.as_array()) else { continue };
        for t in arr {
            let dia_id = t.get("dia_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let speaker = t.get("speaker").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = t.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let caption = t.get("blip_caption").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if dia_id.is_empty() { continue; }
            let body = if caption.is_empty() { text }
                else if text.is_empty() { format!("[image: {}]", caption) }
                else { format!("{} [image: {}]", text, caption) };
            if body.is_empty() { continue; }
            out.push(Turn { dia_id, speaker, text: body, session_date: session_date.clone() });
        }
    }
    out
}

fn find_encoder() -> Result<&'static str, String> {
    for p in ["said-lam-static", "SAID-LAM-private/said-lam-static",
              "../SAID-LAM-private/said-lam-static", "../../SAID-LAM-private/said-lam-static"] {
        if Path::new(p).exists() { return Ok(p); }
    }
    Err("encoder not found".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let encoder_path = find_encoder()?;
    let dataset_path = PathBuf::from("research/locomo/data/locomo10.json");
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(File::open(&dataset_path)?))?;
    let sample = &samples[0];
    let turns = extract_turns(&sample.conversation);
    println!("[corpus] {} — {} turns across sessions", sample.sample_id, turns.len());

    let brain_path = format!("tmp_realworld_probe_{}.said", sample.sample_id);
    let _ = std::fs::remove_file(&brain_path);
    let mut sf = SaidFile::create(&brain_path);
    sf.engine.load_static_encoder(encoder_path)?;
    sf.engine.core.set_holographic_16view(false, None);

    for t in &turns {
        let body = if t.session_date.is_empty() {
            format!("{}: {}", t.speaker, t.text)
        } else {
            format!("[{}] {}: {}", t.session_date, t.speaker, t.text)
        };
        sf.remember_with_pillar(Some(&t.dia_id), &body, None, Pillar::Episodic, vec![]);
    }
    sf.build_index()?;
    println!("[index] built; corpus ready\n");

    // Queries written WITHOUT peeking at any LoCoMo gold. These are the kind
    // of things a real user with months of chat history might ask. None of
    // them match a LoCoMo benchmark question verbatim.
    let queries = vec![
        "What has Melanie been painting lately?",
        "When did Caroline and Melanie talk about stress?",
        "What kind of stuff do Melanie's kids get up to?",
        "What did Caroline say about her mental health journey?",
        "When was the last camping trip?",
        "What are Caroline's plans for the future?",
        "What does Melanie do with her free time?",
        "How did Caroline feel when she went to the support group?",
        "What art projects have they shared with each other?",
        "Who are the important people in Caroline's life?",
    ];

    for (qi, query) in queries.iter().enumerate() {
        println!("════════════════════════════════════════════════════════════════");
        println!("Q{}: {}", qi + 1, query);
        println!("────────────────────────────────────────────────────────────────");
        let results = sf.recall(query, 100);
        if results.is_empty() {
            println!("  (no results)");
            continue;
        }
        println!("Top 100 total returned: {}", results.len());
        println!("Showing top 20:");
        for (i, r) in results.iter().take(20).enumerate() {
            let snippet: String = r.content.chars().take(120).collect();
            println!("  #{:<3} {} | score={:.3}", i+1, r.doc_id, r.score);
            println!("       {}", snippet.replace('\n', " "));
        }
        println!();
    }

    let _ = std::fs::remove_file(&brain_path);
    let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    Ok(())
}

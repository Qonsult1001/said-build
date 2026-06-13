//! Quick route-classification check — what does SCA classify each of our
//! real-world probe queries as?

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

#[derive(Debug, Deserialize)]
struct Sample { conversation: Value }

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

    let brain_path = "tmp_route_check.said".to_string();
    let _ = std::fs::remove_file(&brain_path);
    let mut sf = SaidFile::create(&brain_path);
    sf.engine.load_static_encoder(encoder_path)?;
    sf.engine.core.set_holographic_16view(false, None);

    let obj = sample.conversation.as_object().unwrap();
    for (k, v) in obj {
        if !k.starts_with("session_") || k.ends_with("date_time") { continue; }
        let Some(arr) = v.as_array() else { continue };
        for t in arr {
            let dia_id = t.get("dia_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let speaker = t.get("speaker").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = t.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if dia_id.is_empty() || text.is_empty() { continue; }
            let body = format!("{}: {}", speaker, text);
            sf.remember_with_pillar(Some(&dia_id), &body, None, Pillar::Episodic, vec![]);
        }
    }
    sf.build_index()?;

    let queries = [
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
        "destress",
        "Melanie running",
        "dinosaur exhibit",
    ];

    println!("{:<50} | route", "query");
    println!("{}", "─".repeat(70));
    for q in queries {
        let route = sf.engine.core.detect_query_type(q);
        println!("{:<50} | {}", q, route);
    }

    let _ = std::fs::remove_file(&brain_path);
    let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    Ok(())
}

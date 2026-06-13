//! Root-cause probe: investigate ALL the "weak" queries from the real-world probe
//! to see where gold turns actually rank. Prior probe was eyeballing — this one
//! has hand-identified gold expectations and reports exact rank.

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

    let brain_path = format!("tmp_diagnose_{}.said", sample.sample_id);
    let _ = std::fs::remove_file(&brain_path);
    let mut sf = SaidFile::create(&brain_path);
    sf.engine.load_static_encoder(encoder_path)?;
    sf.engine.core.set_holographic_16view(false, None);

    // CONTEXTUAL-WINDOW INGEST
    //
    // Dialogue turns are short and answer-shaped: the question lives in the
    // previous speaker's turn, the answer in the current one. Indexing each
    // turn in isolation means "What do Melanie's kids like?" can never find
    // "They loved the dinosaur exhibit" because the word "kids" is only in
    // the question-turn, not the answer-turn.
    //
    // Fix: include the N preceding turns from the same session as context
    // when building the body we index. The turn's own content still appears
    // in the body, so keyword + grep + display all still work as before.
    //
    // This is a harness-side change only — the core .said API is unchanged.
    // If a caller wants contextual retrieval, they build the body; if not,
    // they pass the raw turn. No global rule imposed.
    const WINDOW: usize = 0; // previous turns to include as context (0 = no window, pure BM25 test)

    let obj = sample.conversation.as_object().unwrap();
    let mut n_turns = 0usize;
    let mut keys: Vec<String> = obj.keys()
        .filter(|k| k.starts_with("session_") && !k.ends_with("date_time")).cloned().collect();
    keys.sort_by_key(|k| k.trim_start_matches("session_").parse::<u32>().unwrap_or(u32::MAX));

    for k in keys {
        let date_key = format!("{}_date_time", k);
        let session_date = obj.get(&date_key).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let Some(arr) = obj.get(&k).and_then(|v| v.as_array()) else { continue };

        // Pass 1: collect every turn in this session in order.
        let mut session_turns: Vec<(String, String, String)> = Vec::new(); // (dia_id, speaker, body_text)
        for t in arr {
            let dia_id = t.get("dia_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let speaker = t.get("speaker").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = t.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let caption = t.get("blip_caption").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if dia_id.is_empty() { continue; }
            let body_text = if caption.is_empty() { text }
                else if text.is_empty() { format!("[image: {}]", caption) }
                else { format!("{} [image: {}]", text, caption) };
            if body_text.is_empty() { continue; }
            session_turns.push((dia_id, speaker, body_text));
        }

        // Pass 2: index each turn with a WINDOW-size trailing context of
        // previous turns from this same session. The current turn is
        // ALWAYS last in the body string so its words dominate the
        // fingerprint quantization (the encoder weights first/last tokens
        // most, so "last" keeps current-turn tokens influential).
        for i in 0..session_turns.len() {
            let (dia_id, speaker, body_text) = &session_turns[i];
            let start = i.saturating_sub(WINDOW);

            let mut body = if session_date.is_empty() {
                String::new()
            } else {
                format!("[{}] ", session_date)
            };
            // Preceding context
            for j in start..i {
                let (_, ps, pt) = &session_turns[j];
                body.push_str(&format!("{}: {}\n", ps, pt));
            }
            // Current turn (last — dominates fingerprint)
            body.push_str(&format!("{}: {}", speaker, body_text));

            sf.remember_with_pillar(Some(dia_id), &body, None, Pillar::Episodic, vec![]);
            n_turns += 1;
        }
    }
    sf.build_index()?;
    println!("[corpus] indexed {} turns with WINDOW={} context", n_turns, WINDOW);

    // For each "weak" query from the real-world probe, list the turns I *expect*
    // to be in top-10 based on reading the corpus. Then measure actual rank.
    let probes: Vec<(&str, Vec<&str>)> = vec![
        ("When did Caroline and Melanie talk about stress?",
            vec!["D7:22"]), // Melanie running to destress
        ("What kind of stuff do Melanie's kids get up to?",
            vec!["D6:6", "D4:8", "D8:4", "D9:1"]), // dinosaur exhibit, camping, pottery mugs
        ("What does Melanie do with her free time?",
            vec!["D5:4", "D7:22", "D14:30", "D10:12"]), // pottery, running, painting, camping
        ("Who are the important people in Caroline's life?",
            vec!["D4:3", "D6:13", "D12:10"]), // grandma in Sweden, support network, Melanie
    ];

    for (query, expected) in &probes {
        println!("\n════════════════════════════════════════════════════════════════");
        println!("Q: {}", query);
        println!("Expected gold ids: {:?}", expected);
        let hits = sf.recall(query, 100);
        print!("   ranks: ");
        for eid in expected {
            let rank = hits.iter().position(|r| &r.doc_id == eid);
            match rank {
                Some(r) => print!("{}=#{}  ", eid, r+1),
                None => print!("{}=MISS  ", eid),
            }
        }
        println!();
        // Show what was top-5
        println!("   top 5 actually returned:");
        for (i, r) in hits.iter().take(5).enumerate() {
            let s: String = r.content.chars().take(95).collect();
            let tag = if expected.contains(&r.doc_id.as_str()) { " ★" } else { "" };
            println!("     #{}  {}{}  {}", i+1, r.doc_id, tag, s.replace('\n', " "));
        }
    }

    let _ = std::fs::remove_file(&brain_path);
    let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    Ok(())
}

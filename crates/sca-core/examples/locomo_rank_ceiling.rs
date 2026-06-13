//! LoCoMo retrieval-rank CEILING diagnostic — no LLM calls.
//!
//! For each QA, retrieve the FULL ranked list (no k cap), find the rank
//! of the first gold evidence dia_id, and report the R@k curve across
//! k = 1, 5, 10, 20, 50, 100, 500, 1000, all.
//!
//! Why: F1 ceiling ≈ R@k for whatever k we feed the LLM. Before
//! spending 40 min of Claude calls on a full F1 run, we need to know:
//! at what k does retrieval cover ≥95% of gold evidence? If ≤100, we
//! just ship k=100. If we need k=600, we have a real retrieval problem
//! to fix.
//!
//! Run:
//!   cargo run --release -p sca-core --example locomo_rank_ceiling \
//!     --features "static-embed" -- --conv all

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
    category: u8,
    #[allow(dead_code)]
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
        let session_date = obj
            .get(&date_key)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
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
    Err("static encoder not found".to_string())
}

/// Given a ranked list (doc_ids in descending-score order) and a set of
/// gold doc_ids, return the 1-indexed rank of the FIRST gold hit, or None
/// if none of the gold ids appear in the ranked list.
fn first_gold_rank(ranked: &[String], gold: &HashSet<String>) -> Option<usize> {
    for (i, did) in ranked.iter().enumerate() {
        if gold.contains(did) {
            return Some(i + 1);
        }
    }
    None
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
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(File::open(dataset_path)?))?;
    println!("LoCoMo rank-ceiling diagnostic — {} conversations\n", samples.len());

    // Ladder of k values we report on.
    let ladder: [usize; 9] = [1, 5, 10, 20, 50, 100, 200, 500, 1000];
    let mut hits_at: Vec<u32> = vec![0; ladder.len()];
    let mut hits_at_by_cat: std::collections::HashMap<u8, Vec<u32>> = std::collections::HashMap::new();
    let mut total_scored: u32 = 0;
    let mut total_miss: u32 = 0; // gold nowhere in the ranked list
    let mut miss_by_cat: std::collections::HashMap<u8, u32> = std::collections::HashMap::new();
    let mut total_by_cat: std::collections::HashMap<u8, u32> = std::collections::HashMap::new();
    let mut rank_sum: u64 = 0; // for mean rank of hits
    let mut hit_count_for_mean: u32 = 0;

    for (idx, sample) in samples.iter().enumerate() {
        if let Some(f) = conv_filter { if idx != f { continue; } }
        let turns = extract_turns(&sample.conversation);
        if turns.is_empty() { continue; }

        let brain_path = format!("tmp_rank_ceiling_{}.said", sample.sample_id);
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
            sf.remember_with_pillar(
                Some(&t.dia_id),
                &body,
                None,
                Pillar::Episodic,
                vec![],
            );
        }
        sf.build_index()?;

        // Retrieve with a very large k to find the ceiling. `recall` is
        // capped by the ranked list length anyway, so top_k=10_000 gets us
        // "every doc that scored at all".
        let corpus_size = turns.len();
        let probe_k = corpus_size.max(1000);

        print!("[{}/{}] {} | corpus={} QA=? ", idx + 1, samples.len(), sample.sample_id, corpus_size);
        use std::io::Write;
        std::io::stdout().flush().ok();

        let mut conv_total: u32 = 0;
        let mut conv_miss: u32 = 0;

        for qa in &sample.qa {
            let gold_vec = parse_evidence(&qa.evidence);
            if gold_vec.is_empty() { continue; }
            let gold: HashSet<String> = gold_vec.into_iter().collect();

            let hits = sf.recall(&qa.question, probe_k);
            let ranked: Vec<String> = hits.into_iter().map(|h| h.doc_id).collect();

            total_scored += 1;
            conv_total += 1;
            *total_by_cat.entry(qa.category).or_insert(0) += 1;

            match first_gold_rank(&ranked, &gold) {
                Some(rank) => {
                    rank_sum += rank as u64;
                    hit_count_for_mean += 1;
                    // Tick every ladder tier this rank crosses.
                    for (i, &k) in ladder.iter().enumerate() {
                        if rank <= k {
                            hits_at[i] += 1;
                            let per_cat = hits_at_by_cat
                                .entry(qa.category)
                                .or_insert_with(|| vec![0u32; ladder.len()]);
                            per_cat[i] += 1;
                        }
                    }
                }
                None => {
                    total_miss += 1;
                    conv_miss += 1;
                    *miss_by_cat.entry(qa.category).or_insert(0) += 1;
                }
            }
        }

        println!("QA={} miss={}", conv_total, conv_miss);

        let _ = std::fs::remove_file(&brain_path);
        let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    }

    println!("\n════════════════════════════════════════════════════════════════");
    println!("OVERALL — {} scored, {} missing from ranked list entirely", total_scored, total_miss);
    println!("────────────────────────────────────────────────────────────────");
    if total_scored > 0 {
        let mean_rank_on_hit = rank_sum as f64 / hit_count_for_mean.max(1) as f64;
        println!("Mean rank of gold when retrieved: {:.1}", mean_rank_on_hit);
        println!();
        for (i, &k) in ladder.iter().enumerate() {
            let recall = hits_at[i] as f64 / total_scored as f64;
            let bar = "█".repeat((recall * 40.0) as usize);
            println!("R@{:<5} = {:.4}  ({:>4}/{})  {}", k, recall, hits_at[i], total_scored, bar);
        }
        let unretrieveable = total_miss as f64 / total_scored as f64;
        println!("\nUnretrievable ceiling: {:.4}  ({}/{})", 1.0 - unretrieveable, total_scored - total_miss, total_scored);
        println!("  — this is the hard upper bound of R@k for any k; if it's <1.0, retrieval is fundamentally missing some gold evidence");
    }

    println!("\nBy category:");
    let mut cats: Vec<&u8> = total_by_cat.keys().collect();
    cats.sort();
    for cat in cats {
        let total = total_by_cat.get(cat).copied().unwrap_or(0);
        let miss = miss_by_cat.get(cat).copied().unwrap_or(0);
        let per_cat = hits_at_by_cat.get(cat);
        print!("  cat {}: n={:4}  miss={:3}  ", cat, total, miss);
        if let Some(h) = per_cat {
            for (i, &k) in ladder.iter().enumerate() {
                if matches!(k, 1 | 10 | 100 | 1000) {
                    let r = h[i] as f64 / total.max(1) as f64;
                    print!("R@{}={:.3} ", k, r);
                }
            }
        }
        println!();
    }

    Ok(())
}

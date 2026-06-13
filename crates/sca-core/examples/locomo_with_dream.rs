//! LoCoMo with content-level dream — Decision 5 lift measurement.
//!
//! Same eval as `locomo_baseline.rs` (Recall@k on gold evidence dia_ids) but
//! INGESTS each conversation into a fresh `.said` brain as Episodic frames,
//! runs `run_dream_content(1, ...)` to create Semantic consolidations, then
//! queries via `SaidFile::recall`. Compares the R@k line against the 0.5540
//! pre-dream baseline.
//!
//! What this measures: does consolidation surface relevant turns that the
//! raw pipeline misses? Expectation: cat 1 (single-hop factoid) and cat 5
//! (open-domain) should lift because a distilled Semantic frame matches a
//! short query better than any single raw turn. Cat 3 (temporal) shouldn't
//! move much in v1 — temporal reasoning needs the recency rerank, not
//! content consolidation.
//!
//! Run:
//!   cargo run --release -p sca-core --example locomo_with_dream \
//!     --features "static-embed" -- --conv all

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use sca_core::dream::DreamParams;
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
            let dia_id = t
                .get("dia_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let speaker = t
                .get("speaker")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let text = t
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
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

/// Check if any of the top-k ranked hits overlap the gold evidence set.
/// A ranked hit can be either a raw Episodic dia_id OR a Semantic
/// consolidation whose `derived_from:<id1>,<id2>,...` tag includes a gold
/// dia_id. Both count — the whole point of consolidation is to make the
/// gold evidence findable via the distilled frame.
fn hit_at_k(
    ranked: &[(String, f32)],
    gold: &HashSet<String>,
    derived_of: &HashMap<String, Vec<String>>,
    k: usize,
) -> bool {
    for (did, _) in ranked.iter().take(k) {
        if gold.contains(did) {
            return true;
        }
        if let Some(derived) = derived_of.get(did) {
            for src in derived {
                if gold.contains(src) {
                    return true;
                }
            }
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
    let mut threshold: f32 = 0.40;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--conv" {
            if i + 1 < args.len() && args[i + 1] != "all" {
                conv_filter = Some(args[i + 1].parse()?);
            }
            i += 2;
        } else if args[i] == "--threshold" {
            if i + 1 < args.len() {
                threshold = args[i + 1].parse()?;
            }
            i += 2;
        } else {
            i += 1;
        }
    }

    let encoder_path = find_encoder()?;
    println!("[encoder] {}", encoder_path);
    println!("[dream threshold] {}", threshold);

    let dataset_path = PathBuf::from("research/locomo/data/locomo10.json");
    let f = File::open(&dataset_path)
        .map_err(|e| format!("cannot open {}: {}", dataset_path.display(), e))?;
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(f))?;

    println!("LoCoMo with dream — {} conversations", samples.len());
    println!("Pipeline: SaidFile ingest → run_dream_content(1) → recall\n");

    let ks = [1usize, 5, 10, 20];
    let mut overall: HashMap<usize, (u32, u32)> = HashMap::new();
    let mut by_cat: HashMap<u8, HashMap<usize, (u32, u32)>> = HashMap::new();
    let mut clusters_total: u32 = 0;
    let mut semantic_frames_total: u32 = 0;

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

        // Per-conversation brain file (kept under target/ so cleanup is easy).
        let brain_path = format!("tmp_locomo_dream_{}.said", sample.sample_id);
        let _ = std::fs::remove_file(&brain_path);

        let mut sf = SaidFile::create(&brain_path);
        sf.engine.load_static_encoder(encoder_path)?;
        sf.engine.core.set_holographic_16view(false, None);

        // Ingest each turn as an Episodic frame. Doc_id = dia_id so the
        // gold evidence lookup works as-is.
        for t in &turns {
            let body = format!("{}: {}", t.speaker, t.text);
            sf.remember_with_pillar(
                Some(&t.dia_id),
                &body,
                None,
                Pillar::Episodic,
                vec![],
            );
        }
        sf.build_index()?;

        // Run dream with the supplied threshold.
        let params = DreamParams {
            cluster_threshold: threshold,
            ..DreamParams::default()
        };
        let report = sf.run_dream_content(1, &params);
        clusters_total += report.clusters_formed as u32;
        semantic_frames_total += report.semantic_frames_created.len() as u32;

        // Re-index so Semantic frames participate in retrieval.
        sf.build_index()?;

        // Build a derived-from map so hit_at_k can credit Semantic frames.
        let mut derived_of: HashMap<String, Vec<String>> = HashMap::new();
        for meta in sf.frames.get_all_frames() {
            if meta.pillar != Pillar::Semantic {
                continue;
            }
            for tag in &meta.tags {
                if let Some(rest) = tag.strip_prefix("derived_from:") {
                    let ids: Vec<String> =
                        rest.split(',').map(|s| s.trim().to_string()).collect();
                    derived_of.insert(meta.doc_id.clone(), ids);
                    break;
                }
            }
        }

        use std::io::Write;
        print!(
            "[{}/{}] {} | turns={} dreamed_clusters={} sem={} ... ",
            idx + 1,
            samples.len(),
            sample.sample_id,
            turns.len(),
            report.clusters_formed,
            report.semantic_frames_created.len()
        );
        std::io::stdout().flush().ok();

        let mut local_hits_at_10: u32 = 0;
        let mut local_total: u32 = 0;

        for qa in &sample.qa {
            let gold_vec = parse_evidence(&qa.evidence);
            if gold_vec.is_empty() {
                continue;
            }
            let gold: HashSet<String> = gold_vec.into_iter().collect();

            // Recall via SaidFile — returns RecallResult with doc_ids we can
            // feed straight into hit_at_k.
            let results = sf.recall(&qa.question, 20);
            let ranked: Vec<(String, f32)> =
                results.into_iter().map(|r| (r.doc_id, r.score)).collect();

            local_total += 1;
            for &k in &ks {
                let hit = hit_at_k(&ranked, &gold, &derived_of, k);
                let e = overall.entry(k).or_insert((0, 0));
                e.1 += 1;
                if hit {
                    e.0 += 1;
                    if k == 10 {
                        local_hits_at_10 += 1;
                    }
                }
                let ce = by_cat
                    .entry(qa.category)
                    .or_default()
                    .entry(k)
                    .or_insert((0, 0));
                ce.1 += 1;
                if hit {
                    ce.0 += 1;
                }
            }
        }

        let r10 = local_hits_at_10 as f64 / local_total.max(1) as f64;
        println!("R@10 = {:.4}", r10);

        let _ = std::fs::remove_file(&brain_path);
        let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    }

    println!("\n════════════════════════════════════════════════════════════════");
    println!(
        "DREAM STATS   clusters_total={}  semantic_frames_total={}",
        clusters_total, semantic_frames_total
    );
    println!("OVERALL (with dream)");
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

    println!("\nPre-dream baseline for comparison:");
    println!("  Recall@10 = 0.5540  (1098 / 1982)  ← locomo_baseline");
    Ok(())
}

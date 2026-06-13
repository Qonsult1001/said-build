//! LoCoMo F1 harness — the Decision-5 v2 comparability measurement.
//!
//! For each LoCoMo QA pair:
//!   1. Ingest the conversation's turns into a `.said` brain (one per conv).
//!   2. Query `.said` for top-k evidence (default k=20).
//!   3. Pipe Q + evidence into `scripts/claude-answer.sh` (Claude Opus 4.7
//!      via Claude Code CLI — no API key needed).
//!   4. Write a JSONL line with the gold answer + predicted answer.
//!
//! A separate Python scorer (`scripts/locomo_score_f1.py`) reads the JSONL
//! and computes the LoCoMo-official F1 (stem-aware token overlap,
//! matching `research/locomo/task_eval/evaluation.py::f1_score`).
//!
//! This split (Rust = retrieval + LLM call, Python = scoring) matches the
//! LoCoMo reference pipeline so the number is directly comparable to
//! their published 91.6 F1.
//!
//! Run:
//!   cargo run --release -p sca-core --example locomo_f1 \
//!     --features "static-embed" -- --conv 0 --top-k 20 \
//!     --out tmp_locomo_f1_conv0.jsonl
//!   python scripts/locomo_score_f1.py tmp_locomo_f1_conv0.jsonl

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

// ═════════════════════════════════════════════════════════════════════════
// LoCoMo schema (only fields we use)
// ═════════════════════════════════════════════════════════════════════════

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
    answer: Option<Value>,
    #[serde(default)]
    evidence: Option<Value>,
    category: u8,
}

struct Turn {
    dia_id: String,
    speaker: String,
    text: String,
    session_date: String, // e.g. "8:56 pm on 20 July, 2023" — empty if missing
    blip_caption: String, // image caption — multimodal LoCoMo turns carry answers here, not in text
}

fn extract_turns(conversation: &Value) -> Vec<Turn> {
    let Some(obj) = conversation.as_object() else { return Vec::new() };
    let mut turns = Vec::new();
    for (k, v) in obj {
        if !k.starts_with("session_") || k.ends_with("date_time") { continue; }
        // Resolve matching session date (`<session_name>_date_time`).
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
            let blip_caption = t.get("blip_caption").and_then(|x| x.as_str()).unwrap_or("").to_string();
            // Keep turns that have either text OR a caption (image-only turns are valid answers).
            if dia_id.is_empty() || (text.is_empty() && blip_caption.is_empty()) { continue; }
            turns.push(Turn {
                dia_id,
                speaker,
                text,
                session_date: session_date.clone(),
                blip_caption,
            });
        }
    }
    turns
}

fn answer_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) => a
            .iter()
            .map(answer_as_string)
            .collect::<Vec<_>>()
            .join(", "),
        _ => v.to_string(),
    }
}

fn parse_evidence(ev: &Option<Value>) -> Vec<String> {
    match ev {
        Some(Value::Array(arr)) => arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Paths + bash
// ═════════════════════════════════════════════════════════════════════════

fn find_encoder() -> Result<&'static str, String> {
    let paths: [&'static str; 4] = [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "../../SAID-LAM-private/said-lam-static",
    ];
    for p in paths { if Path::new(p).exists() { return Ok(p); } }
    Err("static encoder not found — run from SAID-ECHO repo root".to_string())
}

fn find_answer_script_named(name: &str) -> Result<String, String> {
    let paths = [
        format!("scripts/{}", name),
        format!("../scripts/{}", name),
        format!("../../scripts/{}", name),
    ];
    for p in paths { if Path::new(&p).exists() { return Ok(p); } }
    Err(format!("scripts/{} not found", name))
}

fn find_answer_script() -> Result<String, String> {
    find_answer_script_named("claude-answer.sh")
}

fn find_bash() -> Result<String, String> {
    let candidates: [&str; 5] = [
        "C:\\Program Files\\Git\\bin\\bash.exe",
        "C:\\Program Files\\Git\\usr\\bin\\bash.exe",
        "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
        "/usr/bin/bash",
        "bash",
    ];
    for c in candidates {
        if c.contains('\\') || c.starts_with('/') {
            if Path::new(c).exists() { return Ok(c.to_string()); }
        } else {
            return Ok(c.to_string());
        }
    }
    Err("no bash binary found".to_string())
}

/// Invoke claude-answer.sh with the Q+evidence payload; return the answer.
/// Retry wrapper with exponential backoff — matches mem0's approach
/// (`add.py` retries 3x with time.sleep(1)). Claude CLI rate-limits silently
/// at ~3-5 qps; pacing + retry keeps us under that ceiling without flags.
fn answer_via_claude_retry(
    bash_path: &str,
    script: &str,
    payload: &str,
    claude_model: Option<&str>,
) -> Result<String, String> {
    let max_retries = 3u32;
    let mut last_err = String::new();
    for attempt in 0..=max_retries {
        if attempt > 0 {
            // Exponential backoff: 1s, 2s, 4s
            let wait_ms = 1000u64 * (1u64 << (attempt - 1));
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        }
        match answer_via_claude(bash_path, script, payload, claude_model) {
            Ok(s) => {
                if s.trim().is_empty() {
                    last_err = format!("empty output on attempt {}", attempt + 1);
                    continue;
                }
                return Ok(s);
            }
            Err(e) => {
                last_err = e;
                // Keep retrying on any error — the CLI often fails silently
                // with exit 1 and empty stderr on transient rate limits.
            }
        }
    }
    Err(format!("claude-answer failed after {} retries: {}", max_retries + 1, last_err))
}

fn answer_via_claude(
    bash_path: &str,
    script: &str,
    payload: &str,
    claude_model: Option<&str>,
) -> Result<String, String> {
    // Resolve full CLAUDE_CMD for bash subprocess. On Windows+nvm4w, claude.cmd
    // can't run under bash — use "node /unix/path/cli.js" instead (eval-expanded in script).
    let base_cmd = std::env::var("CLAUDE_CMD")
        .unwrap_or_else(|_| {
            let win_cli = "C:\\nvm4w\\nodejs\\node_modules\\@anthropic-ai\\claude-code\\cli.js";
            if std::path::Path::new(win_cli).exists() {
                return "node /c/nvm4w/nodejs/node_modules/@anthropic-ai/claude-code/cli.js".to_string();
            }
            "claude".to_string()
        });
    let claude_cmd = if let Some(m) = claude_model {
        format!("{} --model {}", base_cmd, m)
    } else {
        base_cmd
    };
    let mut child = Command::new(bash_path)
        .arg(script)
        .env("CLAUDE_CMD", &claude_cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {}: {}", bash_path, e))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(payload.as_bytes()).map_err(|e| format!("stdin: {}", e))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("wait: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("claude-answer exit {:?} stderr={}", out.status.code(), stderr.trim()));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { return Err("empty claude output".to_string()); }
    Ok(s)
}

// ═════════════════════════════════════════════════════════════════════════
// Main
// ═════════════════════════════════════════════════════════════════════════

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut conv_filter: Option<usize> = None;
    let mut top_k: usize = 20;
    let mut out_path: String = "tmp_locomo_f1.jsonl".to_string();
    let mut qa_limit: Option<usize> = None;
    let mut claude_model: Option<String> = None;
    let mut multistage = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--conv" => { if i + 1 < args.len() && args[i + 1] != "all" { conv_filter = Some(args[i + 1].parse()?); } i += 2; }
            "--top-k" => { if i + 1 < args.len() { top_k = args[i + 1].parse()?; } i += 2; }
            "--out" => { if i + 1 < args.len() { out_path = args[i + 1].clone(); } i += 2; }
            "--limit" => { if i + 1 < args.len() { qa_limit = Some(args[i + 1].parse()?); } i += 2; }
            "--model" => { if i + 1 < args.len() { claude_model = Some(args[i + 1].clone()); } i += 2; }
            "--multistage" => { multistage = true; i += 1; }
            _ => i += 1,
        }
    }

    let encoder_path = find_encoder()?;
    let answer_script = if multistage {
        find_answer_script_named("claude-answer-multistage.sh")?
    } else {
        find_answer_script()?
    };
    let bash_path = find_bash()?;
    println!("[encoder] {}", encoder_path);
    println!("[answer]  {}", answer_script);
    println!("[bash]    {}", bash_path);
    println!("[params]  top_k={} out={} conv={:?} limit={:?} model={}", top_k, out_path, conv_filter, qa_limit, claude_model.as_deref().unwrap_or("default"));

    let dataset_path = PathBuf::from("research/locomo/data/locomo10.json");
    let f = File::open(&dataset_path)
        .map_err(|e| format!("cannot open {}: {}", dataset_path.display(), e))?;
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(f))?;

    let out_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&out_path)?;
    let mut writer = BufWriter::new(out_file);

    let mut qa_done: u32 = 0;
    let mut qa_total: u32 = 0;
    let mut llm_ms_total: u64 = 0;
    let mut llm_calls: u32 = 0;
    let mut errors: u32 = 0;

    for (idx, sample) in samples.iter().enumerate() {
        if let Some(f) = conv_filter { if idx != f { continue; } }

        let turns = extract_turns(&sample.conversation);
        if turns.is_empty() { continue; }

        let brain_path = format!("tmp_locomo_f1_{}.said", sample.sample_id);
        let _ = std::fs::remove_file(&brain_path);

        let mut sf = SaidFile::create(&brain_path);
        sf.engine.load_static_encoder(encoder_path)?;
        sf.engine.core.set_holographic_16view(false, None);

        for t in &turns {
            // Prepend the session date so temporal grounding is available
            // both at retrieval (SCA can match "May 2023" to a frame) and
            // at answer time (Opus sees the date in the evidence block).
            // Append the image caption (blip_caption) when present — multimodal
            // LoCoMo turns carry the entity/event in the image, not the text.
            let text_part = if t.blip_caption.is_empty() {
                t.text.clone()
            } else if t.text.is_empty() {
                format!("[image: {}]", t.blip_caption)
            } else {
                format!("{} [image: {}]", t.text, t.blip_caption)
            };
            let body = if t.session_date.is_empty() {
                format!("{}: {}", t.speaker, text_part)
            } else {
                format!("[{}] {}: {}", t.session_date, t.speaker, text_part)
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

        println!("\n[conv {}/{} — {}]", idx + 1, samples.len(), sample.sample_id);

        let mut qa_processed_this_conv: u32 = 0;
        for (qa_idx, qa) in sample.qa.iter().enumerate() {
            if let Some(lim) = qa_limit {
                if qa_processed_this_conv >= lim as u32 { break; }
            }
            qa_total += 1;

            let Some(gold_val) = qa.answer.as_ref() else { continue; };
            let gold_str = answer_as_string(gold_val);
            if gold_str.trim().is_empty() { continue; }

            // Retrieve top-k evidence.
            let hits = sf.recall(&qa.question, top_k);
            if hits.is_empty() {
                writer.write_all(
                    serde_json::to_string(&json!({
                        "conv": sample.sample_id,
                        "qa_idx": qa_idx,
                        "category": qa.category,
                        "question": qa.question,
                        "gold": gold_str,
                        "predicted": "",
                        "evidence_dia_ids": parse_evidence(&qa.evidence),
                        "retrieved_dia_ids": Vec::<String>::new(),
                        "error": "no retrieval hits",
                    }))?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                errors += 1;
                continue;
            }

            // Build payload for the LLM.
            let mut payload = String::new();
            payload.push_str("Q: ");
            payload.push_str(&qa.question);
            payload.push_str("\n---8<---\n");
            let mut retrieved_dia_ids: Vec<String> = Vec::new();
            for (i, h) in hits.iter().enumerate() {
                payload.push_str(&format!("E{}: {}\n", i + 1, h.content));
                retrieved_dia_ids.push(h.doc_id.clone());
            }

            let t0 = Instant::now();
            // Small pacing delay between sequential calls so Claude CLI doesn't
            // throttle us. 200ms = ~5 qps ceiling, well under mem0's 2-thread
            // concurrency equivalent of ~2 qps.
            if qa_done > 0 {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let result = answer_via_claude_retry(&bash_path, &answer_script, &payload, claude_model.as_deref());
            let elapsed = t0.elapsed().as_millis() as u64;

            let (predicted, err): (String, Option<String>) = match result {
                Ok(s) => {
                    llm_calls += 1;
                    llm_ms_total += elapsed;
                    (s, None)
                }
                Err(e) => {
                    errors += 1;
                    (String::new(), Some(e))
                }
            };

            writer.write_all(
                serde_json::to_string(&json!({
                    "conv": sample.sample_id,
                    "qa_idx": qa_idx,
                    "category": qa.category,
                    "question": qa.question,
                    "gold": gold_str,
                    "predicted": predicted,
                    "evidence_dia_ids": parse_evidence(&qa.evidence),
                    "retrieved_dia_ids": retrieved_dia_ids,
                    "elapsed_ms": elapsed,
                    "error": err,
                }))?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
            writer.flush()?;

            qa_done += 1;
            qa_processed_this_conv += 1;
            let avg_ms = if llm_calls > 0 { llm_ms_total / llm_calls as u64 } else { 0 };
            print!("\r  qa {}/{} [avg {}ms, errors={}] ", qa_done, qa_total, avg_ms, errors);
            std::io::stdout().flush().ok();
        }
        println!();

        let _ = std::fs::remove_file(&brain_path);
        let _ = std::fs::remove_file(format!("{}.tmp", brain_path));
    }

    println!("\n════════════════════════════════════════════════════════════════");
    println!("F1 HARNESS DONE");
    println!("qa_done={} qa_total={} errors={}", qa_done, qa_total, errors);
    if llm_calls > 0 {
        println!("avg_llm_ms={}", llm_ms_total / llm_calls as u64);
    }
    println!("output: {}", out_path);
    println!("\nRun: python scripts/locomo_score_f1.py {}", out_path);

    Ok(())
}

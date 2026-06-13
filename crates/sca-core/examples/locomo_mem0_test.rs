//! Mem0-equivalent pipeline test — proves whether mem0's win comes from
//! LLM write-time distillation (not retrieval magic).
//!
//! Pipeline:
//!   1. For each session in a LoCoMo conversation, send the whole session
//!      to scripts/mem0_distill.sh (Claude Opus 4.7 running mem0's
//!      ADDITIVE_EXTRACTION_PROMPT). Collect the extracted memories.
//!   2. For each QA, send Q + ALL distilled memories to scripts/mem0_answer.sh
//!      (Claude Opus 4.7 running mem0's ANSWER_PROMPT).
//!   3. Write JSONL compatible with scripts/locomo_score_f1.py.
//!
//! This uses ZERO .said retrieval — it's a straight mem0 port for comparison.
//! If F1 jumps from our 0.55 to ~0.85, the concept works. Then we design how
//! to build the same distillation effect into .said without an LLM.
//!
//! Run:
//!   cargo run --release -p sca-core --example locomo_mem0_test -- \
//!     --conv 0 --limit 5 --out tmp_mem0_test.jsonl

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

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

struct SessionBlock {
    #[allow(dead_code)]
    session_key: String,
    session_date: String,
    turns: Vec<(String, String, String)>, // (dia_id, speaker, text_with_caption)
}

fn extract_sessions(conversation: &Value) -> Vec<SessionBlock> {
    let Some(obj) = conversation.as_object() else { return Vec::new() };
    // Collect keys in deterministic order (session_1, session_2, ...)
    let mut keys: Vec<String> = obj.keys()
        .filter(|k| k.starts_with("session_") && !k.ends_with("date_time"))
        .cloned()
        .collect();
    keys.sort_by_key(|k| {
        k.trim_start_matches("session_").parse::<u32>().unwrap_or(u32::MAX)
    });
    let mut out = Vec::new();
    for k in keys {
        let date_key = format!("{}_date_time", k);
        let session_date = obj.get(&date_key)
            .and_then(|x| x.as_str()).unwrap_or("").to_string();
        let Some(arr) = obj.get(&k).and_then(|v| v.as_array()) else { continue };
        let mut turns = Vec::new();
        for t in arr {
            let dia_id = t.get("dia_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let speaker = t.get("speaker").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = t.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let caption = t.get("blip_caption").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if dia_id.is_empty() || (text.is_empty() && caption.is_empty()) { continue; }
            let body = if caption.is_empty() {
                text
            } else if text.is_empty() {
                format!("[image: {}]", caption)
            } else {
                format!("{} [image: {}]", text, caption)
            };
            turns.push((dia_id, speaker, body));
        }
        if !turns.is_empty() {
            out.push(SessionBlock { session_key: k, session_date, turns });
        }
    }
    out
}

fn answer_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) => a.iter().map(|x| answer_as_string(x)).collect::<Vec<_>>().join(", "),
        Value::Null => String::new(),
        _ => v.to_string(),
    }
}

fn find_bash() -> Result<String, String> {
    for cand in ["C:\\Program Files\\Git\\bin\\bash.exe", "/usr/bin/bash", "/bin/bash"] {
        if Path::new(cand).exists() { return Ok(cand.to_string()); }
    }
    Err("bash not found".to_string())
}

fn find_script(name: &str) -> Result<String, String> {
    for rel in [format!("scripts/{}", name), format!("../scripts/{}", name), format!("../../scripts/{}", name)] {
        if Path::new(&rel).exists() { return Ok(rel); }
    }
    Err(format!("scripts/{} not found", name))
}

fn run_script(bash_path: &str, script: &str, payload: &str, claude_cmd: &str) -> Result<String, String> {
    let mut child = Command::new(bash_path)
        .arg(script)
        .env("CLAUDE_CMD", claude_cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {}: {}", script, e))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(payload.as_bytes()).map_err(|e| format!("stdin: {}", e))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("wait: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{} exit {:?} stderr={}", script, out.status.code(), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn resolve_claude_cmd() -> String {
    if let Ok(v) = std::env::var("CLAUDE_CMD") { return v; }
    let win_cli = "C:\\nvm4w\\nodejs\\node_modules\\@anthropic-ai\\claude-code\\cli.js";
    if Path::new(win_cli).exists() {
        return "node /c/nvm4w/nodejs/node_modules/@anthropic-ai/claude-code/cli.js".to_string();
    }
    "claude".to_string()
}

fn distill_session(
    bash_path: &str, distill_script: &str, claude_cmd: &str, session: &SessionBlock,
) -> Result<Vec<String>, String> {
    let mut payload = format!("SESSION_DATE: {}\n---8<---\n", session.session_date);
    for (_, speaker, text) in &session.turns {
        payload.push_str(&format!("{}: {}\n", speaker, text));
    }
    let raw = run_script(bash_path, distill_script, &payload, claude_cmd)?;
    // Expect JSON like {"memory":[{"id":"0","text":"..."}, ...]}.
    // Be forgiving — strip markdown fences if model added them.
    let cleaned = raw.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim();
    let v: Value = serde_json::from_str(cleaned)
        .map_err(|e| format!("distill JSON parse error: {} — raw: {}", e, cleaned.chars().take(200).collect::<String>()))?;
    let arr = v.get("memory").and_then(|x| x.as_array())
        .ok_or_else(|| "distill missing 'memory' array".to_string())?;
    let mut out = Vec::new();
    for m in arr {
        if let Some(t) = m.get("text").and_then(|x| x.as_str()) {
            if !t.is_empty() { out.push(t.to_string()); }
        }
    }
    Ok(out)
}

fn answer_question(
    bash_path: &str, answer_script: &str, claude_cmd: &str,
    question: &str, memories: &[String],
) -> Result<String, String> {
    let mut payload = format!("Q: {}\n---8<---\n", question);
    for (i, m) in memories.iter().enumerate() {
        payload.push_str(&format!("M{}: {}\n", i + 1, m));
    }
    run_script(bash_path, answer_script, &payload, claude_cmd)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut conv_filter: Option<usize> = None;
    let mut qa_limit: Option<usize> = None;
    let mut out_path = "tmp_mem0_test.jsonl".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--conv" => { if i+1 < args.len() && args[i+1] != "all" { conv_filter = Some(args[i+1].parse()?); } i += 2; }
            "--limit" => { if i+1 < args.len() { qa_limit = Some(args[i+1].parse()?); } i += 2; }
            "--out" => { if i+1 < args.len() { out_path = args[i+1].clone(); } i += 2; }
            _ => i += 1,
        }
    }

    let bash_path = find_bash()?;
    let distill_script = find_script("mem0_distill.sh")?;
    let answer_script = find_script("mem0_answer.sh")?;
    let claude_cmd = resolve_claude_cmd();
    println!("[bash]     {}", bash_path);
    println!("[distill]  {}", distill_script);
    println!("[answer]   {}", answer_script);
    println!("[claude]   {}", claude_cmd);
    println!("[params]   conv={:?} limit={:?} out={}", conv_filter, qa_limit, out_path);

    let dataset_path = PathBuf::from("research/locomo/data/locomo10.json");
    let samples: Vec<Sample> = serde_json::from_reader(BufReader::new(File::open(&dataset_path)?))?;

    let out_file = OpenOptions::new().create(true).truncate(true).write(true).open(&out_path)?;
    let mut writer = BufWriter::new(out_file);

    for (idx, sample) in samples.iter().enumerate() {
        if let Some(f) = conv_filter { if idx != f { continue; } }
        let sessions = extract_sessions(&sample.conversation);
        println!("\n[conv {}/{} — {}] sessions={}", idx+1, samples.len(), sample.sample_id, sessions.len());

        // Distill every session.
        let mut all_memories: Vec<String> = Vec::new();
        for (si, sess) in sessions.iter().enumerate() {
            let t0 = Instant::now();
            match distill_session(&bash_path, &distill_script, &claude_cmd, sess) {
                Ok(mems) => {
                    println!("  distill session {}/{}: {} memories ({}ms)",
                        si+1, sessions.len(), mems.len(), t0.elapsed().as_millis());
                    all_memories.extend(mems);
                }
                Err(e) => {
                    println!("  distill session {}/{}: ERROR {}", si+1, sessions.len(), e);
                }
            }
        }
        println!("  total memories: {}", all_memories.len());

        // Answer every QA.
        let mut qa_done = 0usize;
        for (qi, qa) in sample.qa.iter().enumerate() {
            if let Some(lim) = qa_limit { if qa_done >= lim { break; } }
            let gold_text = qa.answer.as_ref().map(answer_as_string).unwrap_or_default();
            if gold_text.is_empty() { continue; } // skip QA with no gold

            let t0 = Instant::now();
            let (predicted, err) = match answer_question(&bash_path, &answer_script, &claude_cmd, &qa.question, &all_memories) {
                Ok(s) => (s, None),
                Err(e) => (String::new(), Some(e)),
            };
            let elapsed = t0.elapsed().as_millis() as u64;
            println!("  qa {}/{}: {} ({}ms)", qi+1, sample.qa.len(),
                if err.is_some() { "ERR" } else { "ok" }, elapsed);

            let evidence_ids: Vec<String> = qa.evidence.as_ref().and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();

            let record = json!({
                "conv": sample.sample_id,
                "qa_idx": qi,
                "category": qa.category,
                "question": qa.question,
                "gold": gold_text,
                "predicted": predicted,
                "evidence_dia_ids": evidence_ids,
                "retrieved_dia_ids": Vec::<String>::new(),
                "elapsed_ms": elapsed,
                "error": err,
                "memory_count": all_memories.len(),
            });
            writeln!(writer, "{}", record)?;
            writer.flush()?;
            qa_done += 1;
        }
    }

    println!("\n════════════════════════════════════════════════════════════════");
    println!("MEM0-EQUIVALENT TEST DONE — output: {}", out_path);
    println!("Score with: python scripts/locomo_score_f1.py {}", out_path);
    Ok(())
}

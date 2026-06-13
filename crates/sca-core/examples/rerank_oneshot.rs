//! One-shot Phase-Q-style rerank against any .said file with any query.
//!
//! Purpose: empirically prove (or disprove) whether Phase Q rerank
//! recovers the S2.2-class failure where the answer chunk sits past
//! `--deep` rank 30 and the agent loop can't reach it.
//!
//! Usage:
//!   PATH_SAID=willie.said \
//!   QUERY="Vendor SLA Project Prometheus signed" \
//!   cargo run --release --example rerank_oneshot --features static-embed
//!
//! Provider: defaults to local Claude Code CLI (no API key required).
//! Override with PROVIDER=anthropic|groq + the matching env keys.

use std::env;
use std::path::PathBuf;
use std::time::Instant;

use sca_core::ask::ask;
use sca_core::said_file::SaidFile;
use said_llm::{provider_from_config, CompletionRequest, LlmConfig};
use serde_json::{json, Value};

const TOP_N_RERANK: usize = 10;
const SNIPPET_CHARS: usize = 400;
const CANDIDATES_TO_SEND: usize = 30;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path_said = env::var("PATH_SAID")
        .unwrap_or_else(|_| "willie.said".into());
    let query = env::var("QUERY")
        .unwrap_or_else(|_| "Vendor SLA Project Prometheus signed".into());

    println!("=== rerank_oneshot ===");
    println!("brain: {}", path_said);
    println!("query: {:?}", query);
    println!();

    // ── Step 1: open brain + run ask --deep ────────────────────────────
    let mut brain = SaidFile::open(PathBuf::from(&path_said))?;
    let t0 = Instant::now();
    let (kept, _kw) = ask(&mut brain, &query, 200, true /* deep */, None);
    let ask_ms = t0.elapsed().as_millis();
    println!("--- BEFORE rerank ---");
    println!("brain returned {} candidates in {} ms (--deep mode)\n", kept.len(), ask_ms);
    println!("Top 15 BEFORE rerank:");
    for (i, c) in kept.iter().take(15).enumerate() {
        let snippet: String = c.content.chars().take(80).collect::<String>()
            .replace('\n', " ").replace('\r', "");
        println!("  {:2}. conf={:.3}  {:55}  {}", i + 1, c.confidence, c.doc_id, snippet);
    }
    println!();

    // ── Step 2: build the Phase Q rerank request ───────────────────────
    let send_count = kept.len().min(CANDIDATES_TO_SEND);
    let candidates: Vec<(String, String)> = kept.iter()
        .take(send_count)
        .map(|c| (c.doc_id.clone(), c.content.clone()))
        .collect();

    let system = format!(
        "You are a retrieval reranker. Given a question and {} candidate \
         document chunks (each with an ID and a snippet), pick the {} most \
         likely to contain the answer, IN ORDER (most likely first). \
         Only use the IDs as given. Don't invent IDs.",
        candidates.len(),
        TOP_N_RERANK,
    );

    let mut user = String::new();
    user.push_str("Question: ");
    user.push_str(&query);
    user.push_str("\n\nCandidates:\n");
    for (i, (id, body)) in candidates.iter().enumerate() {
        let snippet: String = body.chars().take(SNIPPET_CHARS).collect();
        user.push_str(&format!("[{}] id={} | {}\n", i + 1, id, snippet));
    }
    user.push_str(&format!(
        "\nReturn the {} most likely candidate IDs in ranked order (most likely first). \
         If unsure, prefer chunks that mention the specific entities or topics from the question.",
        TOP_N_RERANK,
    ));

    let schema = json!({
        "type": "object",
        "properties": {
            "ranked_ids": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "maxItems": TOP_N_RERANK + 5,
            }
        },
        "required": ["ranked_ids"]
    });

    let request = CompletionRequest {
        system,
        user,
        cacheable_prelude: None,
        schema,
        schema_name: "rerank_top_n".into(),
        max_output_tokens: 800,
        temperature: 0.0,
    };

    // ── Step 3: pick provider — default Claude CLI ─────────────────────
    let provider_kind = env::var("PROVIDER").unwrap_or_else(|_| "claude-cli".into());
    let llm_cfg = match provider_kind.as_str() {
        "anthropic" => LlmConfig::anthropic_from_env(
            &env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-5".into()),
        ).ok_or("ANTHROPIC_API_KEY not set")?,
        "groq" => LlmConfig::groq_from_env(
            &env::var("GROQ_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".into()),
        ).ok_or("GROQ_API_KEY not set")?,
        _ => LlmConfig::claude_cli(""),
    };
    println!("--- Calling LLM rerank ---");
    println!("provider: {}", llm_cfg.provider_name());
    println!("sending top {} candidates ({} chars each)\n", send_count, SNIPPET_CHARS);

    let provider = provider_from_config(&llm_cfg)?;
    let t1 = Instant::now();
    let resp = provider.complete(&request).await?;
    let llm_ms = t1.elapsed().as_millis();

    let ranked_ids: Vec<String> = resp.json
        .get("ranked_ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    println!("LLM returned {} ids in {} ms", ranked_ids.len(), llm_ms);
    println!(
        "usage: input={} output={}",
        resp.usage.input_tokens, resp.usage.output_tokens
    );
    println!();

    // ── Step 4: print AFTER rerank ─────────────────────────────────────
    println!("--- AFTER rerank ---");
    let body_by_id: std::collections::HashMap<String, String> = candidates
        .iter()
        .map(|(id, body)| (id.clone(), body.clone()))
        .collect();

    for (i, id) in ranked_ids.iter().take(TOP_N_RERANK).enumerate() {
        let body = body_by_id
            .get(id)
            .cloned()
            .unwrap_or_else(|| "(id not in candidate set)".into());
        let snippet: String = body.chars().take(80).collect::<String>()
            .replace('\n', " ").replace('\r', "");
        let prev_rank = candidates.iter().position(|(cid, _)| cid == id).map(|p| p + 1);
        let prev_str = prev_rank
            .map(|p| format!("was #{}", p))
            .unwrap_or_else(|| "NEW".into());
        println!("  {:2}. ({:7})  {:55}  {}", i + 1, prev_str, id, snippet);
    }

    println!();
    println!("--- TIMING ---");
    println!("brain --deep: {} ms", ask_ms);
    println!("LLM rerank:   {} ms", llm_ms);
    println!("total:        {} ms", ask_ms + llm_ms);

    Ok(())
}

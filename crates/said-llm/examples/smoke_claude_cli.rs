//! Smoke test the ClaudeCodeCliProvider end-to-end against the real
//! local `claude` binary. Sends one trivial query, expects a JSON response.
//!
//! Run from repo root:
//!   cargo run --release -p said-llm --example smoke_claude_cli

use said_llm::{
    provider_from_config, CompletionRequest, LlmConfig,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = LlmConfig::claude_cli("");
    let provider = provider_from_config(&cfg)?;
    println!("[smoke] provider: {}", provider.name());

    let req = CompletionRequest {
        system: "You are a strict JSON-only responder.".into(),
        user: "Rewrite this question 3 different ways. Question: \"What breed is my dog?\"".into(),
        cacheable_prelude: None,
        schema: json!({
            "type": "object",
            "properties": {
                "rewrites": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 3,
                    "maxItems": 3
                }
            },
            "required": ["rewrites"]
        }),
        schema_name: "rewrite_query".into(),
        max_output_tokens: 256,
        temperature: 0.2,
        json_object: false,
    };

    println!("[smoke] sending one query ...");
    let started = std::time::Instant::now();
    let resp = provider.complete(&req).await?;
    let elapsed = started.elapsed();

    println!("[smoke] elapsed: {} ms", elapsed.as_millis());
    println!("[smoke] provider returned: {}", provider.name());
    println!("[smoke] model:    {}", resp.model);
    println!(
        "[smoke] raw (first 240 chars):\n  {}",
        resp.raw.chars().take(240).collect::<String>()
    );
    println!(
        "[smoke] parsed json:\n  {}",
        serde_json::to_string_pretty(&resp.json)?
    );

    if let Some(rewrites) = resp.json.get("rewrites").and_then(|v| v.as_array()) {
        println!("[smoke] got {} rewrites:", rewrites.len());
        for (i, r) in rewrites.iter().enumerate() {
            println!("  {}. {}", i + 1, r.as_str().unwrap_or("<not-string>"));
        }
    } else {
        println!("[smoke] WARNING: response did not contain a `rewrites` array");
    }

    Ok(())
}

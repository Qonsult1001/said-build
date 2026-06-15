//! `said-orchestrate` — run the closed-loop coding orchestrator once.
//!
//! A SEPARATE process (BYO-LLM rule): it calls the user's configured LLM via
//! said-llm, drives the Claude lifecycle (plan→design→code→test→repair→learn)
//! using said-prompts + the .said brain, and gates every change with the
//! project's real build/test commands. The core .said binary never does this.
//!
//! Usage:
//!   said-orchestrate --brain <brain.said> --repo <project-root> \
//!     --task "<what to build/fix>" \
//!     --build "cargo build" [--test "cargo test"] [--max-attempts 3]
//!
//! LLM via env (model-agnostic): GROQ_API_KEY (+ GROQ_MODEL), or OPENAI_API_KEY
//! + SAID_LLM_BASE_URL + SAID_LLM_MODEL, or ANTHROPIC_API_KEY + ANTHROPIC_MODEL.

use sca_core::said_file::SaidFile;
use said_llm::LlmConfig;
use said_orchestration::{apply_change_set, steps, GateRunner};

fn arg(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

fn split_cmd(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

fn llm_config_from_env() -> Result<LlmConfig, String> {
    if std::env::var("GROQ_API_KEY").is_ok() {
        let model = std::env::var("GROQ_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".into());
        return LlmConfig::groq_from_env(&model).ok_or_else(|| "GROQ_API_KEY set but config failed".into());
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        let base = std::env::var("SAID_LLM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let model = std::env::var("SAID_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        return LlmConfig::openai_from_env(&model, &base).ok_or_else(|| "OPENAI_API_KEY set but config failed".into());
    }
    if let Ok(model) = std::env::var("ANTHROPIC_MODEL") {
        if let Some(c) = LlmConfig::anthropic_from_env(&model) { return Ok(c); }
    }
    Err("no LLM configured: set GROQ_API_KEY (+GROQ_MODEL), or OPENAI_API_KEY + \
         SAID_LLM_BASE_URL + SAID_LLM_MODEL, or ANTHROPIC_API_KEY + ANTHROPIC_MODEL".into())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let brain_path = arg("--brain").ok_or("missing --brain <path>")?;
    let repo = arg("--repo").ok_or("missing --repo <project-root>")?;
    let task = arg("--task").ok_or("missing --task <description>")?;
    let build = arg("--build").ok_or("missing --build \"<command>\"")?;
    let test = arg("--test").unwrap_or_default();
    let max_attempts: u32 = arg("--max-attempts").and_then(|s| s.parse().ok()).unwrap_or(3);

    let cfg = llm_config_from_env()?;
    let provider = said_llm::provider_from_config(&cfg).map_err(|e| format!("llm provider: {}", e))?;

    let mut brain = SaidFile::open(&brain_path).map_err(|e| format!("open brain: {}", e))?;
    brain.auto_load_encoder();

    let gate = GateRunner {
        cwd: repo.clone(),
        build: split_cmd(&build),
        test: if test.trim().is_empty() { Vec::new() } else { split_cmd(&test) },
    };
    let run_cfg = steps::RunConfig { brain_path, task: task.clone(), max_attempts, gate };

    let repo_for_apply = repo.clone();
    let apply = move |change_set: &str| apply_change_set(&repo_for_apply, change_set).map(|_| ());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {}", e))?;
    let outcome = rt.block_on(steps::run(&mut brain, provider.as_ref(), &run_cfg, apply))?;

    println!("\n=== run complete: green={} attempts={} ===", outcome.green, outcome.attempts);
    for step in &outcome.log {
        let detail = step.detail.lines().next().unwrap_or("").chars().take(120).collect::<String>();
        println!("[{}] {}", step.step, detail);
    }
    if !outcome.green {
        std::process::exit(2);
    }
    Ok(())
}

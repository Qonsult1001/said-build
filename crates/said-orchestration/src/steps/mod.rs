//! The Claude-Code lifecycle, ONE FILE PER STEP, run in a fixed order.
//!
//! This module is the single place that shows exactly which steps are always
//! followed and in what sequence. Each sub-module is one step; [`run`] is the
//! pipeline. Adding/removing/reordering a step is a visible change here.
//!
//! Order (always):
//!   1. [`plan`]   — read-only exploration + approach        (Claude: plan mode)
//!   2. [`design`] — structure consistent with conventions   (Claude: design)
//!   3. [`code`]   — surgical edits                           (Claude: code)
//!   4. [`test`]   — run the gate                             (Claude: verify)
//!   5. [`repair`] — on red, fix + re-gate (bounded loop)     (Claude: repair)
//!   6. [`learn`]  — on green, store the iteration            (Claude: memory)

pub mod plan;
pub mod design;
pub mod code;
pub mod test;
pub mod repair;
pub mod learn;

use crate::gate::GateRunner;
use crate::PhaseResult;
use said_llm::{CompletionRequest, LlmProvider};
use said_prompts::coding::{phase_prompt, CodingContext, Phase};

/// Max output tokens per LLM call. Default 32768 (big change-sets don't truncate), but
/// some provider tiers cap output-tokens-per-minute LOWER than this and 400 on the very
/// first call (e.g. Groq gpt-oss-20b on-demand = 25000 OTPM → a 32768 request is rejected
/// before any work happens). Override with SAID_MAX_OUTPUT_TOKENS to fit the tier.
pub(crate) fn max_output_tokens() -> u32 {
    std::env::var("SAID_MAX_OUTPUT_TOKENS").ok().and_then(|s| s.parse().ok()).unwrap_or(32768)
}

/// Configuration for one orchestration run.
pub struct RunConfig {
    /// Path to the `.said` brain (project memory).
    pub brain_path: String,
    /// The coding task in plain words.
    pub task: String,
    /// Max repair attempts before giving up (the loop never merges red).
    pub max_attempts: u32,
    /// The build/test gate (the sole judge of correctness).
    pub gate: GateRunner,
    /// Project root (for reading real source into the code/repair context).
    pub repo_root: String,
    /// Repo-relative files the task is likely to touch. Their ACTUAL content is
    /// injected into the code/repair prompt so the model anchors on real lines
    /// (Claude's "Read before Edit" rule). Empty = no source surfaced (the model
    /// relies on recall only, or it's a new-file task).
    pub files: Vec<String>,
}

impl RunConfig {
    pub fn new(brain_path: impl Into<String>, task: impl Into<String>, gate: GateRunner) -> Self {
        let repo_root = gate.cwd.clone();
        Self {
            brain_path: brain_path.into(),
            task: task.into(),
            max_attempts: 3,
            gate,
            repo_root,
            files: Vec::new(),
        }
    }
}

/// A record of what one step did (for the caller's transcript / audit).
#[derive(Debug, Clone)]
pub struct StepLog {
    pub step: &'static str,
    pub detail: String,
}

/// The result of a full run.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub green: bool,
    pub attempts: u32,
    pub log: Vec<StepLog>,
}

/// Run the full lifecycle in the fixed order. `apply` is the caller-supplied
/// closure that turns a code/repair step's change-set output into actual file
/// edits (kept injectable so the orchestrator stays testable and the apply
/// strategy — `said edit` anchored ops — is pluggable). Returns the outcome;
/// green ONLY when the gate passes.
pub async fn run<A>(
    brain: &mut sca_core::said_file::SaidFile,
    provider: &dyn LlmProvider,
    cfg: &RunConfig,
    mut apply: A,
) -> Result<RunOutcome, String>
where
    A: FnMut(&str) -> Result<(), String>,
{
    // Single-brain entry point: no mounted skill packs.
    run_with_skills(brain, &mut [], provider, cfg, apply).await
}

/// Like [`run`] but with read-only SKILL PACKS mounted alongside the primary `brain`.
/// Recall federates primary + packs (see [`crate::recall::best_iterations_federated`]);
/// learnings on green are written to the PRIMARY ONLY (packs stay read-only). The single-
/// brain [`run`] delegates here with an empty pack slice.
pub async fn run_with_skills<A>(
    brain: &mut sca_core::said_file::SaidFile,
    skills: &mut [sca_core::said_file::SaidFile],
    provider: &dyn LlmProvider,
    cfg: &RunConfig,
    mut apply: A,
) -> Result<RunOutcome, String>
where
    A: FnMut(&str) -> Result<(), String>,
{
    let mut log: Vec<StepLog> = Vec::new();

    // ── 0. MEMORY: transfer the LEARNING (not the literal diff) ──────────────
    // A stored fix never applies byte-for-byte across codebases (different files,
    // anchors, surrounding code). What TRANSFERS is the LEARNING — the verified
    // approach, gotchas, and a reference implementation — which the LLM ADAPTS to
    // THIS codebase. So memory TEACHES the model the known-good pattern; the gate
    // still verifies. This is the moat: .said makes a weak model succeed by
    // transferring verified learning, not by pasting stale code.
    // FAILURE-TRIGGERED RECALL (try cold, consult memory only on failure).
    // We RECALL the top-K verified learnings now (cheap, no LLM) and hold them, but
    // we do NOT inject them into the first attempt. The local model tries the task
    // COLD first — the tasks it can already do never see memory (no distraction, no
    // regression by construction). Only when the gate goes RED do we inject the
    // learning into the repair phase and loop. This mirrors "try, fail, then look it
    // up", and spends recall only where it's needed.
    let topk = crate::recall::inject_topk();
    // Federated recall: primary brain + any mounted read-only skill packs, merged+ranked.
    let recalled = crate::recall::best_iterations_federated(brain, skills, &cfg.task, topk);
    if !recalled.is_empty() {
        log.push(StepLog {
            step: "memory",
            detail: format!("recalled {} learning(s) (best {:.2}) — held for repair if the cold attempt fails",
                recalled.len(), recalled[0].score),
        });
    }

    // Build the memory-injection block ONCE (used only if the cold attempt fails).
    let memory_block: Option<String> = if recalled.is_empty() {
        None
    } else {
        let mut m = String::new();
        for (i, hit) in recalled.iter().enumerate() {
            if recalled.len() > 1 {
                m.push_str(&format!("# Candidate learning #{} of {}\n", i + 1, recalled.len()));
            }
            m.push_str(&said_prompts::coding::fill_memory_injection(hit.score, &hit.note, &hit.edits_json));
            m.push('\n');
        }
        Some(m)
    };

    // WARM-FIRST on a HIGH-CONFIDENCE match. The cold-first design (try cold, inject
    // memory only on repair) was the cause of the flaky warm result: on a task the model
    // can't do cold (h1_lru), the COLD first attempt writes the TEXTBOOK version, then the
    // model has to PATCH wrong code during repair — which often fails. When memory holds a
    // strong match for THIS shape, inject the learning into the FIRST code attempt so the
    // model writes the known-good version up front (this is what the original green run
    // did). Below the bar we stay cold-first (so easy tasks aren't distracted). Threshold
    // SAID_WARM_FIRST_MIN (default 0.60); set >1 to force always-cold.
    let warm_first_min = std::env::var("SAID_WARM_FIRST_MIN").ok()
        .and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.60);
    let warm_first = recalled.first().map(|h| h.score >= warm_first_min).unwrap_or(false);

    let src = crate::source::source_context(&cfg.repo_root, &cfg.files);
    let code_ctx = if warm_first {
        if let Some(mem) = &memory_block {
            log.push(StepLog { step: "memory", detail: format!(
                "WARM-FIRST: strong match ({:.2} >= {:.2}) — injecting learning into the FIRST code attempt",
                recalled[0].score, warm_first_min) });
            let mut c = mem.clone();
            if !src.is_empty() { c.push_str("\n\n"); c.push_str(&src); }
            c
        } else { src.clone() }
    } else { src.clone() };
    let code_ctx_opt = if code_ctx.is_empty() { None } else { Some(code_ctx.as_str()) };

    // 1. PLAN — read-only.
    let plan = plan::run(brain, provider, &cfg.task).await?;
    log.push(StepLog { step: "plan", detail: plan.output.clone() });

    // 2. DESIGN — structure/conventions.
    let design = design::run(brain, provider, &cfg.task).await?;
    log.push(StepLog { step: "design", detail: design.output.clone() });

    // 3. CODE — produce + apply the change-set (warm context when a strong match exists).
    let coded = code::run(brain, provider, &cfg.task, code_ctx_opt).await?;
    log.push(StepLog { step: "code", detail: coded.output.clone() });
    // An apply failure (bad/unsafe anchor) is REPAIRABLE, not fatal — treat it
    // like a gate failure so the repair loop fixes the anchor. `pending_failure`
    // carries an apply error into the loop's repair branch (skips the gate run
    // since nothing valid was applied).
    let mut pending_failure: Option<String> = apply(&coded.output).err().map(|e| format!("apply (code) failed: {}", e));

    // 4. TEST → 5. REPAIR loop. The gate is the sole judge.
    let mut attempts = 1u32;
    loop {
        // If a prior apply failed, that's the failure to repair; otherwise run
        // the gate.
        let outcome = match pending_failure.take() {
            Some(apply_err) => crate::gate::GateOutcome { green: false, output: apply_err, failed_step: "apply".into() },
            None => test::run(&cfg.gate),
        };
        log.push(StepLog { step: "test", detail: format!("green={} ({})", outcome.green, outcome.failed_step) });
        if outcome.green {
            // 6. LEARN — on green, the LLM authors the structured iteration note and
            //    .said stores it. SKIPPABLE via SAID_NO_LEARN: auto-storing a fresh
            //    (often weaker, LLM-authored) note on every green run POLLUTES the
            //    brain — duplicate near-identical frames accumulate and can OUTRANK the
            //    original hand-verified learning, degrading later recall. For A/B moat
            //    tests (and any run against a curated brain) we want the brain FROZEN.
            if std::env::var("SAID_NO_LEARN").is_ok() {
                log.push(StepLog { step: "learn", detail: "skipped (SAID_NO_LEARN) — brain left frozen".into() });
            } else {
                let transcript = format!(
                    "TASK: {}\n\n## Plan\n{}\n\n## Design\n{}\n\n## Code (verified change-set)\n{}\n\n## Gate\n{}",
                    cfg.task, plan.output, design.output, coded.output, outcome.output
                );
                learn::run(brain, provider, &cfg.task, &transcript, &coded.output).await?;
                log.push(StepLog { step: "learn", detail: "authored + compressed + stored iteration".into() });
            }
            return Ok(RunOutcome { green: true, attempts, log });
        }
        if attempts >= cfg.max_attempts {
            log.push(StepLog { step: "stop", detail: "max attempts reached; not merging red".into() });
            return Ok(RunOutcome { green: false, attempts, log });
        }
        // REPAIR — the cold attempt failed, so NOW consult memory: feed the gate
        // error + current source + the held verified learning. Re-read source each
        // attempt (a prior apply changed the file, so anchors must reflect the
        // CURRENT state — Claude re-reads after a write). The learning is injected on
        // EVERY repair attempt (it's the known-good approach for this failure).
        let cur_src = crate::source::source_context(&cfg.repo_root, &cfg.files);
        let mut repair_extra = format!("# Gate failure\n{}", outcome.output);
        if !cur_src.is_empty() {
            repair_extra.push_str("\n\n");
            repair_extra.push_str(&cur_src);
        }
        if let Some(mem) = &memory_block {
            if attempts == 1 {
                log.push(StepLog { step: "memory", detail: "cold attempt failed — injecting verified learning into repair".into() });
            }
            repair_extra.push_str("\n\n");
            repair_extra.push_str(mem);
        }
        let fix = repair::run(brain, provider, &cfg.task, &repair_extra).await?;
        log.push(StepLog { step: "repair", detail: fix.output.clone() });
        // A repair apply failure is also repairable — carry it into the next
        // iteration instead of aborting (bounded by max_attempts).
        pending_failure = apply(&fix.output).err().map(|e| format!("apply (repair) failed: {}", e));
        attempts += 1;
    }
}

/// Shared: build a phase prompt (said-prompts) filled with recalled memory
/// (sca-core), call the LLM (said-llm), return the model's free-form output.
/// Every step routes through here so the compose-three-pieces logic lives once.
pub(crate) async fn run_phase(
    brain: &mut sca_core::said_file::SaidFile,
    provider: &dyn LlmProvider,
    phase: Phase,
    task: &str,
    extra: Option<&str>,
) -> Result<PhaseResult, String> {
    // NOTE: memory injection is the ORCHESTRATOR's job (run()), which injects the
    // TOP-K (default 5) verified learnings via `extra` — the documented top-5 contract.
    // run_phase must NOT do its own separate top-1 recall here: that injected only the
    // single best match (inconsistent with top-5) AND duplicated/competed with the
    // memory_block run() already passes. So `context` is built purely from `extra`.
    let mut context = String::new();
    if let Some(e) = extra {
        if !e.trim().is_empty() {
            context.push_str(&format!("\n# Current attempt context\n{}\n", e.trim()));
        }
    }
    let had_ctx = !context.is_empty();
    let prompt = phase_prompt(phase, &CodingContext { task: task.to_string(), context });

    // The CODE/REPAIR phases emit a change-set ({"edits":[...]}) which apply.rs
    // consumes directly — DON'T wrap it in {"output":"..."} (two conflicting
    // shape instructions confuse the model and break apply). Prose phases
    // (plan/design/test) return {"output":"..."} and we unwrap it.
    let emits_change_set = matches!(phase, Phase::Code | Phase::Repair);
    let system = if emits_change_set {
        said_prompts::coding::SYSTEM_CHANGESET.to_string()
    } else {
        said_prompts::coding::SYSTEM_PROSE.to_string()
    };

    let req = CompletionRequest {
        system,
        user: prompt.clone(),
        cacheable_prelude: None,
        schema: serde_json::json!({
            "type": "object",
            "properties": { "output": { "type": "string" } },
            "required": ["output"],
            "additionalProperties": false
        }),
        schema_name: "phase_output".to_string(),
        max_output_tokens: max_output_tokens(),
        temperature: 0.2,
        // Permissive JSON (json_object), not strict schema (strict mode fails on
        // Groq for arbitrary code content). Proven in Advisory's GroqCycle.
        json_object: true,
    };
    let resp = provider
        .complete(&req)
        .await
        .map_err(|e| format!("llm completion ({}): {}", phase.name(), e))?;
    // resp.json is the PARSED message content (parse_body already extracted
    // choices[0].message.content and json-parsed it). Change-set phases: serialize
    // that parsed object back to a string for apply.rs (which extracts `edits`).
    // Prose phases: unwrap the `output` string.
    let output = if emits_change_set {
        serde_json::to_string(&resp.json).unwrap_or_else(|_| resp.raw.clone())
    } else {
        resp.json.get("output").and_then(|v| v.as_str()).unwrap_or(&resp.raw).to_string()
    };
    Ok(PhaseResult { phase: phase.name(), prompt, output, had_recalled_context: had_ctx })
}

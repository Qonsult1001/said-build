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
use said_orchestration::{apply_change_set, config, steps, GateRunner};

fn arg(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

fn split_cmd(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

/// Load the approved-publisher allowlist: each `*.pub` file under `~/.said/publishers/`
/// (and `$SAID_PUBLISHERS_DIR` if set) holds one Ed25519 pubkey as 64-hex-char text. This
/// is the trust root, cached from the registry's `publishers.json`. Empty = trust nobody
/// (so signed-by-unknown packs are refused). See docs/said-structure/18 Gate 3.
fn load_publisher_allowlist() -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("SAID_PUBLISHERS_DIR") { dirs.push(std::path::PathBuf::from(d)); }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        dirs.push(std::path::Path::new(&home).join(".said").join("publishers"));
    }
    for dir in dirs {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("pub") {
                    if let Ok(s) = std::fs::read_to_string(&p) {
                        let hx = s.trim().to_lowercase();
                        if hx.len() == 64 && hx.chars().all(|c| c.is_ascii_hexdigit()) {
                            set.insert(hx);
                        }
                    }
                }
            }
        }
    }
    set
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

    // LLM config: a `[llm]` TOML file (--llm-config) if given, else env vars.
    // Keys in the file use ${ENV} placeholders so they never sit in plaintext.
    let cfg = config::resolve(arg("--llm-config").as_deref())?;
    let provider = said_llm::provider_from_config(&cfg).map_err(|e| format!("llm provider: {}", e))?;

    let mut brain = SaidFile::open(&brain_path).map_err(|e| format!("open brain: {}", e))?;
    brain.auto_load_encoder();

    // ── SKILL PACKS (multi-brain mount) ──────────────────────────────────────
    // Read-only `.said` skill packs federated into recall alongside the primary brain.
    // Discovery (union; --skills wins): explicit --skills <file|dir,...>, then
    // $SAID_SKILLS_DIR, then <repo>/.said/skills, then ~/.said/skills. Each candidate is
    // opened via SaidFile::open — the header/format gate (magic/version/CRC) REJECTS
    // anything that isn't a real .said, so a junk file in the folder is skipped, logged.
    // Writes never touch packs (learn-on-green writes the primary only). See
    // docs/said-structure/18-skill-pack-linking-and-trust.md.
    let mut skills: Vec<SaidFile> = Vec::new();
    {
        let primary_canon = std::fs::canonicalize(&brain_path).ok();
        let mut roots: Vec<std::path::PathBuf> = Vec::new();
        if let Some(s) = arg("--skills") {
            for p in s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()) {
                roots.push(std::path::PathBuf::from(p));
            }
        }
        if let Ok(d) = std::env::var("SAID_SKILLS_DIR") { roots.push(std::path::PathBuf::from(d)); }
        roots.push(std::path::Path::new(&repo).join(".said").join("skills"));
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            roots.push(std::path::Path::new(&home).join(".said").join("skills"));
        }
        // Expand each root: a file → that file; a dir → its *.said. Dedup by canonical path,
        // and never mount the primary brain as its own skill pack.
        let mut seen = std::collections::HashSet::new();
        let mut pack_files: Vec<std::path::PathBuf> = Vec::new();
        for root in roots {
            let candidates: Vec<std::path::PathBuf> = if root.is_dir() {
                std::fs::read_dir(&root).into_iter().flatten().flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("said"))
                    .collect()
            } else if root.is_file() {
                vec![root]
            } else { Vec::new() };
            for c in candidates {
                let canon = std::fs::canonicalize(&c).ok();
                if canon.is_some() && canon == primary_canon { continue; } // not the primary
                let key = canon.clone().unwrap_or_else(|| c.clone());
                if !seen.insert(key) { continue; }
                pack_files.push(c);
            }
        }
        // Gate 3 (publisher trust) policy. A pack with a `.sig` sidecar MUST verify and
        // its pubkey MUST be in the approved-publisher allowlist (~/.said/publishers/*.pub,
        // cached from the registry) or it is REFUSED. A pack WITHOUT a sidecar is allowed
        // (local/self-built) unless SAID_SKILLS_STRICT=1, which requires every pack signed.
        let strict = std::env::var("SAID_SKILLS_STRICT").is_ok();
        let allowlist = load_publisher_allowlist();
        for f in pack_files {
            // Verify signature first (if present / required).
            #[cfg(feature = "pack-sign")]
            {
                let sc = sca_core::pack_sign::sidecar_path(&f);
                if sc.exists() {
                    match sca_core::pack_sign::verify_pack(&f) {
                        Ok(v) => {
                            if sca_core::pack_sign::is_approved(&v, &allowlist) {
                                eprintln!("[skills] signature OK + approved publisher {} for {}", &v.pubkey_hex[..16], f.display());
                            } else {
                                eprintln!("[skills] REFUSED {} — signed by UNKNOWN publisher {} (not in allowlist)", f.display(), &v.pubkey_hex[..16]);
                                continue;
                            }
                        }
                        Err(e) => { eprintln!("[skills] REFUSED {} — signature invalid: {}", f.display(), e); continue; }
                    }
                } else if strict {
                    eprintln!("[skills] REFUSED {} — unsigned (SAID_SKILLS_STRICT set)", f.display());
                    continue;
                }
            }
            match SaidFile::open(&f) {
                Ok(mut p) => {
                    p.auto_load_encoder();
                    eprintln!("[skills] mounted {} (read-only)", f.display());
                    skills.push(p);
                }
                Err(e) => eprintln!("[skills] skipped {} — not a valid .said ({})", f.display(), e),
            }
        }
        let _ = &allowlist; let _ = strict; // (used only under pack-sign feature)
        if !skills.is_empty() {
            eprintln!("[skills] {} skill pack(s) federated into recall", skills.len());
        }
    }

    // Files to surface into the code/repair context (Claude's "Read before
    // Edit"): comma-separated, repo-relative. Optional — without it the model
    // relies on recall only and may hallucinate anchors.
    let files: Vec<String> = arg("--files")
        .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();

    let gate = GateRunner {
        cwd: repo.clone(),
        build: split_cmd(&build),
        test: if test.trim().is_empty() { Vec::new() } else { split_cmd(&test) },
    };
    let run_cfg = steps::RunConfig {
        brain_path,
        task: task.clone(),
        max_attempts,
        gate,
        repo_root: repo.clone(),
        files,
    };

    let repo_for_apply = repo.clone();
    let apply = move |change_set: &str| apply_change_set(&repo_for_apply, change_set).map(|_| ());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {}", e))?;
    let outcome = rt.block_on(steps::run_with_skills(&mut brain, &mut skills, provider.as_ref(), &run_cfg, apply))?;

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

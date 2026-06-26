//! CLI end-to-end for the agent-steering hook (`said hook`) and registration (`said setup`).
//! Mirrors nudge's developer-guide test: build a brain, pipe a real PreToolUse JSON to `said hook`,
//! assert the decision on STDOUT. Also checks `said setup --dry-run` shape + removal-safety markers.
//!
//! Gated on `code` (the hook/setup commands are `#[cfg(feature = "code")]`). Run:
//!   cargo test -p said-cli --no-default-features --features "coding" --test test_steering_cli

#![cfg(feature = "code")]

use std::io::Write;
use std::process::{Command, Stdio};

const SAID: &str = env!("CARGO_BIN_EXE_said");

/// Run `said <args>` with `stdin_json` piped to stdin; return (stdout, stderr).
fn run_hook(brain: &str, stdin_json: &str, extra: &[&str]) -> (String, String) {
    let mut cmd = Command::new(SAID);
    cmd.args(["--path", brain, "hook"]).args(extra)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn said hook");
    child.stdin.as_mut().unwrap().write_all(stdin_json.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait said hook");
    (String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string())
}

static UNIQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn uniq_dir(prefix: &str) -> std::path::PathBuf {
    let n = UNIQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("{prefix}_{}_{n}", std::process::id()))
}

/// Build a tiny code brain in a fresh temp dir; returns (tmpdir, brain_path).
fn make_brain() -> (std::path::PathBuf, String) {
    let dir = uniq_dir("said_steer_cli");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sess.rs"),
        "fn make_session(user: u32) -> u64 { now() + 30 }\nfn validate_session(s: u64) -> bool { now() < s }\n").unwrap();
    let brain = dir.join("t.said").to_string_lossy().to_string();
    assert!(Command::new(SAID).args(["create", &brain]).output().unwrap().status.success(), "create");
    assert!(Command::new(SAID).args(["--path", &brain, "init", &dir.to_string_lossy()])
        .output().unwrap().status.success(), "init");
    (dir, brain)
}

#[test]
fn hook_provides_recall_on_user_prompt_clean_stdout() {
    let (dir, brain) = make_brain();
    // UserPromptSubmit — the TRUSTED channel (the registered default). The hook recalls .said against
    // the user's prompt and PROVIDES the result as factual project_index alongside the prompt.
    let json = r#"{"hook_event_name":"UserPromptSubmit","prompt":"where is make_session and how is session expiry handled?"}"#;
    let (stdout, _stderr) = run_hook(&brain, json, &["--agent", "claude"]);
    let _ = std::fs::remove_dir_all(&dir);

    // STDOUT must be CLEAN JSON only (the SCA load message is on stderr, must not leak here).
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("hook stdout must be valid JSON only, got:\n{stdout}\nerr: {e}"));
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
    let ctx = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("");
    assert!(ctx.contains("make_session"), "provided context must point at make_session; got:\n{ctx}");
    assert!(ctx.contains("<project_index"), "must be a factual <project_index> block (not imperative)");
}

#[test]
fn hook_passthrough_on_write_emits_nothing() {
    let (dir, brain) = make_brain();
    let json = r#"{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"x.rs","content":"fn x(){}"}}"#;
    let (stdout, _) = run_hook(&brain, json, &[]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(stdout.trim().is_empty(), "Write must passthrough (empty stdout), got:\n{stdout}");
}

#[test]
fn hook_fails_open_on_offtopic_search() {
    let (dir, brain) = make_brain();
    let json = r#"{"hook_event_name":"PreToolUse","tool_name":"Grep","tool_input":{"pattern":"kubernetes ingress TLS certificate rotation"}}"#;
    let (stdout, _) = run_hook(&brain, json, &[]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(stdout.trim().is_empty(), "off-topic search must fail open (empty stdout), got:\n{stdout}");
}

#[test]
fn setup_dry_run_shows_removal_safe_registration() {
    let dir = uniq_dir("said_setup_dry");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(SAID).args(["setup", "--dry-run"]).current_dir(&dir).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Dry-run must NOT create any files.
    let wrote_settings = dir.join(".claude/settings.local.json").exists();
    let wrote_skill = dir.join(".claude/skills/said/SKILL.md").exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "setup --dry-run must succeed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("would write .claude/skills/said/SKILL.md"), "dry-run announces the skill; got:\n{stdout}");
    assert!(stdout.contains(".claude/settings.local.json"), "registers into gitignored local settings; got:\n{stdout}");
    assert!(stdout.contains("UserPromptSubmit"), "registers a UserPromptSubmit hook (the trusted channel)");
    assert!(!wrote_settings && !wrote_skill, "dry-run must NOT write any files");
}

#[test]
fn setup_install_then_remove_is_clean() {
    let dir = uniq_dir("said_setup_rt");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // install
    let i = Command::new(SAID).args(["setup"]).current_dir(&dir).output().unwrap();
    assert!(i.status.success(), "install: {}", String::from_utf8_lossy(&i.stderr));
    let settings = dir.join(".claude/settings.local.json");
    let skill = dir.join(".claude/skills/said/SKILL.md");
    assert!(settings.exists(), "install writes settings.local.json");
    assert!(skill.exists(), "install bundles the said skill");
    // the registered hook carries our __said marker (so remove can find it) + the said binary path
    let s = std::fs::read_to_string(&settings).unwrap();
    assert!(s.contains("__said"), "hook entry must carry the __said marker");
    assert!(s.contains("hook"), "hook command invokes `said ... hook`");
    // the bundled skill body is the said-prompts SKILL_BODY (frontmatter present)
    let body = std::fs::read_to_string(&skill).unwrap();
    assert!(body.starts_with("---\nname: said\n"), "skill is the SKILL.md from said-prompts");

    // remove
    let r = Command::new(SAID).args(["setup", "--remove"]).current_dir(&dir).output().unwrap();
    assert!(r.status.success(), "remove: {}", String::from_utf8_lossy(&r.stderr));
    let settings_after = std::fs::read_to_string(&settings).unwrap_or_default();
    let skill_gone = !skill.exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!settings_after.contains("__said"), "remove strips the said hook from settings");
    assert!(skill_gone, "remove deletes the bundled skill");
}

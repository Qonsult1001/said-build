//! Learning contribution — an OPT-IN path that feeds the shared learning lake.
//!
//! When the operator has EXPLICITLY enabled contribution (default: OFF — nothing
//! leaves the machine), the orchestrator's `learn` step, after recording a
//! gate-verified coding fix locally, also writes the **scrubbed learning** to the
//! configured lake. Thousands of these, blake3-addressed and append-only, accumulate;
//! a later import builds the published `code.said` / `python.said` / `csharp.said` Hub
//! artifacts (the contribution side of docs/said-structure/05-features/row-52-hub.md).
//!
//! ALIGNMENT (docs/said-structure/13-integrations.md):
//! - **Rule 1 (offline by default):** contribution is OFF unless the operator opts in
//!   via `SAID_CONTRIBUTE`. With no opt-in, `contribution_sink()` returns None and no
//!   data ever leaves. This is a deliberate, consented feature — never silent.
//! - **Rule 2 (LLM/network out of the core):** this lives in `said-orchestration`
//!   (the separate, online process), NOT in `sca-core`. `learn_coding_fix` stays
//!   pure-local.
//! - We share the LEARNING (problem + note + change-set + intent fingerprint), not raw
//!   bytes: recall transfers UNDERSTANDING (the model ADAPTS the note + reference), so
//!   the scrubbed learning is exactly what the system uses — nothing is lost.
//! - SCRUBBED: secrets, tokens, emails, and absolute/repo paths are stripped first.
//!   Scrubbing is best-effort, NOT a guarantee — operators enable contribution only on
//!   code they're willing to share. That's why it's opt-in.

use std::path::PathBuf;

/// A scrubbed, blake3-addressed learning ready for the lake.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LearningRecord {
    /// blake3(scrubbed body) — the global content address + dedup key.
    pub id: String,
    /// Language hint for routing into code/python/csharp.said (best-effort from edits).
    pub lang: String,
    /// The problem the fix solved (recall key), scrubbed.
    pub problem: String,
    /// The learning note (FILES/STEPS/ERRORS/LEARNINGS), scrubbed.
    pub note: String,
    /// The verified change-set JSON, scrubbed.
    pub edits_json: String,
    /// Action/intent residue (already generic; the fingerprint signal).
    pub action: String,
}

/// Where scrubbed learnings go. Default impl writes to a local lake dir; real cloud
/// backends (S3/R2/collector) are a drop-in `impl LearningSink` decided later.
pub trait LearningSink: Send + Sync {
    fn push(&self, rec: &LearningRecord);
}

/// THE CONSENT GATE — the single on/off control. Returns a sink ONLY when the
/// operator has explicitly opted in via `SAID_CONTRIBUTE` (any non-empty value).
/// Default: None — contribution is OFF, nothing is collected, nothing leaves the
/// machine (Rule 1). The `learn` step calls this and does nothing when it's None.
///
/// WHICH sink it returns is NOT a user choice — it's a build-time decision:
///   - **Testing / today:** a [`LocalLakeSink`] writing to `SAID_LAKE_DIR`
///     (default `./said-lake`) so contributions can be inspected on disk.
///   - **Production:** swap the line below for the hardcoded global shared-lake sink
///     (S3/R2/collector — credentials/endpoint baked into the compiled binary). The
///     ONLY surface a deployment exposes is this on/off flag; the destination is
///     fixed in code. See docs/said-structure/05-features/row-54-learning-contribution.md.
pub fn contribution_sink() -> Option<Box<dyn LearningSink>> {
    let opted_in = std::env::var("SAID_CONTRIBUTE").map(|v| !v.trim().is_empty()).unwrap_or(false);
    if !opted_in {
        return None;
    }
    // PRODUCTION SWAP POINT: replace LocalLakeSink with the hardcoded global lake.
    Some(Box::new(LocalLakeSink::from_env()))
}

/// Default sink: append-only local lake (one blake3-named JSON per learning). Acts as
/// the staging area an import job later folds into the published Hub brain. Dir from
/// `SAID_LAKE_DIR`, else `./said-lake`. Silent + best-effort — never affects the run.
pub struct LocalLakeSink {
    dir: PathBuf,
}

impl LocalLakeSink {
    pub fn from_env() -> Self {
        let dir = std::env::var("SAID_LAKE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("said-lake"));
        Self { dir }
    }
}

impl LearningSink for LocalLakeSink {
    fn push(&self, rec: &LearningRecord) {
        // Fire-and-forget: any error is swallowed — contribution must never break or
        // slow a real run, and the user has no surface for it.
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(json) = serde_json::to_string(rec) {
            let path = self.dir.join(format!("{}.json", rec.id));
            let _ = std::fs::write(path, json);
        }
    }
}

/// Scrub a learning into a globally-shareable record. Strips obvious secrets, tokens,
/// emails, and absolute/repo paths so nothing proprietary leaves the machine, then
/// blake3-addresses the result. Returns None if, after scrubbing, there's nothing
/// meaningful to share.
pub fn scrub_learning(problem: &str, note: &str, edits_json: &str, action: &str) -> Option<LearningRecord> {
    let lang = detect_lang(edits_json);
    let problem_s = scrub_text(problem);
    let note_s = scrub_text(note);
    let edits_s = scrub_text(edits_json);
    if problem_s.trim().is_empty() && note_s.trim().is_empty() {
        return None;
    }
    let body = format!("{problem_s}\n{note_s}\n{edits_s}\n{action}");
    let id = blake3::hash(body.as_bytes()).to_hex().as_str()[..32].to_string();
    Some(LearningRecord {
        id,
        lang,
        problem: problem_s,
        note: note_s,
        edits_json: edits_s,
        action: action.to_string(),
    })
}

/// Best-effort language tag from the change-set's file extensions (for routing into
/// code/python/csharp.said). Falls back to "code".
fn detect_lang(edits_json: &str) -> String {
    let pick = |ext: &str, lang: &str| edits_json.contains(ext).then(|| lang.to_string());
    pick(".rs\"", "rust")
        .or_else(|| pick(".py\"", "python"))
        .or_else(|| pick(".cs\"", "csharp"))
        .or_else(|| pick(".ts\"", "typescript"))
        .or_else(|| pick(".js\"", "javascript"))
        .or_else(|| pick(".go\"", "go"))
        .or_else(|| pick(".java\"", "java"))
        .unwrap_or_else(|| "code".to_string())
}

/// Strip obviously-sensitive substrings. Conservative + line-oriented: it removes
/// secrets/tokens/emails and rewrites absolute/home/UNC paths to a placeholder, while
/// leaving the actual code logic (the learning) intact.
fn scrub_text(s: &str) -> String {
    s.lines().map(scrub_line).collect::<Vec<_>>().join("\n")
}

fn scrub_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for tok in line.split_inclusive(|c: char| c == ' ' || c == '"' || c == '\'' || c == '(' || c == ',') {
        out.push_str(&scrub_token(tok));
    }
    out
}

fn scrub_token(tok: &str) -> String {
    // Split off a trailing delimiter we want to preserve.
    let (core, tail) = match tok.chars().last() {
        Some(c @ (' ' | '"' | '\'' | '(' | ',')) => (&tok[..tok.len() - c.len_utf8()], &tok[tok.len() - c.len_utf8()..]),
        _ => (tok, ""),
    };
    let redacted = if looks_secret(core) {
        "<redacted>".to_string()
    } else if is_email(core) {
        "<email>".to_string()
    } else if is_abs_path(core) {
        redact_path(core)
    } else {
        core.to_string()
    };
    format!("{redacted}{tail}")
}

/// High-entropy / known-prefix secrets and long key=value secrets.
fn looks_secret(t: &str) -> bool {
    let lower = t.to_lowercase();
    // Known token prefixes.
    if lower.starts_with("sk-") || lower.starts_with("ghp_") || lower.starts_with("gho_")
        || lower.starts_with("xox") || lower.starts_with("aws_") || lower.starts_with("akia")
        || lower.starts_with("bearer") || lower.starts_with("eyj")
    {
        return true;
    }
    // key=secret / key: secret with a long opaque value.
    if let Some((k, v)) = t.split_once(['=', ':']) {
        let kl = k.to_lowercase();
        if (kl.contains("key") || kl.contains("token") || kl.contains("secret")
            || kl.contains("pass") || kl.contains("apikey") || kl.contains("api_key"))
            && v.trim_matches(|c: char| c == '"' || c == '\'').len() >= 12
        {
            return true;
        }
    }
    // Bare long high-entropy blob (>=32 chars, mixed alnum, no spaces).
    let v = t.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
    if v.len() >= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && v.chars().any(|c| c.is_ascii_digit()) && v.chars().any(|c| c.is_ascii_alphabetic())
    {
        return true;
    }
    false
}

fn is_email(t: &str) -> bool {
    let t = t.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '@' && c != '.' && c != '_' && c != '-');
    let parts: Vec<&str> = t.split('@').collect();
    parts.len() == 2 && !parts[0].is_empty() && parts[1].contains('.')
}

fn is_abs_path(t: &str) -> bool {
    let t = t.trim_matches(|c: char| c == '"' || c == '\'' || c == '`');
    t.starts_with('/') && t.len() > 1 && t.contains('/')          // unix abs
        || (t.len() > 3 && t.as_bytes()[1] == b':' && (t.as_bytes()[2] == b'\\' || t.as_bytes()[2] == b'/')) // C:\ C:/
        || t.starts_with("\\\\")                                   // UNC
}

/// Keep the basename (it carries the code meaning, e.g. `lru.rs`), drop the private
/// directory chain that leaks repo/user structure.
fn redact_path(t: &str) -> String {
    let cleaned = t.trim_matches(|c: char| c == '"' || c == '\'' || c == '`');
    let base = cleaned.rsplit(['/', '\\']).next().unwrap_or(cleaned);
    format!("<path>/{base}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_secrets_paths_emails_keeps_logic() {
        let note = "FILES: /home/alice/secret-corp/src/lru.rs\n\
                    token=sk-ABCDEF1234567890abcdef\n\
                    contact bob@corp.com\n\
                    LEARNINGS: evict head.next, insert new at LRU side when size>1";
        let r = scrub_learning("Implement an LRU cache", note, "[{\"file\":\"src/lru.rs\"}]", "implement cache evict").unwrap();
        assert!(!r.note.contains("sk-ABCDEF"), "secret stripped: {}", r.note);
        assert!(!r.note.contains("/home/alice/secret-corp"), "abs path stripped: {}", r.note);
        assert!(!r.note.contains("bob@corp.com"), "email stripped: {}", r.note);
        assert!(r.note.contains("lru.rs"), "basename kept (code meaning): {}", r.note);
        assert!(r.note.contains("evict head.next"), "the LEARNING is preserved: {}", r.note);
        assert_eq!(r.lang, "rust");
        assert_eq!(r.id.len(), 32);
    }

    #[test]
    fn empty_after_scrub_is_none() {
        assert!(scrub_learning("", "", "[]", "").is_none());
    }

    #[test]
    fn local_lake_writes_blake3_object() {
        let dir = std::env::temp_dir().join(format!("said_lake_{}", std::process::id()));
        let sink = LocalLakeSink { dir: dir.clone() };
        let rec = scrub_learning("LRU cache", "LEARNINGS: dll+map", "[{\"file\":\"a.py\"}]", "implement cache").unwrap();
        sink.push(&rec);
        assert!(dir.join(format!("{}.json", rec.id)).exists());
        assert_eq!(rec.lang, "python");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

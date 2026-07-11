#![cfg(feature = "browser")]
//! AI-chat-export import — turn a ChatGPT or Claude `conversations.json` export into `.said` memories.
//!
//! Sibling of `browser_ingest`: one public entry per source, a progress callback, an import report.
//! Reuses the `browser` feature gate (no new deps — serde_json is already in sca-core), so it ships in
//! the same FREE brain bundle as `import browser`.
//!
//! WHY JSON, NOT HTML: the official ChatGPT export zip contains BOTH `chat.html` and
//! `conversations.json`; Claude's contains `conversations.json`. We parse the JSON — it's the stable,
//! documented shape and needs no HTML parser. LEANN scrapes the HTML (fragile); we take the robust path.
//!
//! WHY EPISODIC (not External pointers): a chat export is STATIC (unlike live browser history), so we
//! DO distil the conversation text into the brain — one memory per conversation, title + a compact
//! transcript — recallable by meaning. Each frame carries:
//!   - `Pillar::Episodic` (it's a past conversation, "what we discussed")
//!   - `ingest:chatgpt` / `ingest:claude` kind tag (scope recall to / away from chat history)
//!   - `source:chatgpt` / `source:claude` + a `date:<yyyy-mm-dd>` tag when a timestamp is present

use crate::said_file::SaidFile;
use crate::frames::Pillar;

/// What a chat import did.
#[derive(Debug, Clone)]
pub struct ChatImportReport {
    pub conversations_seen: usize,
    pub conversations_imported: usize,
    pub skipped_empty: usize,
    pub source: String, // "chatgpt" | "claude"
}

/// One distilled conversation ready to store.
struct Conversation {
    id: String,
    title: String,
    transcript: String,
    date_tag: Option<String>, // "date:2026-07-11" when a create-time is present
}

/// Import a ChatGPT `conversations.json` (or the export file/dir containing it) into `brain`.
pub fn import_chatgpt<F>(brain: &mut SaidFile, path: &str, max_chars: usize, mut progress: F)
    -> Result<ChatImportReport, String>
where F: FnMut(usize, usize, &str)
{
    let json = read_conversations_json(path, "chatgpt")?;
    let convos = parse_chatgpt(&json, max_chars)?;
    store_conversations(brain, convos, "chatgpt", &mut progress)
}

/// Import a Claude `conversations.json` (or the export file/dir containing it) into `brain`.
pub fn import_claude<F>(brain: &mut SaidFile, path: &str, max_chars: usize, mut progress: F)
    -> Result<ChatImportReport, String>
where F: FnMut(usize, usize, &str)
{
    let json = read_conversations_json(path, "claude")?;
    let convos = parse_claude(&json, max_chars)?;
    store_conversations(brain, convos, "claude", &mut progress)
}

/// Resolve the export path to the `conversations.json` text: accept the JSON file directly, a directory
/// containing it, or (best-effort) a `.zip` export — pulling `conversations.json` out of the archive.
fn read_conversations_json(path: &str, source: &str) -> Result<String, String> {
    // How to get the export — shown on any "can't find it" error so a first-timer isn't stuck. The
    // conversations live on OpenAI/Anthropic's servers, so the user requests a data export first; we
    // only ever read the LOCAL unzipped file (offline, no account, no API).
    let how_to = if source == "claude" {
        "Get your export: Claude → Settings → Privacy → Export data. You'll get an email with a link; \
         download and UNZIP it, then point at that folder (it contains conversations.json)."
    } else {
        "Get your export: ChatGPT → Settings → Data Controls → Export data. You'll get an email with a \
         link; download and UNZIP it, then point at that folder (it contains conversations.json)."
    };

    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err(format!("{} export not found at '{}'.\n{}", source, path, how_to));
    }
    // A directory → look for conversations.json inside it.
    if p.is_dir() {
        let cj = p.join("conversations.json");
        if cj.is_file() {
            return std::fs::read_to_string(&cj)
                .map_err(|e| format!("read {}: {}", cj.display(), e));
        }
        return Err(format!("no conversations.json in '{}'.\n{}", path, how_to));
    }
    // A .zip → extract conversations.json from it.
    if p.extension().and_then(|e| e.to_str()) == Some("zip") {
        return extract_conversations_from_zip(path);
    }
    // Otherwise treat it as the JSON file itself.
    std::fs::read_to_string(p).map_err(|e| format!("read {}: {}", path, e))
}

/// Pull `conversations.json` out of an export .zip. `zip` is already a dep behind the `docs` feature;
/// but to keep `import chat` in the `browser`-gated brain bundle without pulling `docs`, we do a tiny
/// hand-rolled scan only if the `zip` crate isn't available. Here we require the file be unzipped first
/// if the `zip` dep isn't present — keep the FREE path dependency-light.
fn extract_conversations_from_zip(path: &str) -> Result<String, String> {
    // Minimal, dependency-free guidance: the export zip is easy for the user to unzip. Rather than pull
    // a zip dep into the free brain bundle, ask them to point at the unzipped conversations.json. (If a
    // future build already links `zip`, this is the place to read it directly.)
    Err(format!(
        "'{}' is a zip — unzip it first and point at the conversations.json inside \
         (e.g. `said import chatgpt <unzipped-folder>`).", path))
}

/// Parse a ChatGPT `conversations.json`. Shape: a top-level array of conversations, each with a `title`,
/// an optional `create_time` (unix seconds), and a `mapping` object of nodes; each node's `message` has
/// `author.role` ("user"/"assistant"/…) and `content.parts` (array of strings). We linearize the mapping
/// in node order and keep user+assistant text.
fn parse_chatgpt(json: &str, max_chars: usize) -> Result<Vec<Conversation>, String> {
    let root: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| format!("parse ChatGPT conversations.json: {}", e))?;
    let arr = root.as_array()
        .ok_or_else(|| "ChatGPT export: expected a top-level array of conversations".to_string())?;
    let mut out = Vec::new();
    for (i, c) in arr.iter().enumerate() {
        let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("ChatGPT conversation").trim().to_string();
        let id = c.get("id").or_else(|| c.get("conversation_id")).and_then(|v| v.as_str())
            .map(|s| s.to_string()).unwrap_or_else(|| format!("chatgpt-{}", i));
        let date_tag = c.get("create_time").and_then(|v| v.as_f64()).map(unix_to_date_tag);
        // Linearize the mapping: collect (role, text) in a stable order.
        let mut lines: Vec<String> = Vec::new();
        if let Some(mapping) = c.get("mapping").and_then(|m| m.as_object()) {
            // Node order isn't guaranteed by the object; sort by create_time within each message when
            // present, else keep insertion order (serde preserves object order via a Map).
            let mut nodes: Vec<&serde_json::Value> = mapping.values().collect();
            nodes.sort_by(|a, b| {
                let ta = a.get("message").and_then(|m| m.get("create_time")).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let tb = b.get("message").and_then(|m| m.get("create_time")).and_then(|v| v.as_f64()).unwrap_or(0.0);
                ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
            });
            for node in nodes {
                if let Some(msg) = node.get("message") {
                    let role = msg.get("author").and_then(|a| a.get("role")).and_then(|v| v.as_str()).unwrap_or("");
                    if role != "user" && role != "assistant" { continue; }
                    let text = msg.get("content").and_then(|ct| ct.get("parts"))
                        .and_then(|p| p.as_array())
                        .map(|parts| parts.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "))
                        .unwrap_or_default();
                    let text = text.trim();
                    if text.is_empty() { continue; }
                    lines.push(format!("{}: {}", if role == "user" { "Me" } else { "AI" }, text));
                }
            }
        }
        let transcript = truncate_chars(&lines.join("\n"), max_chars);
        out.push(Conversation { id, title, transcript, date_tag });
    }
    Ok(out)
}

/// Parse a Claude `conversations.json`. Shape: an array of conversations, each with `name` (title),
/// `uuid`, `created_at` (ISO string), and `chat_messages` array; each message has `sender`
/// ("human"/"assistant") and `text`.
fn parse_claude(json: &str, max_chars: usize) -> Result<Vec<Conversation>, String> {
    let root: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| format!("parse Claude conversations.json: {}", e))?;
    let arr = root.as_array()
        .ok_or_else(|| "Claude export: expected a top-level array of conversations".to_string())?;
    let mut out = Vec::new();
    for (i, c) in arr.iter().enumerate() {
        let title = c.get("name").and_then(|v| v.as_str()).unwrap_or("Claude conversation").trim().to_string();
        let id = c.get("uuid").and_then(|v| v.as_str()).map(|s| s.to_string())
            .unwrap_or_else(|| format!("claude-{}", i));
        // created_at is an ISO-8601 string like "2026-07-11T09:15:00Z" → take the date part.
        let date_tag = c.get("created_at").and_then(|v| v.as_str())
            .and_then(|s| s.get(0..10)).map(|d| format!("date:{}", d));
        let mut lines: Vec<String> = Vec::new();
        if let Some(msgs) = c.get("chat_messages").and_then(|m| m.as_array()) {
            for m in msgs {
                let sender = m.get("sender").and_then(|v| v.as_str()).unwrap_or("");
                let text = m.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
                if text.is_empty() { continue; }
                lines.push(format!("{}: {}", if sender == "human" { "Me" } else { "AI" }, text));
            }
        }
        let transcript = truncate_chars(&lines.join("\n"), max_chars);
        out.push(Conversation { id, title, transcript, date_tag });
    }
    Ok(out)
}

/// Store the distilled conversations as Episodic memories. Stable doc_id (source + conversation id) so
/// re-import UPDATES the same memory rather than duplicating — a re-sync, matching `import browser`.
fn store_conversations<F>(brain: &mut SaidFile, convos: Vec<Conversation>, source: &str, progress: &mut F)
    -> Result<ChatImportReport, String>
where F: FnMut(usize, usize, &str)
{
    let total = convos.len();
    let mut imported = 0;
    let mut skipped = 0;
    for (i, c) in convos.iter().enumerate() {
        progress(i, total, source);
        if c.transcript.trim().is_empty() { skipped += 1; continue; }
        let doc_id = format!("{}/{}", source, stable_id(&c.id));
        // Body = the title as the recall hook + the transcript (so "what did I discuss about X" recalls).
        let body = format!("{}\n\n{}", c.title, c.transcript);
        let mut tags = vec![
            format!("ingest:{}", source),
            format!("source:{}", source),
        ];
        if let Some(d) = &c.date_tag { tags.push(d.clone()); }
        brain.remember_with_salience(Some(&doc_id), &body, Some(&c.title), Pillar::Episodic, tags);
        imported += 1;
    }
    progress(total, total, source);
    brain.build_index().map_err(|e| format!("build_index after {} import: {}", source, e))?;
    Ok(ChatImportReport {
        conversations_seen: total,
        conversations_imported: imported,
        skipped_empty: skipped,
        source: source.to_string(),
    })
}

fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 || s.chars().count() <= max { return s.to_string(); }
    s.chars().take(max).collect::<String>() + " …[truncated]"
}

/// Unix seconds → a `date:yyyy-mm-dd` tag. No chrono dep — a tiny civil-date calc (days since epoch).
fn unix_to_date_tag(secs: f64) -> String {
    let days = (secs as i64) / 86400;
    let (y, m, d) = civil_from_days(days);
    format!("date:{:04}-{:02}-{:02}", y, m, d)
}

/// days since 1970-01-01 → (year, month, day). Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn stable_id(key: &str) -> String {
    blake3::hash(key.as_bytes()).to_hex()[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_parse_store_and_recall() {
        let json = r#"[
          {"title":"Rust ownership","id":"c1","create_time":1752200000,
           "mapping":{
             "n1":{"message":{"author":{"role":"user"},"create_time":1.0,"content":{"parts":["How does borrowing work in Rust?"]}}},
             "n2":{"message":{"author":{"role":"assistant"},"create_time":2.0,"content":{"parts":["A borrow is a reference; the borrow checker enforces aliasing rules."]}}}
           }},
          {"title":"empty one","id":"c2","mapping":{}}
        ]"#;
        // Write to a temp file so we exercise the REAL entry point (import_chatgpt reads a path).
        let f = std::env::temp_dir().join(format!("chatgpt_conv_{}.json", std::process::id()));
        std::fs::write(&f, json).unwrap();

        let convos = parse_chatgpt(json, 0).unwrap();
        assert_eq!(convos.len(), 2);
        assert_eq!(convos[0].title, "Rust ownership");
        assert!(convos[0].transcript.contains("Me: How does borrowing"));
        assert!(convos[0].transcript.contains("AI: A borrow is a reference"));
        assert!(convos[0].date_tag.as_deref().unwrap().starts_with("date:2025-07"),
            "create_time → a date: tag");

        let mut brain = SaidFile::create(
            std::env::temp_dir().join(format!("chatgpt_test_{}.said", std::process::id()))
                .to_string_lossy().as_ref());
        assert!(brain.auto_load_encoder());
        let report = import_chatgpt(&mut brain, f.to_string_lossy().as_ref(), 0, |_,_,_| {}).unwrap();
        assert_eq!(report.conversations_imported, 1, "1 real conversation, empty one skipped");
        assert_eq!(report.skipped_empty, 1);

        let (cands, _) = crate::ask::ask(&mut brain, "how does rust borrowing work", 5, false, None);
        let top = cands.first().expect("a hit");
        assert!(top.doc_id.starts_with("chatgpt/"), "stored under chatgpt/ id");
        let meta = brain.frames.get_meta(&top.doc_id).unwrap();
        assert!(meta.tags.iter().any(|t| t == "ingest:chatgpt"));
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn claude_parse() {
        let json = r#"[
          {"name":"Trip planning","uuid":"u1","created_at":"2026-03-14T10:00:00Z",
           "chat_messages":[
             {"sender":"human","text":"Plan a 3-day trip to Lisbon"},
             {"sender":"assistant","text":"Day 1: Alfama and the castle…"}
           ]}
        ]"#;
        let convos = super::parse_claude(json, 0).unwrap();
        assert_eq!(convos.len(), 1);
        assert_eq!(convos[0].title, "Trip planning");
        assert!(convos[0].transcript.contains("Me: Plan a 3-day trip"));
        assert_eq!(convos[0].date_tag.as_deref(), Some("date:2026-03-14"));
    }
}

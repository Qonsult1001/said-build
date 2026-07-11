#![cfg(feature = "browser")]
//! Local mail-file import — turn an `.mbox` mailbox or a folder of Apple Mail `.emlx` files into
//! `.said` memories. The third personal-data importer, sibling of `browser_ingest` and `chat_import`.
//!
//! ZERO-AUTH, OFFLINE, LOCAL ONLY — by design. This reads mail that already sits on the user's disk:
//!   • `.mbox` — the portable mailbox format. Thunderbird stores this; and crucially **Google Takeout
//!     (Gmail) and a Microsoft/Outlook export both produce `.mbox`**, so this one reader covers Gmail
//!     and M365 *without any OAuth* — the user exports to a file, we read the file.
//!   • `.emlx` — Apple Mail's per-message format under `~/Library/Mail/**/Messages/*.emlx`.
//! It does NOT talk to Gmail/Graph APIs (that is a live-server connector requiring OAuth, which belongs
//! in the authenticated WASM interface, not this offline binary). See
//! docs/said-structure/06-ingestion-plugins/personal-import.md.
//!
//! Port of LEANN's `MboxReader` / `EmlxReader` (apps/email_data), rewritten dependency-free: an email is
//! RFC-822 text (headers, blank line, body), so we parse the handful of headers we need (Date/From/To/
//! Subject) and the first text/plain body part in plain Rust — no `mail-parser` dep, no new feature.
//!
//! Store model: DISTILLED Episodic memory per message (like a chat export, not a live pointer — a local
//! mbox message has no stable live URI). Each frame carries:
//!   - `Pillar::Episodic` (a past message, "what was said")
//!   - `ingest:email` kind tag + `source:mbox` / `source:emlx`
//!   - `from:<addr>` (filterable sender), `date:<yyyy-mm-dd>` when the Date header parses
//!   - `recency:<rank>` (rank 1 = most recent message) so "what was the last email" ranks by wall-clock,
//!     reusing the same temporal-recall path as `import browser`.

use crate::said_file::SaidFile;
use crate::frames::Pillar;

/// What an email import did.
#[derive(Debug, Clone)]
pub struct EmailIngestReport {
    pub messages_seen: usize,
    pub messages_imported: usize,
    pub skipped_empty: usize,
    pub source: String, // "mbox" | "emlx"
}

/// One distilled message ready to store.
struct Message {
    /// Stable key for the doc_id (Message-ID header if present, else a hash of from+subject+date).
    key: String,
    subject: String,
    from: String,
    to: String,
    date_raw: String,
    body: String,
    /// Unix seconds parsed from the Date header, for recency ordering (None if unparseable).
    epoch: Option<i64>,
}

/// Import a local mail file/folder into `brain`. Auto-detects `.mbox` (a mailbox file) vs a directory of
/// `.emlx` files (Apple Mail). `max` caps the number of messages (0 = all). Newest messages rank first.
pub fn import_email<F>(brain: &mut SaidFile, path: &str, max: usize, max_chars: usize, mut progress: F)
    -> Result<EmailIngestReport, String>
where F: FnMut(usize, usize, &str)
{
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err(format!(
            "Mail path not found: {path}\n\
             Point `import email` at a local mail file or folder:\n  \
             • a Thunderbird / Google Takeout / Outlook export `.mbox` file\n  \
             • an Apple Mail folder containing `.emlx` files (e.g. ~/Library/Mail/…/Messages)\n\
             (Gmail: Google Takeout → Mail → download the .mbox. Outlook/M365: export/save as .mbox or\n \
             use a folder of saved .eml/.emlx. This importer is offline — it never logs into your account.)"
        ));
    }

    let (mut messages, source) = if p.is_dir() {
        (read_emlx_dir(p, max)?, "emlx")
    } else {
        // A file: treat as mbox (the common case; .eml single-message is also handled as a 1-entry mbox).
        (read_mbox(p, max)?, "mbox")
    };

    // Recency ordering: sort newest-first by parsed Date (messages with no parseable date sink to the end
    // but keep their file order among themselves), then stamp recency:<rank>.
    messages.sort_by(|a, b| b.epoch.unwrap_or(i64::MIN).cmp(&a.epoch.unwrap_or(i64::MIN)));

    store_messages(brain, messages, source, max_chars, &mut progress)
}

/// Parse an `.mbox` file into messages. Mbox = messages concatenated, each starting with a `From ` line
/// (the "From_" separator) at column 0. We split on those, then RFC-822-parse each chunk.
fn read_mbox(path: &std::path::Path, max: usize) -> Result<Vec<Message>, String> {
    let raw = std::fs::read(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let text = String::from_utf8_lossy(&raw);
    let mut out = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let flush = |chunk: &str, out: &mut Vec<Message>| {
        let t = chunk.trim();
        if !t.is_empty() {
            if let Some(m) = parse_rfc822(t) { out.push(m); }
        }
    };
    for line in text.lines() {
        // A new message begins at a line starting with "From " at column 0 (the mbox separator).
        if line.starts_with("From ") {
            if started {
                flush(&current, &mut out);
                if max > 0 && out.len() >= max { return Ok(out); }
                current.clear();
            }
            started = true;
            continue; // the separator line itself is not part of the message
        }
        if started {
            current.push_str(line);
            current.push('\n');
        }
    }
    flush(&current, &mut out);
    if max > 0 && out.len() > max { out.truncate(max); }
    Ok(out)
}

/// Walk a directory for Apple Mail `.emlx` files and parse each. An `.emlx` is a length-prefix line, then
/// the RFC-822 message (then a plist trailer we ignore). We strip the first line and parse the rest.
fn read_emlx_dir(dir: &std::path::Path, max: usize) -> Result<Vec<Message>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = match std::fs::read_dir(&d) { Ok(e) => e, Err(_) => continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                // Skip hidden dirs (matches LEANN).
                if p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(false) {
                    continue;
                }
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|x| x.to_str()) != Some("emlx") { continue; }
            let raw = match std::fs::read(&p) { Ok(r) => r, Err(_) => continue };
            let text = String::from_utf8_lossy(&raw);
            // Drop the leading byte-count line; parse the remainder.
            let msg_text = match text.split_once('\n') { Some((_, rest)) => rest, None => &text };
            if let Some(m) = parse_rfc822(msg_text.trim()) {
                out.push(m);
                if max > 0 && out.len() >= max { return Ok(out); }
            }
        }
    }
    Ok(out)
}

/// Parse a single RFC-822 message: unfold headers, pull Date/From/To/Subject/Message-ID, then take the
/// body (first text/plain part for multipart, else the whole body). Dependency-free — good enough for
/// recall (we don't need MIME-perfect rendering, we need the words).
fn parse_rfc822(text: &str) -> Option<Message> {
    // Split headers from body at the first blank line.
    let (header_block, body_block) = match text.split_once("\n\n") {
        Some((h, b)) => (h, b),
        None => (text, ""), // headers only, no body
    };

    // Unfold continuation lines (a header value line beginning with space/tab continues the prior header).
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in header_block.lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && !headers.is_empty() {
            let last = headers.last_mut().unwrap();
            last.1.push(' ');
            last.1.push_str(line.trim());
        } else if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());

    let subject = get("subject").unwrap_or_else(|| "(no subject)".to_string());
    let from = get("from").unwrap_or_else(|| "(unknown)".to_string());
    let to = get("to").unwrap_or_default();
    let date_raw = get("date").unwrap_or_default();
    let message_id = get("message-id").unwrap_or_default();
    let epoch = parse_email_date(&date_raw);

    // Body: for a multipart message, grab the text/plain section; otherwise the whole body. We do a cheap
    // boundary scan rather than a full MIME parse — we want the readable text, not perfect structure.
    let body = extract_text_body(body_block);

    // Skip truly empty messages (no subject AND no body) — matches the LEANN reader's guard.
    if body.trim().is_empty() && subject == "(no subject)" {
        return None;
    }

    // Stable key: prefer Message-ID; else a hash of the identifying headers (so re-import dedups).
    let key = if !message_id.is_empty() {
        message_id
    } else {
        format!("{from}|{subject}|{date_raw}")
    };

    Some(Message { key, subject, from, to, date_raw, body, epoch })
}

/// Best-effort text body: if the body looks multipart (a `text/plain` part exists), return that part's
/// text; otherwise return the whole body. No decoding of quoted-printable/base64 — recall tolerates it.
fn extract_text_body(body: &str) -> String {
    // Look for a text/plain content-type marker; if found, take from just after its following blank line
    // up to the next boundary marker ("--").
    if let Some(idx) = body.to_ascii_lowercase().find("content-type: text/plain") {
        let after = &body[idx..];
        if let Some((_, rest)) = after.split_once("\n\n") {
            // Cut at the next MIME boundary line ("--…").
            let end = rest.find("\n--").unwrap_or(rest.len());
            return rest[..end].trim().to_string();
        }
    }
    body.trim().to_string()
}

/// Parse an RFC-2822 Date header ("Wed, 09 Jun 2026 14:20:00 +0000") to Unix seconds. Minimal hand
/// parser — enough for a `date:` tag and recency ordering; returns None on anything it can't read.
fn parse_email_date(s: &str) -> Option<i64> {
    // Strip an optional leading "Day, " prefix.
    let s = s.trim();
    let s = match s.split_once(", ") { Some((_, rest)) => rest, None => s };
    let mut it = s.split_whitespace();
    let day: i64 = it.next()?.parse().ok()?;
    let mon = month_num(it.next()?)?;
    let year: i64 = {
        let y: i64 = it.next()?.parse().ok()?;
        if y < 100 { if y < 70 { 2000 + y } else { 1900 + y } } else { y }
    };
    let time = it.next().unwrap_or("00:00:00");
    let mut tp = time.split(':');
    let hh: i64 = tp.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mm: i64 = tp.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let ss: i64 = tp.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    // days since epoch via civil→days (inverse of the civil_from_days used elsewhere).
    let days = days_from_civil(year, mon, day);
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

fn month_num(m: &str) -> Option<i64> {
    Some(match &m.to_ascii_lowercase()[..m.len().min(3)] {
        "jan" => 1, "feb" => 2, "mar" => 3, "apr" => 4, "may" => 5, "jun" => 6,
        "jul" => 7, "aug" => 8, "sep" => 9, "oct" => 10, "nov" => 11, "dec" => 12,
        _ => return None,
    })
}

/// (year, month, day) → days since 1970-01-01. Howard Hinnant's days-from-civil.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Unix seconds → "yyyy-mm-dd". Howard Hinnant's civil-from-days.
fn unix_to_date(secs: i64) -> String {
    let z = secs / 86400 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Extract a bare email address from a From/To header value ("Alice <a@x.com>" → "a@x.com").
fn bare_addr(v: &str) -> String {
    if let (Some(a), Some(b)) = (v.find('<'), v.find('>')) {
        if a < b { return v[a + 1..b].trim().to_ascii_lowercase(); }
    }
    v.trim().to_ascii_lowercase()
}

/// Store distilled messages as Episodic memories. Stable doc_id (source + message key) → re-import
/// UPDATES rather than duplicating. `recency:<rank>` (1 = newest) drives temporal recall.
fn store_messages<F>(brain: &mut SaidFile, messages: Vec<Message>, source: &str, max_chars: usize, progress: &mut F)
    -> Result<EmailIngestReport, String>
where F: FnMut(usize, usize, &str)
{
    let total = messages.len();
    let mut imported = 0;
    let mut skipped = 0;
    for (i, m) in messages.iter().enumerate() {
        progress(i, total, source);
        if m.body.trim().is_empty() && m.subject == "(no subject)" { skipped += 1; continue; }

        let doc_id = format!("email/{}", stable_id(&m.key));
        let body = truncate_chars(
            &format!("Subject: {}\nFrom: {}\nTo: {}\nDate: {}\n\n{}",
                m.subject, m.from, m.to, m.date_raw, m.body),
            max_chars);

        let mut tags = vec![
            "ingest:email".to_string(),
            format!("source:{}", source),
            format!("from:{}", bare_addr(&m.from)),
        ];
        if let Some(e) = m.epoch {
            // `sent_at:<unix_secs>` — the ABSOLUTE timestamp, the GLOBAL recency sort key (same role as
            // browser `visited_at:`). "What was my last email" ranks by this across EVERY imported mbox /
            // account, so a message from yesterday beats a years-old one no matter the import order. The
            // per-import `recency:` ordinal restarts at 1 each import and is NOT globally comparable.
            tags.push(format!("sent_at:{}", e));
            tags.push(format!("date:{}", unix_to_date(e)));
        }
        // recency:<rank>, 1 = most recent WITHIN this import (kept for display; global order uses sent_at:).
        tags.push(format!("recency:{}", i + 1));

        brain.remember_with_salience(Some(&doc_id), &body, Some(&m.subject), Pillar::Episodic, tags);
        imported += 1;
    }
    progress(total, total, source);
    brain.build_index().map_err(|e| format!("build_index after email import: {}", e))?;
    Ok(EmailIngestReport {
        messages_seen: total,
        messages_imported: imported,
        skipped_empty: skipped,
        source: source.to_string(),
    })
}

fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 || s.chars().count() <= max { return s.to_string(); }
    s.chars().take(max).collect::<String>() + " …[truncated]"
}

fn stable_id(key: &str) -> String {
    blake3::hash(key.as_bytes()).to_hex()[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MBOX: &str = "From alice@example.com Wed Jun 09 14:20:00 2026\n\
Date: Wed, 09 Jun 2026 14:20:00 +0000\n\
From: Alice <alice@example.com>\n\
To: Bob <bob@example.com>\n\
Subject: Quarterly budget review\n\
Message-ID: <msg-1@example.com>\n\
\n\
Hi Bob, the Q2 budget numbers are ready. Net margin improved 4 points.\n\
\n\
From bob@example.com Thu Jul 10 09:00:00 2026\n\
Date: Thu, 10 Jul 2026 09:00:00 +0000\n\
From: Bob <bob@example.com>\n\
To: Alice <alice@example.com>\n\
Subject: Re: Quarterly budget review\n\
Message-ID: <msg-2@example.com>\n\
\n\
Thanks Alice — approving the marketing spend for next quarter.\n";

    #[test]
    fn mbox_date_parse() {
        let e = parse_email_date("Wed, 09 Jun 2026 14:20:00 +0000").unwrap();
        assert_eq!(unix_to_date(e), "2026-06-09");
    }

    #[test]
    fn bare_addr_extract() {
        assert_eq!(bare_addr("Alice <alice@example.com>"), "alice@example.com");
        assert_eq!(bare_addr("plain@example.com"), "plain@example.com");
    }

    #[test]
    fn ingest_mbox_end_to_end() {
        let mbox = std::env::temp_dir().join(format!("said_email_test_{}.mbox", std::process::id()));
        std::fs::write(&mbox, SAMPLE_MBOX).unwrap();

        let mut brain = SaidFile::create(
            std::env::temp_dir().join(format!("said_email_brain_{}.said", std::process::id()))
                .to_string_lossy().as_ref());
        assert!(brain.auto_load_encoder());

        let report = import_email(&mut brain, mbox.to_string_lossy().as_ref(), 0, 0, |_,_,_| {}).unwrap();
        assert_eq!(report.messages_seen, 2, "two messages in the mbox");
        assert_eq!(report.messages_imported, 2);

        // Recall BY MEANING: the budget email comes back for a paraphrase.
        let (cands, _) = crate::ask::ask(&mut brain, "the email about our quarterly finances", 5, false, None);
        let top = cands.first().expect("a hit");
        assert!(top.doc_id.starts_with("email/"), "stored under email/ id");
        let meta = brain.frames.get_meta(&top.doc_id).unwrap();
        assert!(meta.tags.iter().any(|t| t == "ingest:email"), "tagged ingest:email");
        assert!(meta.tags.iter().any(|t| t.starts_with("from:")), "carries a from: tag");
        assert!(meta.tags.iter().any(|t| t.starts_with("date:")), "carries a date: tag");
        assert!(meta.tags.iter().any(|t| t.starts_with("sent_at:")),
            "carries the ABSOLUTE epoch tag (the global recency sort key)");

        // TEMPORAL: "what was the last email" ranks the NEWEST (Jul 10 > Jun 09) first — recency:1.
        let (recent, _) = crate::ask::ask(&mut brain, "what was the last email i received?", 5, false, None);
        let lead = recent.first().expect("a recency-ranked hit");
        let lead_meta = brain.frames.get_meta(&lead.doc_id).unwrap();
        assert!(lead_meta.tags.iter().any(|t| t == "recency:1"),
            "newest email (Jul 10) leads the temporal query");
        assert!(lead.content.contains("marketing spend"), "the Jul 10 reply is the most recent");

        let _ = std::fs::remove_file(&mbox);
    }

    /// Cross-mbox global recency: importing an OLD mailbox AFTER a NEW one must still rank the newest
    /// message first (sort by absolute `sent_at:`, not the per-import `recency:` ordinal). This is the
    /// email twin of the browser cross-profile bug.
    #[test]
    fn global_recency_across_mailboxes() {
        let pid = std::process::id();
        let old_mbox_text = "From a@x.com Mon Jan 05 10:00:00 2019\n\
Date: Mon, 05 Jan 2019 10:00:00 +0000\nFrom: Old <old@x.com>\nSubject: Ancient thread\n\
Message-ID: <old-1@x.com>\n\nThis is a very old message from years ago.\n";
        let new_mbox_text = "From b@y.com Fri Jul 10 09:00:00 2026\n\
Date: Fri, 10 Jul 2026 09:00:00 +0000\nFrom: New <new@y.com>\nSubject: Fresh thread\n\
Message-ID: <new-1@y.com>\n\nThis message arrived just yesterday.\n";
        let old = std::env::temp_dir().join(format!("said_old_{}.mbox", pid));
        let new = std::env::temp_dir().join(format!("said_new_{}.mbox", pid));
        std::fs::write(&old, old_mbox_text).unwrap();
        std::fs::write(&new, new_mbox_text).unwrap();

        let mut brain = SaidFile::create(
            std::env::temp_dir().join(format!("said_mbox_recency_{}.said", pid)).to_string_lossy().as_ref());
        assert!(brain.auto_load_encoder());

        // NEW first, then OLD (the order that used to make the stale message win on the per-batch ordinal).
        import_email(&mut brain, new.to_string_lossy().as_ref(), 0, 0, |_,_,_| {}).unwrap();
        import_email(&mut brain, old.to_string_lossy().as_ref(), 0, 0, |_,_,_| {}).unwrap();

        let (recent, _) = crate::ask::ask(&mut brain, "what was the last email i received?", 5, false, None);
        let lead = recent.first().expect("a recency-ranked hit");
        assert!(lead.content.contains("yesterday") || lead.content.contains("Fresh"),
            "global recency must return the 2026 message, NOT the 2019 one (got: {})",
            lead.content.lines().next().unwrap_or(""));

        for p in [old, new] { let _ = std::fs::remove_file(&p); }
    }

    /// Source routing: a "last WEBSITE" query must not return an email that merely has a newer date, and
    /// "last EMAIL" must not return a browser page. Guards the cross-source leak found in real testing.
    #[test]
    fn temporal_query_respects_source() {
        let pid = std::process::id();
        // An email dated LATER (Jul 10) than the browser page (Jul 03) — the newer date must NOT let it
        // win a browser-scoped query.
        let mbox = std::env::temp_dir().join(format!("said_route_{}.mbox", pid));
        std::fs::write(&mbox,
            "From b@y.com Fri Jul 10 09:00:00 2026\n\
Date: Fri, 10 Jul 2026 09:00:00 +0000\nFrom: Bob <bob@y.com>\nSubject: Newest email thread\n\
Message-ID: <route-1@y.com>\n\nThis email is the single newest dated item.\n").unwrap();
        let db = std::env::temp_dir().join(format!("said_route_{}.db", pid));
        let _ = std::fs::remove_file(&db);
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            // 13380000000000000 µs ≈ 2025 — clearly a real browser page, but OLDER than the Jul-2026 email.
            c.execute_batch(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER);
                 INSERT INTO urls(url,title,visit_count,typed_count,last_visit_time) VALUES
                   ('https://route.example/page','A browsed web page',3,1,13380000000000000);").unwrap();
        }
        let mut brain = SaidFile::create(
            std::env::temp_dir().join(format!("said_route_brain_{}.said", pid)).to_string_lossy().as_ref());
        assert!(brain.auto_load_encoder());
        crate::browser_ingest::ingest_browser_history(&mut brain, &db.to_string_lossy(), "Chrome/Default", 0, 1, 0, |_,_,_| {}).unwrap();
        import_email(&mut brain, &mbox.to_string_lossy(), 0, 0, |_,_,_| {}).unwrap();

        // "last WEBSITE" → the browser page leads, NOT the newer-dated email.
        let (web, _) = crate::ask::ask(&mut brain, "what was the last website i visited?", 5, false, None);
        assert!(web.first().map(|c| c.doc_id.starts_with("browser/")).unwrap_or(false),
            "browser-scoped temporal query must lead with a browser page, got {:?}",
            web.first().map(|c| &c.doc_id));
        // "last EMAIL" → the email leads.
        let (mail, _) = crate::ask::ask(&mut brain, "what was the last email i received?", 5, false, None);
        assert!(mail.first().map(|c| c.doc_id.starts_with("email/")).unwrap_or(false),
            "email-scoped temporal query must lead with an email, got {:?}",
            mail.first().map(|c| &c.doc_id));

        let _ = std::fs::remove_file(&mbox);
        let _ = std::fs::remove_file(&db);
    }
}

#![cfg(feature = "browser")]
//! Browser-history ingest — turn a Chrome/Edge `History` SQLite DB into searchable `.said` memories.
//!
//! Mirrors the `document_ingest` / `whisper_ingest` shape: one public `ingest_browser_history` entry
//! point, a progress callback, and an `IngestReport`-style summary. Gated behind the `browser` feature
//! (rusqlite is a C lib) so it is NEVER on the WASM/offline build path — exactly how `whisper` is gated.
//!
//! WHY EXTERNAL POINTERS (not embedded content): browser history is a LIVE source — the browser DB
//! keeps changing. We do not copy page bodies into the brain (they'd go stale and bloat it). Instead we
//! index a lightweight, searchable summary (the page TITLE + a fragment of the URL) and store the URL as
//! an `external:uri`. Recall finds the entry by meaning; the agent then opens the live URL. This is the
//! same model LEANN uses for live data (arXiv 2506.08276) and the same `remember_as_external_pointer`
//! primitive `.said` already exposes for any live source.
//!
//! Each ingested frame carries:
//!   - `Pillar::External` (it's a pointer to an outside resource, not embedded content)
//!   - `external:pointer` + `external:uri=<url>` tags (written by remember_as_external_pointer)
//!   - `ingest:browser` kind tag (so recall can scope to / away from browsing history)
//!   - `visits:<n>` tag (visit_count — a salience hint: pages you returned to matter more)

use crate::said_file::SaidFile;

/// What `ingest_browser_history` did.
#[derive(Debug, Clone)]
pub struct BrowserIngestReport {
    pub entries_seen: usize,
    pub entries_ingested: usize,
    pub skipped_no_title: usize,
    pub source_db: String,
}

/// Ingest a Chrome/Edge `History` SQLite DB into `brain` as external-pointer memories.
///
/// * `brain`        — the open `.said` file to write into
/// * `history_db`   — path to the browser `History` file. The browser LOCKS this while running, so the
///                    caller should pass a COPY (we open read-only + immutable, but a copy is safest).
/// * `max_entries`  — cap the number of most-recently-visited pages ingested (0 = no cap).
/// * `min_visits`   — only ingest pages visited at least this many times (1 = everything; raise it to
///                    keep only pages that mattered). A cheap, deterministic relevance filter.
/// * `progress`     — `(done, total, label)` callback, same shape as document_ingest.
pub fn ingest_browser_history<F>(
    brain: &mut SaidFile,
    history_db: &str,
    max_entries: usize,
    min_visits: u32,
    mut progress: F,
) -> Result<BrowserIngestReport, String>
where
    F: FnMut(usize, usize, &str),
{
    // Open read-only + immutable so we never disturb the live browser DB (and so a WAL/locked file
    // still opens). `mode=ro` + `immutable=1` via a URI filename.
    let uri = format!("file:{}?mode=ro&immutable=1", history_db.replace('\\', "/"));
    let conn = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("open browser history '{}': {}", history_db, e))?;

    // Chrome/Edge share the same `urls` schema: (url, title, visit_count, last_visit_time).
    // Order by recency so `max_entries` keeps the MOST RECENT pages.
    let sql = "SELECT url, title, visit_count \
               FROM urls \
               WHERE visit_count >= ?1 \
               ORDER BY last_visit_time DESC";
    let mut stmt = conn.prepare(sql)
        .map_err(|e| format!("prepare urls query (is this a Chrome/Edge History DB?): {}", e))?;

    let rows = stmt.query_map([min_visits], |r| {
        Ok((
            r.get::<_, String>(0)?,                       // url
            r.get::<_, Option<String>>(1)?.unwrap_or_default(), // title
            r.get::<_, i64>(2)? as u32,                   // visit_count
        ))
    }).map_err(|e| format!("query urls: {}", e))?;

    // Collect first (we need a total for the progress bar, and to apply max_entries).
    let mut entries: Vec<(String, String, u32)> = Vec::new();
    for row in rows {
        let (url, title, visits) = row.map_err(|e| format!("read row: {}", e))?;
        if url.is_empty() { continue; }
        entries.push((url, title, visits));
        if max_entries > 0 && entries.len() >= max_entries { break; }
    }

    let total = entries.len();
    let mut ingested = 0usize;
    let mut skipped_no_title = 0usize;

    for (i, (url, title, visits)) in entries.iter().enumerate() {
        progress(i, total, "browser");
        // A page with no title carries almost no searchable signal beyond the URL itself; skip it (the
        // distil-don't-pollute rule). The URL is still reachable via a direct grep if ever needed.
        if title.trim().is_empty() {
            skipped_no_title += 1;
            continue;
        }
        // The SEARCHABLE summary is the page title (what you'd actually remember a page by) plus a short
        // human-readable host fragment so "that rust docs page" still grounds on the host. The full URL
        // lives in the external:uri tag, not duplicated into the body.
        let host = url_host(url);
        let summary = if host.is_empty() {
            title.clone()
        } else {
            format!("{} — {}", title.trim(), host)
        };
        // Stable doc_id from the URL so re-ingest UPDATES the same entry (dedup) rather than duplicating.
        let doc_id = format!("browser/{}", stable_id(url));
        brain.remember_as_external_pointer(
            Some(&doc_id),
            url,
            Some("text/html"),
            Some(title.trim()),
            &summary,
            vec![
                "ingest:browser".to_string(),
                format!("visits:{}", visits),
            ],
        );
        ingested += 1;
    }

    progress(total, total, "browser");
    brain.build_index().map_err(|e| format!("build_index after browser ingest: {}", e))?;

    Ok(BrowserIngestReport {
        entries_seen: total,
        entries_ingested: ingested,
        skipped_no_title,
        source_db: history_db.to_string(),
    })
}

/// Extract the host from a URL for the human-readable summary (no url crate — keep deps lean).
/// "https://doc.rust-lang.org/std/vec/struct.Vec.html" -> "doc.rust-lang.org".
fn url_host(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    after_scheme.split(['/', '?', '#']).next().unwrap_or("").to_string()
}

/// A stable, filesystem/doc-id-safe id derived from the URL (blake3, hex-truncated). Same URL → same
/// id → re-ingest updates in place (the dedup the research calls for).
fn stable_id(url: &str) -> String {
    let h = blake3::hash(url.as_bytes());
    h.to_hex()[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_extraction() {
        assert_eq!(url_host("https://doc.rust-lang.org/std/vec.html"), "doc.rust-lang.org");
        assert_eq!(url_host("http://example.com"), "example.com");
        assert_eq!(url_host("https://news.site.com/path?q=1#frag"), "news.site.com");
        assert_eq!(url_host("about:blank"), "about:blank"); // no scheme sep → passthrough first seg
    }

    #[test]
    fn stable_id_is_deterministic_and_safe() {
        let a = stable_id("https://example.com/page");
        let b = stable_id("https://example.com/page");
        assert_eq!(a, b, "same URL → same id (dedup)");
        assert_ne!(a, stable_id("https://example.com/other"));
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// End-to-end against a synthetic Chrome-shaped History DB.
    #[test]
    fn ingest_synthetic_history() {
        let db = std::env::temp_dir().join(format!("said_browser_test_{}.db", std::process::id()));
        let db_str = db.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&db);
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER);
                 INSERT INTO urls(url,title,visit_count,last_visit_time) VALUES
                   ('https://doc.rust-lang.org/std/vec/struct.Vec.html','Vec in std::vec - Rust',12,300),
                   ('https://arxiv.org/abs/2506.08276','LEANN: low-storage vector index',3,200),
                   ('https://news.ycombinator.com/','Hacker News',40,100),
                   ('https://blank.example/no-title','',1,50);"
            ).unwrap();
        }
        let mut brain = SaidFile::create(
            std::env::temp_dir().join(format!("said_browser_brain_{}.said", std::process::id()))
                .to_string_lossy().as_ref());
        assert!(brain.auto_load_encoder());

        let report = ingest_browser_history(&mut brain, &db_str, 0, 1, |_, _, _| {}).unwrap();
        assert_eq!(report.entries_seen, 4);
        assert_eq!(report.entries_ingested, 3, "3 titled pages ingested");
        assert_eq!(report.skipped_no_title, 1, "the untitled page is skipped");

        // The Rust docs page is recallable BY MEANING and stored as an external pointer to its live URL.
        let (cands, _) = crate::ask::ask(&mut brain, "the rust Vec documentation page", 5, false, None);
        let top = cands.first().expect("a hit");
        assert!(top.doc_id.starts_with("browser/"), "stored under browser/ id");
        let meta = brain.frames.get_meta(&top.doc_id).unwrap();
        assert!(meta.tags.iter().any(|t| t == "ingest:browser"), "tagged ingest:browser");
        assert!(meta.tags.iter().any(|t| t.starts_with("external:uri=https://doc.rust-lang.org")),
            "carries the live URL as external:uri, content NOT embedded");

        let _ = std::fs::remove_file(&db);
    }
}

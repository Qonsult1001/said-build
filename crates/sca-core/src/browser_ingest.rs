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

/// A discovered browser history DB: which browser, which profile, and the path.
#[derive(Debug, Clone)]
pub struct DiscoveredBrowser {
    pub browser: String,   // "Chrome", "Edge", "Brave", …
    pub profile: String,   // "Default", "Profile 1", …
    pub db_path: String,   // absolute path to the `History` SQLite file
}

/// The Chromium family — ALL share the identical `urls` schema, so one query serves every one of them.
/// This is a DATA-DRIVEN table: adding another Chromium browser (Vivaldi, Arc, …) is a one-line entry
/// here, not new code. Each entry is the per-OS parent dir that contains the browser's profile folders
/// (Default, Profile 1, …), each of which holds a `History` file. (Firefox/Safari use a DIFFERENT schema
/// — deliberately NOT here; they'd be separate readers, matching LEANN which ships Chromium only.)
///
/// `{app_local}` = %LOCALAPPDATA% (Windows) / ~/Library/Application Support (macOS) / ~/.config (Linux),
/// resolved by `chromium_roots()`.
const CHROMIUM_BROWSERS: &[(&str, &str, &str, &str)] = &[
    // (browser,   windows subpath under %LOCALAPPDATA%,        macOS subpath under App Support,  linux subpath under ~/.config)
    ("Chrome",     "Google/Chrome/User Data",                    "Google/Chrome",                  "google-chrome"),
    ("Edge",       "Microsoft/Edge/User Data",                   "Microsoft Edge",                 "microsoft-edge"),
    ("Brave",      "BraveSoftware/Brave-Browser/User Data",      "BraveSoftware/Brave-Browser",    "BraveSoftware/Brave-Browser"),
    ("Opera",      "Opera Software/Opera Stable",                "com.operasoftware.Opera",        "opera"),
    ("Vivaldi",    "Vivaldi/User Data",                          "Vivaldi",                        "vivaldi"),
];

/// Resolve the OS-specific base dir where Chromium browsers keep their profile data.
fn chromium_base() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    { std::env::var("LOCALAPPDATA").ok().map(std::path::PathBuf::from) }
    #[cfg(target_os = "macos")]
    { std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join("Library/Application Support")) }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    { std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".config")) }
}

/// Auto-discover every installed Chromium browser and ALL its profiles that have a history DB.
/// Dynamic + zero-config: the user runs `import browser` and we find what's there. Overlap between
/// browsers is harmless — the URL-keyed dedup in `ingest_browser_history` merges duplicates.
pub fn discover_chromium_history() -> Vec<DiscoveredBrowser> {
    let base = match chromium_base() { Some(b) => b, None => return Vec::new() };
    let mut found = Vec::new();
    for (browser, win, mac, linux) in CHROMIUM_BROWSERS {
        #[cfg(target_os = "windows")] let sub = *win;
        #[cfg(target_os = "macos")]   let sub = *mac;
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))] let sub = *linux;
        let user_data = base.join(sub);
        if !user_data.is_dir() { continue; }
        // Each profile folder (Default, Profile 1, …) may hold a `History` SQLite file.
        if let Ok(entries) = std::fs::read_dir(&user_data) {
            for e in entries.flatten() {
                let p = e.path();
                if !p.is_dir() { continue; }
                let hist = p.join("History");
                if hist.is_file() {
                    let profile = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    // Only real profiles (Default / "Profile N"), skip System Profile / Guest.
                    if profile == "Default" || profile.starts_with("Profile ") {
                        found.push(DiscoveredBrowser {
                            browser: browser.to_string(),
                            profile,
                            db_path: hist.to_string_lossy().into_owned(),
                        });
                    }
                }
            }
        }
    }
    found
}

/// Ingest a Chrome/Edge `History` SQLite DB into `brain` as external-pointer memories.
///
/// * `brain`        — the open `.said` file to write into
/// * `history_db`   — path to the browser `History` file. The browser LOCKS this while running, so the
///                    caller should pass a COPY (we open read-only + immutable, but a copy is safest).
/// * `max_entries`  — cap the number of most-recently-visited pages ingested (0 = no cap).
/// * `min_visits`   — only ingest pages visited at least this many times (1 = everything; raise it to
///                    keep only pages that mattered). A cheap, deterministic relevance filter.
/// * `since_days`   — only ingest pages last-visited within this many days (0 = all history). The
///                    "import my recent browsing" control — dynamic recency window, not a fixed cap.
/// * `profile`      — a label for the browser+profile these pages came from (e.g. "Chrome/Profile 1"),
///                    stored as a `profile:` tag. Pass "" to omit. Enables profile-scoped recall AND
///                    keeps the GLOBAL recency sort correct across profiles (it ranks by `visited_at:`,
///                    the absolute timestamp, not the per-import `recency:` ordinal).
/// * `progress`     — `(done, total, label)` callback, same shape as document_ingest.
pub fn ingest_browser_history<F>(
    brain: &mut SaidFile,
    history_db: &str,
    profile: &str,
    max_entries: usize,
    min_visits: u32,
    since_days: u32,
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

    // Chrome/Edge/Brave/… share the same `urls` schema: (url, title, visit_count, typed_count,
    // last_visit_time). typed_count = how often the user TYPED/searched this URL (higher intent than a
    // passive visit) — a stronger salience signal, adopted from LEANN's latest reader. Order by recency
    // so `max_entries` keeps the MOST RECENT pages.
    //
    // Chrome time = microseconds since 1601-01-01. `since_days` → a WHERE cutoff in that epoch: the
    // "import my last N days" recency window (0 = all). Computed against a passed-in "now" is overkill;
    // SQLite's own clock via strftime keeps it deterministic and DB-relative.
    let since_clause = if since_days > 0 {
        // Chrome epoch µs of (now - since_days). 11644473600 = seconds between 1601 and 1970.
        format!(" AND last_visit_time >= (strftime('%s','now','-{} days') + 11644473600) * 1000000",
            since_days)
    } else { String::new() };
    let sql = format!(
        "SELECT url, title, visit_count, typed_count, last_visit_time \
         FROM urls \
         WHERE visit_count >= ?1{} \
         ORDER BY last_visit_time DESC",
        since_clause);
    let mut stmt = conn.prepare(&sql)
        .map_err(|e| format!("prepare urls query (is this a Chrome/Edge History DB?): {}", e))?;

    let rows = stmt.query_map([min_visits], |r| {
        Ok((
            r.get::<_, String>(0)?,                       // url
            r.get::<_, Option<String>>(1)?.unwrap_or_default(), // title
            r.get::<_, i64>(2)? as u32,                   // visit_count
            r.get::<_, i64>(3)? as u32,                   // typed_count
            r.get::<_, i64>(4)?,                          // last_visit_time (Chrome epoch µs)
        ))
    }).map_err(|e| format!("query urls: {}", e))?;

    // Collect first (we need a total for the progress bar, and to apply max_entries). Rows are already
    // ORDER BY last_visit_time DESC, so `entries[0]` is the MOST RECENTLY visited page.
    let mut entries: Vec<(String, String, u32, u32, i64)> = Vec::new();
    for row in rows {
        let (url, title, visits, typed, last_visit) = row.map_err(|e| format!("read row: {}", e))?;
        if url.is_empty() { continue; }
        entries.push((url, title, visits, typed, last_visit));
        if max_entries > 0 && entries.len() >= max_entries { break; }
    }

    let total = entries.len();
    let mut ingested = 0usize;
    let mut skipped_no_title = 0usize;

    for (i, (url, title, visits, typed, last_visit)) in entries.iter().enumerate() {
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
        // Tags: `domain:<host>` makes browsing history TAG-FILTERABLE (ask --tag domain:github.com) —
        // pairs with the tie-scoping UX. `typed:<n>` is the search-intent salience signal from LEANN.
        let mut tags = vec![
            "ingest:browser".to_string(),
            format!("visits:{}", visits),
        ];
        if !host.is_empty() { tags.push(format!("domain:{}", host)); }
        if *typed > 0 { tags.push(format!("typed:{}", typed)); }
        // Which browser+profile this page came from (e.g. `profile:Chrome/Profile 1`). Makes recall
        // profile-filterable AND — critically — lets the GLOBAL recency sort span every profile without
        // the per-profile `recency:` ordinal colliding (three profiles each had a `recency:1`).
        if !profile.is_empty() { tags.push(format!("profile:{}", profile)); }
        // RECENCY: the visit time was being DROPPED, so "what was the last website I visited?" had no
        // signal to rank by (created_at = import time, not visit time). Capture it now:
        //   - `visited_at:<unix_secs>` — the ABSOLUTE wall-clock timestamp. This is the GLOBAL sort key:
        //     temporal recall ranks by this across ALL profiles/imports, so "last site I visited" returns
        //     yesterday's page from Profile 1, never a 3-year-old page from Profile 3. (The per-batch
        //     `recency:` ordinal below is NOT globally comparable — each import restarts it at 1.)
        //   - `visited:<yyyy-mm-dd>` — the human-readable date (display + `--tag` filtering).
        //   - `recency:<rank>` — 1-based position within THIS import's DESC order (kept for display /
        //     backward-compat; global ordering uses visited_at:).
        // Chrome epoch is µs since 1601; convert to unix seconds.
        if *last_visit > 0 {
            let unix_secs = last_visit / 1_000_000 - 11_644_473_600;
            if unix_secs > 0 {
                tags.push(format!("visited_at:{}", unix_secs));
                tags.push(format!("visited:{}", unix_to_date(unix_secs)));
            }
        }
        tags.push(format!("recency:{}", i + 1)); // 1 = most recently visited within this import
        brain.remember_as_external_pointer(
            Some(&doc_id),
            url,
            Some("text/html"),
            Some(title.trim()),
            &summary,
            tags,
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

/// Unix seconds → "yyyy-mm-dd" for the `visited:` tag. No chrono dep — Howard Hinnant's civil-from-days.
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
            // Real Chrome schema includes `typed_count`. last_visit_time is Chrome-epoch µs; use a huge
            // value so the (default since_days=0) query has no cutoff effect.
            c.execute_batch(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER);
                 INSERT INTO urls(url,title,visit_count,typed_count,last_visit_time) VALUES
                   ('https://doc.rust-lang.org/std/vec/struct.Vec.html','Vec in std::vec - Rust',12,5,13300000000000000),
                   ('https://arxiv.org/abs/2506.08276','LEANN: low-storage vector index',3,0,13200000000000000),
                   ('https://news.ycombinator.com/','Hacker News',40,20,13100000000000000),
                   ('https://blank.example/no-title','',1,0,13050000000000000);"
            ).unwrap();
        }
        let mut brain = SaidFile::create(
            std::env::temp_dir().join(format!("said_browser_brain_{}.said", std::process::id()))
                .to_string_lossy().as_ref());
        assert!(brain.auto_load_encoder());

        // max=0 (all), min_visits=1, since_days=0 (all history). profile label = "Chrome/Default".
        let report = ingest_browser_history(&mut brain, &db_str, "Chrome/Default", 0, 1, 0, |_, _, _| {}).unwrap();
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
        // New v0.12 signals: domain tag (filterable) + typed count (search-intent salience).
        assert!(meta.tags.iter().any(|t| t == "domain:doc.rust-lang.org"),
            "carries a filterable domain: tag");
        assert!(meta.tags.iter().any(|t| t == "typed:5"), "carries the typed-count salience signal");
        // RECENCY signals: the Rust page has the LARGEST last_visit_time → recency:1 (most recent),
        // and a visited:<date> tag (13300000000000000 µs since 1601 → a 2382 date; just assert shape).
        assert!(meta.tags.iter().any(|t| t == "recency:1"),
            "most-recently-visited page is recency:1 (the 'last website' signal)");
        assert!(meta.tags.iter().any(|t| t.starts_with("visited:")),
            "carries the human-readable visit-date tag");
        assert!(meta.tags.iter().any(|t| t.starts_with("visited_at:")),
            "carries the ABSOLUTE epoch tag (the global recency sort key)");
        assert!(meta.tags.iter().any(|t| t == "profile:Chrome/Default"),
            "carries the profile: tag for profile-scoped recall");

        // TEMPORAL RECALL: "what was the LAST website I visited?" must return the most-recently
        // visited page FIRST. The Rust docs page has the largest last_visit_time, so it leads.
        let (recent, _) = crate::ask::ask(&mut brain, "what was the last website i visited?", 5, false, None);
        let lead = recent.first().expect("a recency-ranked hit");
        assert_eq!(lead.doc_id, top.doc_id,
            "temporal query 'last website i visited' ranks the most-recent (largest visited_at:) page first");

        let _ = std::fs::remove_file(&db);
    }

    /// THE cross-profile bug: `recency:N` is per-import, so importing an OLD profile after a NEW one used
    /// to leave two `recency:1` pages and rank the stale one arbitrarily. Global recency must sort by the
    /// ABSOLUTE `visited_at:` timestamp so yesterday's page always beats a years-old one, regardless of
    /// which profile it came from or what order they were imported.
    #[test]
    fn global_recency_across_profiles() {
        let pid = std::process::id();
        // Profile A ("old"): its newest page is from ~2018 (Chrome µs). Profile B ("new"): ~2026.
        let mk = |name: &str, url: &str, title: &str, ts: i64| {
            let db = std::env::temp_dir().join(format!("said_prof_{}_{}.db", name, pid));
            let _ = std::fs::remove_file(&db);
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch(&format!(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER);
                 INSERT INTO urls(url,title,visit_count,typed_count,last_visit_time) VALUES ('{}','{}',5,1,{});",
                url, title, ts)).unwrap();
            db.to_string_lossy().to_string()
        };
        // 13100000000000000 µs ≈ 2016; 13380000000000000 µs ≈ 2025 — B is clearly newer.
        let old_db = mk("old", "https://old.example/ancient-news", "Ancient news from long ago", 13100000000000000);
        let new_db = mk("new", "https://fresh.example/todays-update", "Todays fresh update", 13380000000000000);

        let mut brain = SaidFile::create(
            std::env::temp_dir().join(format!("said_prof_brain_{}.said", pid)).to_string_lossy().as_ref());
        assert!(brain.auto_load_encoder());

        // Import the NEW profile FIRST, then the OLD one — the exact order that used to break it (the old
        // profile's page ALSO gets recency:1 in its own batch, and being imported last, it won ties).
        ingest_browser_history(&mut brain, &new_db, "Chrome/Profile 1", 0, 1, 0, |_,_,_| {}).unwrap();
        ingest_browser_history(&mut brain, &old_db, "Chrome/Profile 3", 0, 1, 0, |_,_,_| {}).unwrap();

        let (recent, _) = crate::ask::ask(&mut brain, "what was the last website i visited?", 5, false, None);
        let lead = recent.first().expect("a recency-ranked hit");
        assert!(lead.content.contains("fresh update") || lead.content.contains("Today"),
            "global recency must return the 2025 page from Profile 1, NOT the 2016 page from Profile 3 \
             (got: {})", lead.content.lines().next().unwrap_or(""));

        for p in [old_db, new_db] { let _ = std::fs::remove_file(&p); }
    }
}

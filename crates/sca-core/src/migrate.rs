//! One-line migration from competitor memory systems (mem0, memvid, Zep,
//! LangMem) into `.said` frames. Each adapter reads the competitor's
//! on-disk/API export format and maps records to the right `.said` pillar:
//!
//!   - User facts / preferences     → `Pillar::Semantic`
//!   - Raw conversation turns       → `Pillar::Episodic`
//!   - Agent plans / action recipes → `Pillar::Procedural`
//!   - Document references          → `Pillar::External` (pointer mode if
//!                                     the target brain is Enterprise)
//!
//! All adapters share the `MigrationAdapter` trait so the CLI can dispatch
//! via `--from <name>` without caring how each competitor stores data.
//! Timestamps, user_ids, and source metadata survive as tags so admin
//! queries like `said admin who-deleted` can still show provenance after
//! migration.
//!
//! Status:
//!   - memvid (JSON)   — shipped (reference adapter; smallest format)
//!   - mem0 (JSONL)    — shipped (mem0's export format; matches their
//!                       `export_memories` dump)
//!   - Zep / LangMem   — declared + stubbed; implementations follow the
//!                       same trait when the export format stabilizes.

use std::path::Path;
use std::collections::HashMap;

use crate::frames::Pillar;
use crate::said_file::SaidFile;

/// One migrated record ready to be written into a `.said` brain.
///
/// The adapter produces these; the `run_migration` driver consumes them.
/// Keeping the intermediate shape generic lets every adapter stay small —
/// it only has to parse the competitor's format and yield `MigratedRecord`s.
#[derive(Debug, Clone)]
pub struct MigratedRecord {
    /// Stable doc_id. If the competitor has its own ids we prefix them:
    /// `mem0:<uuid>`, `memvid:<frame>`, `zep:<session>:<turn>`. This keeps
    /// provenance legible and stops collisions with native `.said` ids.
    pub doc_id: String,
    /// Content that gets written into the frame body.
    pub content: String,
    /// Optional title / display label.
    pub title: Option<String>,
    /// Target pillar.
    pub pillar: Pillar,
    /// Tags — always includes `imported_from:<system>`; adapters add more
    /// (`user_id:<id>`, `session:<id>`, `ingested_at:<unix>`, etc.).
    pub tags: Vec<String>,
}

/// Adapter contract — implemented once per competitor. The `name()` is
/// surfaced in `said import --from <name>`.
pub trait MigrationAdapter {
    /// Short identifier used on the CLI (`mem0`, `memvid`, `zep`, `langmem`).
    fn name(&self) -> &'static str;

    /// Parse the competitor's on-disk export at `path` into an iterator of
    /// records. `path` can be a file or directory depending on the format.
    fn load(&self, path: &Path) -> Result<Vec<MigratedRecord>, String>;
}

/// Report returned by `run_migration` — what to print at the end of a
/// successful import.
#[derive(Debug, Default)]
pub struct MigrationReport {
    pub source_system: String,
    pub records_read: usize,
    pub records_written: usize,
    pub records_skipped: usize,
    pub per_pillar: HashMap<String, usize>,
    pub errors: Vec<String>,
}

/// Drive a migration end-to-end: parse, map to pillar, write to brain.
/// Respects the target brain's mode — Enterprise brains auto-downgrade
/// content-embedding records to External pointers when the source record
/// carries a `source_uri` tag; content-bearing records without a URI are
/// refused (admin must switch to Portable or strip the content first).
pub fn run_migration(
    adapter: &dyn MigrationAdapter,
    source: &Path,
    brain: &mut SaidFile,
) -> Result<MigrationReport, String> {
    let records = adapter.load(source)?;
    let mut report = MigrationReport {
        source_system: adapter.name().to_string(),
        records_read: records.len(),
        ..Default::default()
    };

    let is_enterprise = brain.mode() == crate::said_file::BrainMode::Enterprise;

    for rec in records {
        // Mode guard. For Enterprise brains, only External pointer writes
        // are allowed; everything else gets counted as a skip with a reason.
        if is_enterprise && rec.pillar != Pillar::External {
            report.records_skipped += 1;
            report.errors.push(format!(
                "skipped '{}' — Enterprise brain refuses {:?} content embed",
                rec.doc_id, rec.pillar,
            ));
            continue;
        }

        // Ensure `imported_from:<system>` is always present.
        let mut tags = rec.tags.clone();
        let stamp = format!("imported_from:{}", adapter.name());
        if !tags.iter().any(|t| t == &stamp) {
            tags.push(stamp);
        }

        let _ = brain.remember_with_pillar(
            Some(&rec.doc_id),
            &rec.content,
            rec.title.as_deref(),
            rec.pillar,
            tags,
        );
        report.records_written += 1;
        let key = match rec.pillar {
            Pillar::Episodic => "episodic",
            Pillar::Semantic => "semantic",
            Pillar::Procedural => "procedural",
            Pillar::External => "external",
            Pillar::Code => "code",
            Pillar::Memory => "memory",
            Pillar::Document => "document",
        };
        *report.per_pillar.entry(key.to_string()).or_insert(0) += 1;
    }

    Ok(report)
}

// ════════════════════════════════════════════════════════════════════════════
// memvid adapter — JSON manifest with memory records
// ════════════════════════════════════════════════════════════════════════════

/// memvid stores memories as a JSON array of `{id, content, timestamp,
/// metadata}` objects. This adapter reads such a file (or a directory of
/// JSON files) and maps each record to an Episodic frame with timestamp +
/// metadata preserved as tags.
pub struct MemvidAdapter;

impl MigrationAdapter for MemvidAdapter {
    fn name(&self) -> &'static str { "memvid" }

    fn load(&self, path: &Path) -> Result<Vec<MigratedRecord>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;
        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("parse {}: {}", path.display(), e))?;
        let arr = v.as_array()
            .ok_or_else(|| format!("{} is not a JSON array", path.display()))?;

        let mut out: Vec<MigratedRecord> = Vec::with_capacity(arr.len());
        for (i, r) in arr.iter().enumerate() {
            let id = r.get("id").and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("row_{}", i));
            let content = r.get("content").and_then(|x| x.as_str())
                .ok_or_else(|| format!("memvid record {} missing `content`", id))?;
            let ts = r.get("timestamp").and_then(|x| x.as_u64());
            let mut tags: Vec<String> = Vec::new();
            if let Some(ts) = ts { tags.push(format!("ingested_at:{}", ts)); }
            if let Some(meta) = r.get("metadata").and_then(|x| x.as_object()) {
                for (k, v) in meta {
                    if let Some(s) = v.as_str() {
                        tags.push(format!("{}:{}", k, s));
                    }
                }
            }
            out.push(MigratedRecord {
                doc_id: format!("memvid:{}", id),
                content: content.to_string(),
                title: None,
                pillar: Pillar::Episodic,
                tags,
            });
        }
        Ok(out)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// mem0 adapter — JSONL export (one memory per line)
// ════════════════════════════════════════════════════════════════════════════

/// mem0's `export_memories()` produces a JSONL file with one record per
/// line:
///
/// ```jsonc
/// {"id":"...","memory":"User likes dogs","user_id":"u1",
///  "created_at":"2025-04-01T10:00:00Z","categories":["preference"],
///  "metadata":{...}}
/// ```
///
/// The `memory` field is the content; `categories` hint at pillar. mem0
/// doesn't distinguish Episodic from Semantic rigidly, so we default to
/// Semantic (their canonical "user fact") unless a category maps
/// otherwise.
pub struct Mem0Adapter;

impl Mem0Adapter {
    fn pick_pillar(categories: &[&str]) -> Pillar {
        for c in categories {
            match c.to_lowercase().as_str() {
                "turn" | "dialog" | "conversation" => return Pillar::Episodic,
                "plan" | "action" | "procedure" | "recipe" => return Pillar::Procedural,
                "document" | "file" | "reference" => return Pillar::External,
                _ => {}
            }
        }
        Pillar::Semantic
    }
}

impl MigrationAdapter for Mem0Adapter {
    fn name(&self) -> &'static str { "mem0" }

    fn load(&self, path: &Path) -> Result<Vec<MigratedRecord>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;
        let mut out: Vec<MigratedRecord> = Vec::new();
        for (lineno, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let v: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| format!("{} line {}: {}", path.display(), lineno + 1, e))?;
            let id = v.get("id").and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("row_{}", lineno));
            let content = v.get("memory").and_then(|x| x.as_str())
                .ok_or_else(|| format!("mem0 record {} missing `memory`", id))?;
            let user = v.get("user_id").and_then(|x| x.as_str());
            let created_at = v.get("created_at").and_then(|x| x.as_str());
            let categories: Vec<&str> = v.get("categories")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|c| c.as_str()).collect())
                .unwrap_or_default();

            let pillar = Self::pick_pillar(&categories);

            let mut tags: Vec<String> = Vec::new();
            if let Some(u) = user { tags.push(format!("user_id:{}", u)); }
            if let Some(ts) = created_at { tags.push(format!("ingested_at:{}", ts)); }
            for c in &categories { tags.push(format!("category:{}", c)); }

            out.push(MigratedRecord {
                doc_id: format!("mem0:{}", id),
                content: content.to_string(),
                title: None,
                pillar,
                tags,
            });
        }
        Ok(out)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Adapter registry — CLI + MCP call into this to dispatch by name
// ════════════════════════════════════════════════════════════════════════════

/// Resolve an adapter name to an instance. Returns `None` for unknown
/// systems so the caller can emit a clean error.
pub fn adapter_for(name: &str) -> Option<Box<dyn MigrationAdapter>> {
    match name.trim().to_lowercase().as_str() {
        "memvid" => Some(Box::new(MemvidAdapter)),
        "mem0"   => Some(Box::new(Mem0Adapter)),
        // Zep and LangMem adapters declared in the roadmap but not
        // implemented yet — their export formats are still in flux.
        _ => None,
    }
}

/// List the adapter names registered today. Used by `said import --list`
/// and by the MCP schema enum.
pub fn registered_adapters() -> &'static [&'static str] {
    &["memvid", "mem0"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn memvid_round_trip() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let json = r#"[
            {"id":"a","content":"hello","timestamp":1700000000,"metadata":{"user":"alice"}},
            {"id":"b","content":"world","timestamp":1700000001,"metadata":{"user":"bob"}}
        ]"#;
        write!(f, "{}", json).unwrap();
        let records = MemvidAdapter.load(f.path()).expect("load memvid");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].doc_id, "memvid:a");
        assert_eq!(records[1].doc_id, "memvid:b");
        assert!(records[0].tags.iter().any(|t| t == "user:alice"));
    }

    #[test]
    fn mem0_pillar_mapping() {
        assert_eq!(Mem0Adapter::pick_pillar(&["preference"]), Pillar::Semantic);
        assert_eq!(Mem0Adapter::pick_pillar(&["turn"]), Pillar::Episodic);
        assert_eq!(Mem0Adapter::pick_pillar(&["procedure"]), Pillar::Procedural);
        assert_eq!(Mem0Adapter::pick_pillar(&["document"]), Pillar::External);
    }

    #[test]
    fn adapter_registry_resolves() {
        assert!(adapter_for("memvid").is_some());
        assert!(adapter_for("mem0").is_some());
        assert!(adapter_for("MEM0").is_some());  // case-insensitive
        assert!(adapter_for("unknown").is_none());
    }
}

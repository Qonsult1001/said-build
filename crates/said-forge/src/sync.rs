//! `said forge sync` — mechanical ingest from the 4 authority folders into
//! the workspace `.said` brain.
//!
//! Sync is stateless and idempotent: reads `.forge/config.toml`, walks each
//! folder, tags every written frame with `authority:<level>:<scope>`, and
//! records a manifest of `(size, mtime)` per file so subsequent runs skip
//! unchanged content.
//!
//! Sync does **not** prompt. All decisions (directive choice, per-file
//! overrides) were resolved during `forge plan` and live in
//! `.forge/config.toml`.

use sca_core::frames::Pillar as ScaPillar;
use sca_core::said_file::SaidFile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::plan_scan::{scan_project, ScannedFile};
use crate::workspace_config::WorkspaceConfig;
use crate::{ForgeError, ForgeResult};

// ───────────────────────── router ─────────────────────────

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum SyncKind {
    /// `.sql` — ingested as Code pillar.
    CodeSql,
    /// Generic code (`.cs`, `.rs`, `.py`, `.go`, `.ts`, ...) — Code pillar.
    CodeGeneric,
    /// `.md`/`.txt`/etc. — External pillar.
    DocText,
    /// PDF / DOCX — handled via raw read for MVP; sca-core doc plugin
    /// wiring is a follow-up.
    DocBinary,
    /// `.xlsx`/`.xlsm` — deferred to Phase 15's xlsx extractor.
    DocXlsx,
    /// `.yaml`/`.yml`/`.json` — directive candidate. Stored as a pointer
    /// frame; actual story iteration happens during `forge run`.
    Directive,
    /// Unsupported extension — skip.
    Skip,
}

pub fn classify_extension(path: &Path) -> SyncKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "sql" => SyncKind::CodeSql,
        "cs" | "rs" | "py" | "go" | "ts" | "tsx" | "js" | "jsx" | "java" | "kt" | "swift"
        | "rb" | "php" | "cpp" | "c" | "h" | "hpp" | "scala" | "clj" => SyncKind::CodeGeneric,
        "md" | "markdown" | "txt" | "rtf" => SyncKind::DocText,
        "pdf" | "docx" | "doc" => SyncKind::DocBinary,
        "xlsx" | "xlsm" => SyncKind::DocXlsx,
        "yaml" | "yml" | "json" => SyncKind::Directive,
        _ => SyncKind::Skip,
    }
}

pub fn pillar_for_kind(k: SyncKind) -> ScaPillar {
    match k {
        SyncKind::CodeSql | SyncKind::CodeGeneric => ScaPillar::Code,
        _ => ScaPillar::External,
    }
}

// ───────────────────────── authority resolver ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedAuthority {
    /// `law | existing | requested | agreed | wishlist | soft | hard | ...`
    pub level: String,
    /// Which folder produced this file — `ground-truth | progress | requirements | expectations`.
    pub scope: String,
}

impl ResolvedAuthority {
    pub fn to_tag(&self) -> String {
        format!("authority:{}:{}", self.level, self.scope)
    }
}

/// Resolve the authority of a single file. Per-file overrides from
/// `config.toml` `[files]` win over folder defaults.
pub fn resolve_authority(
    cfg: &WorkspaceConfig,
    rel_path: &Path,
    folder_key: &str,
) -> ResolvedAuthority {
    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
    if let Some(over) = cfg.files.get(&rel_str) {
        return ResolvedAuthority {
            level: over.authority.clone(),
            scope: folder_key.to_string(),
        };
    }
    let level = cfg
        .folders
        .get(folder_key)
        .map(|f| f.default_authority.clone())
        .unwrap_or_else(|| "unknown".to_string());
    ResolvedAuthority {
        level,
        scope: folder_key.to_string(),
    }
}

// ───────────────────────── sync plan ─────────────────────────

#[derive(Debug)]
pub struct SyncPlan {
    pub root: PathBuf,
    pub entries: Vec<SyncEntry>,
    pub skipped_unchanged: Vec<PathBuf>,
    pub orphans: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SyncEntry {
    pub path: PathBuf,
    pub rel_path: PathBuf,
    pub kind: SyncKind,
    pub authority: ResolvedAuthority,
    pub size: u64,
    pub mtime_secs: i64,
}

fn mtime_secs_of(f: &ScannedFile) -> i64 {
    std::fs::metadata(&f.path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn build_sync_plan(root: &Path, cfg: &WorkspaceConfig) -> ForgeResult<SyncPlan> {
    let scan = scan_project(root)?;
    let mut entries = Vec::new();
    for (folder_name, folder_key, files) in [
        ("1-ground-truth", "ground-truth", &scan.ground_truth),
        ("2-progress", "progress", &scan.progress),
        ("3-requirements", "requirements", &scan.requirements),
        ("4-expectations", "expectations", &scan.expectations),
    ] {
        for f in files {
            let kind = classify_extension(&f.path);
            if matches!(kind, SyncKind::Skip) {
                continue;
            }
            let rel = f
                .path
                .strip_prefix(root)
                .unwrap_or(&f.path)
                .to_path_buf();
            let authority = resolve_authority(cfg, &rel, folder_key);
            entries.push(SyncEntry {
                path: f.path.clone(),
                rel_path: rel,
                kind,
                authority,
                size: f.size_bytes,
                mtime_secs: mtime_secs_of(f),
            });
            let _ = folder_name;
        }
    }
    Ok(SyncPlan {
        root: root.to_path_buf(),
        entries,
        skipped_unchanged: Vec::new(),
        orphans: scan.orphan_files.iter().map(|f| f.path.clone()).collect(),
    })
}

// ───────────────────────── manifest (idempotent resync gate) ─────────────────────────

pub const MANIFEST_TAG: &str = "forge-sync-manifest:v1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncManifest {
    pub files: BTreeMap<String, ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub size: u64,
    pub mtime_secs: i64,
    /// The authority tag in effect when this file was last ingested. A
    /// change forces re-ingest even if size+mtime match.
    pub authority_tag: String,
}

impl SyncManifest {
    /// Drop entries from `plan.entries` whose (size, mtime, authority) match
    /// this manifest — push them onto `skipped_unchanged` instead.
    pub fn filter_unchanged(&self, plan: &mut SyncPlan) {
        let mut kept = Vec::with_capacity(plan.entries.len());
        for entry in plan.entries.drain(..) {
            let key = entry.rel_path.to_string_lossy().replace('\\', "/");
            if let Some(m) = self.files.get(&key) {
                if m.size == entry.size
                    && m.mtime_secs == entry.mtime_secs
                    && m.authority_tag == entry.authority.to_tag()
                {
                    plan.skipped_unchanged.push(entry.path);
                    continue;
                }
            }
            kept.push(entry);
        }
        plan.entries = kept;
    }

    pub fn record(&mut self, entry: &SyncEntry) {
        self.files.insert(
            entry.rel_path.to_string_lossy().replace('\\', "/"),
            ManifestEntry {
                size: entry.size,
                mtime_secs: entry.mtime_secs,
                authority_tag: entry.authority.to_tag(),
            },
        );
    }
}

// ───────────────────────── execute ─────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncResult {
    pub frames_written: u64,
    pub files_skipped_unchanged: usize,
    pub files_skipped_xlsx_unsupported: usize,
    pub bytes_ingested: u64,
    pub by_kind: BTreeMap<String, u32>,
    pub by_authority: BTreeMap<String, u32>,
}

/// Apply the sync plan to the brain. Does NOT call `.save()` — the caller
/// is responsible for persisting.
pub fn execute_sync(
    plan: &SyncPlan,
    said: &mut SaidFile,
    project_name: &str,
) -> ForgeResult<SyncResult> {
    execute_sync_with_cfg(plan, said, project_name, None)
}

/// Like `execute_sync`, but with access to the workspace config — enables
/// XLSX ingest when `forge-xlsx` feature is on and the user chose a non-skip
/// mode in Q5.
pub fn execute_sync_with_cfg(
    plan: &SyncPlan,
    said: &mut SaidFile,
    project_name: &str,
    cfg: Option<&WorkspaceConfig>,
) -> ForgeResult<SyncResult> {
    let mut result = SyncResult::default();

    // Load previous manifest (if any), merge with new entries. The caller
    // pre-computed `plan.skipped_unchanged` via `SyncManifest::filter_unchanged`;
    // here we just accumulate the fresh entries into a manifest we'll persist.
    let mut manifest = load_manifest(said, project_name).unwrap_or_default();

    for entry in &plan.entries {
        let auth_tag = entry.authority.to_tag();
        let kind_key = format!("{:?}", entry.kind);

        match entry.kind {
            SyncKind::CodeSql | SyncKind::CodeGeneric | SyncKind::DocText => {
                let content = std::fs::read_to_string(&entry.path).map_err(|e| {
                    ForgeError::Io {
                        path: entry.path.display().to_string(),
                        cause: e,
                    }
                })?;
                let pillar = pillar_for_kind(entry.kind);
                let title = entry.rel_path.to_string_lossy().replace('\\', "/");
                let doc_id = format!("forge-sync:{}", title);
                // Derive a `client:<Name>` tag from the path. dt convention:
                // `1-ground-truth/<Client>/...` (e.g. `1-ground-truth/TXN/...`)
                // → `client:TXN`. The MCP `handle_sandbox` multi-client gate
                // requires a client tag on every frame — without this, our
                // forge-sync proc frames get filtered out of `said sandbox <client>`
                // even though the path makes the client clear.
                let mut tags = vec![
                    auth_tag.clone(),
                    format!("forge-project:{}", project_name),
                    format!("forge-kind:{}", kind_str(entry.kind)),
                ];
                if let Some(client) = title
                    .strip_prefix("1-ground-truth/")
                    .and_then(|s| s.split('/').next())
                {
                    if !client.is_empty() {
                        tags.push(format!("client:{}", client));
                    }
                }
                said.remember_with_pillar(
                    Some(&doc_id),
                    &content,
                    Some(&title),
                    pillar,
                    tags,
                );
                result.frames_written += 1;
                result.bytes_ingested += entry.size;
            }
            SyncKind::DocBinary => {
                // Real PDF/DOCX extraction via sca-core's docs plugin.
                // sca_core::document_ingest::ingest_document chunks the
                // file into semantic segments + writes one frame per chunk.
                // We then retrofit the authority tag onto each frame via
                // add_tag so grounding retrieval can filter the whole doc
                // the same way it filters SQL or code frames.
                #[cfg(feature = "forge-docs")]
                {
                    let path_str = entry.path.to_string_lossy().to_string();
                    let before: std::collections::HashSet<String> = said
                        .frames
                        .get_all_frames_with_pending()
                        .into_iter()
                        .map(|m| m.doc_id.clone())
                        .collect();
                    match sca_core::document_ingest::ingest_document(
                        said,
                        &path_str,
                        |_done, _total, _label| {},
                    ) {
                        Ok(report) => {
                            let title = entry.rel_path.to_string_lossy().replace('\\', "/");
                            let new_doc_ids: Vec<String> = said
                                .frames
                                .get_all_frames_with_pending()
                                .into_iter()
                                .map(|m| m.doc_id.clone())
                                .filter(|id| !before.contains(id))
                                .collect();
                            // Tag retrofit on every newly written frame.
                            for did in &new_doc_ids {
                                said.add_tag(did, &auth_tag);
                                said.add_tag(did, &format!("forge-project:{}", project_name));
                                said.add_tag(did, &format!("forge-kind:{}", kind_str(entry.kind)));
                                said.add_tag(did, &format!("forge-source:{}", title));
                            }
                            result.frames_written += new_doc_ids.len() as u64;
                            result.bytes_ingested += entry.size;
                            let _ = report;
                        }
                        Err(e) => {
                            eprintln!(
                                "  ⚠ doc ingest failed for {}: {} (falling back to pointer)",
                                entry.rel_path.display(),
                                e
                            );
                            write_binary_pointer(said, entry, &auth_tag, project_name);
                            result.frames_written += 1;
                            result.bytes_ingested += entry.size;
                        }
                    }
                }
                #[cfg(not(feature = "forge-docs"))]
                {
                    // Without `forge-docs` we can only record a pointer.
                    write_binary_pointer(said, entry, &auth_tag, project_name);
                    result.frames_written += 1;
                    result.bytes_ingested += entry.size;
                }
            }
            SyncKind::Directive => {
                // Pointer frame for the directive file. The actual
                // iteration happens in `forge run`, which reads this frame
                // (or re-parses the file directly).
                let title = entry.rel_path.to_string_lossy().replace('\\', "/");
                let doc_id = format!("forge-sync-directive:{}", title);
                let body = format!("Directive candidate: {}", title);
                said.remember_with_pillar(
                    Some(&doc_id),
                    &body,
                    Some(&title),
                    ScaPillar::External,
                    vec![
                        auth_tag.clone(),
                        format!("forge-project:{}", project_name),
                        "forge-directive-ref".to_string(),
                    ],
                );
                result.frames_written += 1;
            }
            SyncKind::DocXlsx => {
                #[cfg(feature = "forge-xlsx")]
                {
                    use crate::source::xlsx::{
                        ingest_sheet_as_grounding, should_skip_sheet, XlsxReader,
                    };
                    use crate::workspace_config::XlsxMode;
                    let xlsx_mode = cfg.map(|c| c.xlsx.mode).unwrap_or(XlsxMode::Unset);
                    if matches!(xlsx_mode, XlsxMode::Unset) {
                        // User answered "skip" in Q5 (or no config provided) —
                        // treat as deferred.
                        result.files_skipped_xlsx_unsupported += 1;
                        continue;
                    }
                    let default_skip: Vec<String> = Vec::new();
                    let skip_patterns = cfg
                        .map(|c| &c.xlsx.sheet_skip_patterns)
                        .unwrap_or(&default_skip);
                    let mut reader = match XlsxReader::open(&entry.path) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!(
                                "  ⚠ xlsx open failed: {}: {}",
                                entry.rel_path.display(),
                                e
                            );
                            continue;
                        }
                    };
                    for sheet_name in reader.sheet_names() {
                        if should_skip_sheet(&sheet_name, skip_patterns) {
                            continue;
                        }
                        let sheet = match reader.read_sheet(&sheet_name) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        let n = ingest_sheet_as_grounding(
                            said,
                            &sheet,
                            &auth_tag,
                            project_name,
                        )?;
                        result.frames_written += n;
                        result.bytes_ingested += entry.size;
                    }
                }
                #[cfg(not(feature = "forge-xlsx"))]
                {
                    let _ = cfg;
                    result.files_skipped_xlsx_unsupported += 1;
                    continue;
                }
            }
            SyncKind::Skip => continue,
        }

        *result.by_kind.entry(kind_key).or_insert(0) += 1;
        *result
            .by_authority
            .entry(entry.authority.level.clone())
            .or_insert(0) += 1;
        manifest.record(entry);
    }

    result.files_skipped_unchanged = plan.skipped_unchanged.len();

    // Persist manifest as a frame.
    save_manifest(said, &manifest, project_name)?;
    Ok(result)
}

fn write_binary_pointer(
    said: &mut SaidFile,
    entry: &SyncEntry,
    auth_tag: &str,
    project_name: &str,
) {
    let title = entry.rel_path.to_string_lossy().replace('\\', "/");
    let doc_id = format!("forge-sync-pointer:{}", title);
    let body = format!(
        "Binary document at {} (size {} bytes). Full text extraction requires \
         --features forge-docs on said-cli/said-mcp.",
        title, entry.size
    );
    said.remember_with_pillar(
        Some(&doc_id),
        &body,
        Some(&title),
        ScaPillar::External,
        vec![
            auth_tag.to_string(),
            format!("forge-project:{}", project_name),
            format!("forge-kind:{}", kind_str(entry.kind)),
            "forge-binary-pointer".to_string(),
        ],
    );
}

fn kind_str(k: SyncKind) -> &'static str {
    match k {
        SyncKind::CodeSql => "code-sql",
        SyncKind::CodeGeneric => "code-generic",
        SyncKind::DocText => "doc-text",
        SyncKind::DocBinary => "doc-binary",
        SyncKind::DocXlsx => "doc-xlsx",
        SyncKind::Directive => "directive",
        SyncKind::Skip => "skip",
    }
}

fn manifest_doc_id(project_name: &str) -> String {
    format!("forge-sync-manifest:{}", project_name)
}

fn save_manifest(
    said: &mut SaidFile,
    manifest: &SyncManifest,
    project_name: &str,
) -> ForgeResult<()> {
    let body = serde_json::to_string_pretty(manifest)
        .map_err(ForgeError::Serde)?;
    said.remember_with_pillar(
        Some(&manifest_doc_id(project_name)),
        &body,
        Some(MANIFEST_TAG),
        ScaPillar::Memory,
        vec![MANIFEST_TAG.to_string(), format!("forge-project:{}", project_name)],
    );
    Ok(())
}

/// Public form for CLI — load the manifest if present.
pub fn load_manifest_public(said: &mut SaidFile, project_name: &str) -> Option<SyncManifest> {
    load_manifest(said, project_name)
}

fn load_manifest(said: &mut SaidFile, project_name: &str) -> Option<SyncManifest> {
    // Resolve the manifest frame by its well-known doc_id, then fetch
    // content via SaidFile::get (which needs &mut self).
    let doc_id = manifest_doc_id(project_name);
    let has_frame = said
        .frames
        .get_all_frames_with_pending()
        .into_iter()
        .any(|m| m.doc_id == doc_id);
    if !has_frame {
        return None;
    }
    let body = said.get(&doc_id)?;
    serde_json::from_str(&body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{apply_scaffold, ApplyMode};

    fn seeded_workspace(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf, WorkspaceConfig) {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        apply_scaffold("w", &root, ApplyMode::Merge).unwrap();
        for (rel, content) in files {
            let p = root.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, content).unwrap();
        }
        let mut cfg = WorkspaceConfig::default();
        cfg.plan_complete = true;
        (tmp, root, cfg)
    }

    #[test]
    fn classify_extension_maps_known_families() {
        assert_eq!(classify_extension(Path::new("x.sql")), SyncKind::CodeSql);
        assert_eq!(classify_extension(Path::new("x.cs")), SyncKind::CodeGeneric);
        assert_eq!(classify_extension(Path::new("x.rs")), SyncKind::CodeGeneric);
        assert_eq!(classify_extension(Path::new("x.md")), SyncKind::DocText);
        assert_eq!(classify_extension(Path::new("x.pdf")), SyncKind::DocBinary);
        assert_eq!(classify_extension(Path::new("x.xlsm")), SyncKind::DocXlsx);
        assert_eq!(classify_extension(Path::new("x.yaml")), SyncKind::Directive);
        assert_eq!(classify_extension(Path::new("binary.bin")), SyncKind::Skip);
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(classify_extension(Path::new("DATA.SQL")), SyncKind::CodeSql);
        assert_eq!(classify_extension(Path::new("doc.PDF")), SyncKind::DocBinary);
    }

    #[test]
    fn authority_resolves_folder_default() {
        let cfg = WorkspaceConfig::default();
        let a = resolve_authority(&cfg, Path::new("3-requirements/BIN.pdf"), "requirements");
        assert_eq!(a.level, "requested");
        assert_eq!(a.scope, "requirements");
        assert_eq!(a.to_tag(), "authority:requested:requirements");
    }

    #[test]
    fn authority_per_file_override_wins() {
        let mut cfg = WorkspaceConfig::default();
        cfg.files.insert(
            "3-requirements/BIN.pdf".into(),
            crate::workspace_config::FileOverride {
                authority: "soft".into(),
            },
        );
        let a = resolve_authority(&cfg, Path::new("3-requirements/BIN.pdf"), "requirements");
        assert_eq!(a.level, "soft");
        let a2 = resolve_authority(&cfg, Path::new("3-requirements/Other.pdf"), "requirements");
        assert_eq!(a2.level, "requested");
    }

    #[test]
    fn build_sync_plan_enumerates_files_with_correct_authority() {
        let (_t, root, cfg) = seeded_workspace(&[
            ("1-ground-truth/schema.sql", "CREATE TABLE t();"),
            ("2-progress/Service.cs", "// code"),
            ("3-requirements/BIN.pdf", "%PDF-"),
            ("4-expectations/api.yaml", "openapi: 3.0.0\n"),
        ]);
        let plan = build_sync_plan(&root, &cfg).unwrap();
        assert_eq!(plan.entries.len(), 4);
        let sql = plan.entries.iter().find(|e| e.rel_path.ends_with("schema.sql")).unwrap();
        assert_eq!(sql.kind, SyncKind::CodeSql);
        assert_eq!(sql.authority.level, "law");
        let cs = plan.entries.iter().find(|e| e.rel_path.ends_with("Service.cs")).unwrap();
        assert_eq!(cs.kind, SyncKind::CodeGeneric);
        assert_eq!(cs.authority.level, "existing");
        let pdf = plan.entries.iter().find(|e| e.rel_path.ends_with("BIN.pdf")).unwrap();
        assert_eq!(pdf.kind, SyncKind::DocBinary);
        assert_eq!(pdf.authority.level, "requested");
        let yaml = plan.entries.iter().find(|e| e.rel_path.ends_with("api.yaml")).unwrap();
        assert_eq!(yaml.kind, SyncKind::Directive);
        assert_eq!(yaml.authority.level, "agreed");
    }

    #[test]
    fn manifest_filter_drops_matching_entries() {
        let mut plan = SyncPlan {
            root: PathBuf::from("/x"),
            entries: vec![SyncEntry {
                path: PathBuf::from("/x/1-ground-truth/a.sql"),
                rel_path: PathBuf::from("1-ground-truth/a.sql"),
                kind: SyncKind::CodeSql,
                authority: ResolvedAuthority {
                    level: "law".into(),
                    scope: "ground-truth".into(),
                },
                size: 42,
                mtime_secs: 1000,
            }],
            skipped_unchanged: Vec::new(),
            orphans: Vec::new(),
        };
        let mut mf = SyncManifest::default();
        mf.files.insert(
            "1-ground-truth/a.sql".into(),
            ManifestEntry {
                size: 42,
                mtime_secs: 1000,
                authority_tag: "authority:law:ground-truth".into(),
            },
        );
        mf.filter_unchanged(&mut plan);
        assert!(plan.entries.is_empty());
        assert_eq!(plan.skipped_unchanged.len(), 1);
    }

    #[test]
    fn manifest_filter_keeps_authority_changes() {
        let mut plan = SyncPlan {
            root: PathBuf::from("/x"),
            entries: vec![SyncEntry {
                path: PathBuf::from("/x/3-requirements/b.pdf"),
                rel_path: PathBuf::from("3-requirements/b.pdf"),
                kind: SyncKind::DocBinary,
                authority: ResolvedAuthority {
                    level: "soft".into(),
                    scope: "requirements".into(),
                },
                size: 100,
                mtime_secs: 2000,
            }],
            skipped_unchanged: Vec::new(),
            orphans: Vec::new(),
        };
        let mut mf = SyncManifest::default();
        mf.files.insert(
            "3-requirements/b.pdf".into(),
            ManifestEntry {
                size: 100,
                mtime_secs: 2000,
                // authority changed (requested → soft)
                authority_tag: "authority:requested:requirements".into(),
            },
        );
        mf.filter_unchanged(&mut plan);
        assert_eq!(plan.entries.len(), 1);
        assert!(plan.skipped_unchanged.is_empty());
    }

    #[test]
    fn execute_sync_writes_frames_with_authority_tags() {
        use sca_core::said_file::{BrainMode, SaidFile};
        let (_t, root, cfg) = seeded_workspace(&[
            ("1-ground-truth/schema.sql", "CREATE TABLE t();"),
            ("3-requirements/notes.md", "# client notes"),
        ]);
        let plan = build_sync_plan(&root, &cfg).unwrap();
        let said_path = root.join("w.said");
        let mut said = SaidFile::create_with_mode(&said_path, BrainMode::Portable);
        let result = execute_sync(&plan, &mut said, "w").unwrap();
        // 2 content frames + 1 manifest = 3 frames written
        assert_eq!(result.frames_written, 2);
        assert_eq!(result.bytes_ingested, ("CREATE TABLE t();".len() + "# client notes".len()) as u64);
        // Every frame we wrote (code + doc) should carry an authority tag.
        let frames = said.frames.get_all_frames_with_pending();
        let auth_tagged = frames
            .iter()
            .filter(|m| m.tags.iter().any(|t| t.starts_with("authority:")))
            .count();
        // 2 content frames have authority tags (manifest frame uses Memory pillar and forge-sync-manifest tag).
        assert_eq!(auth_tagged, 2);
    }

    #[test]
    fn execute_sync_second_run_skips_unchanged_via_manifest() {
        use sca_core::said_file::{BrainMode, SaidFile};
        let (_t, root, cfg) = seeded_workspace(&[
            ("1-ground-truth/schema.sql", "CREATE TABLE t();"),
        ]);
        let said_path = root.join("w.said");
        let mut said = SaidFile::create_with_mode(&said_path, BrainMode::Portable);

        // First sync — ingests
        let plan1 = build_sync_plan(&root, &cfg).unwrap();
        let r1 = execute_sync(&plan1, &mut said, "w").unwrap();
        assert_eq!(r1.frames_written, 1);

        // Second sync — filter via loaded manifest — should skip
        let mut plan2 = build_sync_plan(&root, &cfg).unwrap();
        let mf = load_manifest(&mut said, "w").unwrap_or_default();
        mf.filter_unchanged(&mut plan2);
        assert!(plan2.entries.is_empty(), "second pass should skip unchanged");
        assert_eq!(plan2.skipped_unchanged.len(), 1);
    }
}

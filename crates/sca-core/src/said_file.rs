//! Unified .said file format — the complete brain file.
//!
//! A single .said file contains everything:
//! - Per-frame document storage (zstd + BLAKE3 + optional AES-256-GCM)
//! - SCA search index (1-bit fingerprints, 23 bytes/doc)
//! - Brain layer (query log, reconsolidation, dream drift)
//! - WAL-safe write (atomic rename)
//!
//! File layout:
//! ```
//! [Header 32B]
//!   magic: "SAID" (4B)
//!   version: u16 (6 = unified)
//!   flags: u16
//!   frame_count: u32
//!   scrm_offset: u64   — where SCA index starts
//!   toc_offset: u64    — where frame TOC starts
//!
//! [Frame Data]
//!   frame_0: [compressed+encrypted bytes]
//!   frame_1: [compressed+encrypted bytes]
//!   ...
//!
//! [SCRM Section]
//!   SCA breadcrumbs (fingerprints + corpus_mean + corpus_std)
//!
//! [BRAN Section]
//!   Brain state (query log + recall weights + dream accumulator)
//!
//! [FTOC Section]
//!   Frame table of contents (per-frame metadata)
//!
//! [CRC32 4B]
//! ```
//!
//! Operations:
//! - create(path) → new empty .said file
//! - open(path) → load existing .said file
//! - put(doc_id, content) → add document frame + update SCA index
//! - search(query, top_k) → search via SCA, return doc_ids
//! - read(doc_id) → decompress + decrypt single frame
//! - delete(doc_id) → tombstone frame
//! - save() → flush all pending writes, WAL-safe
//! - dream() → run brain consolidation cycle

use std::path::{Path, PathBuf};
use crate::frames::FrameStore;
use crate::engine::ScaEngine;

const SAID_MAGIC: &[u8; 4] = b"SAID";
const SAID_VERSION: u16 = 7; // v7: all section offsets in header (u64), no magic scanning

/// `build_index` appends new frames incrementally (O(new)) against the persisted
/// corpus mean — until the corpus has grown by more than this fraction since the
/// last full build, at which point it does one full rebuild to re-center the mean
/// (the documented "recompute on growth", 3.1). 0.5 = rebuild after +50% growth.
const RECOMPUTE_GROWTH: f32 = 0.5;

/// Header size for legacy v7 files (4 u64 offsets + 4 reserved bytes).
const HEADER_SIZE_V7: usize = 48;
/// Header size for v7_1 files (72 bytes: adds trgm_offset + syms_offset +
/// refs_offset, each u64). Signaled via flag bit 0 (FLAG_EXTENDED_HEADER).
///
/// Layout:
///   [0..4]   magic "SAID"
///   [4..6]   version u16 (7)
///   [6..8]   flags u16     (bit 0 = extended header present)
///   [8..12]  frame_count u32
///   [12..20] scrm_offset u64
///   [20..28] toc_offset u64
///   [28..36] dict_offset u64
///   [36..44] blkt_offset u64
///   [44..52] trgm_offset u64   — trigram inverted index (0 = absent)
///   [52..60] syms_offset u64   — symbol table (0 = absent)
///   [60..68] refs_offset u64   — reference edges (0 = absent)
///   [68..72] reserved u32
const HEADER_SIZE_V7_1: usize = 72;

/// Header flag bit 0: extended v7_1 header present. v7_1 contains:
///   trgm_offset (trigram index) + syms_offset (symbol table) + refs_offset (ref edges)
/// Any of those offsets may be 0 (section absent).
/// Old v7 files (48-byte header, flag=0) still load with all new sections absent.
const FLAG_EXTENDED_HEADER: u16 = 0x0001;

/// Header flag bit 1: brain operating mode is Enterprise. Clear (the default
/// for every file written before this bit existed) means Portable. Persisting
/// the mode in a spare flag bit keeps the header size identical and is fully
/// backward-compatible: old files have the bit clear and load as Portable,
/// exactly as before.
const FLAG_ENTERPRISE_MODE: u16 = 0x0002;

/// File data backing — either owned bytes (new files) or mmap (opened files).
/// mmap lets the OS page in only the blocks you touch — zero upfront copy.
enum FileData {
    Owned(Vec<u8>),
    Mmap(memmap2::Mmap),
}

impl FileData {
    fn as_slice(&self) -> &[u8] {
        match self {
            FileData::Owned(v) => v,
            FileData::Mmap(m) => m,
        }
    }
    fn len(&self) -> usize {
        match self {
            FileData::Owned(v) => v.len(),
            FileData::Mmap(m) => m.len(),
        }
    }
    /// Bytes held OWNED in process RAM. Mmap is OS-paged from disk → 0 owned. Lets a
    /// test/diagnostic distinguish "we kept the whole file in RAM" (Owned) from "the file
    /// lives on disk and we mmap it" (the memory-bounded post-save state).
    fn owned_len(&self) -> usize {
        match self {
            FileData::Owned(v) => v.len(),
            FileData::Mmap(_) => 0,
        }
    }
}

/// A recalled memory — content + score.
pub struct RecallResult {
    pub doc_id: String,
    pub score: f32,
    pub content: String,
}

/// A symbol lookup result — the definition of a named code entity.
/// Does NOT include the content (call `read(doc_id)` to fetch it).
#[derive(Debug, Clone)]
pub struct SymbolResult {
    /// Symbol name (may differ from query if using prefix/contains fallback).
    pub name: String,
    /// doc_id of the frame holding the definition (e.g. "frames.rs::compact").
    pub doc_id: String,
    /// "fn", "struct", "enum", "trait", "impl", "class", "method", "type", "const", "?"
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// Helper for copying symbols between brains (used by `said snapshot`).
pub struct SymbolSnapshot {
    symbols: Vec<(String, String, crate::symbol_index::SymbolKind, u32, u32)>,
}

impl SymbolSnapshot {
    /// Get all symbols registered for a given doc_id.
    pub fn get(&self, doc_id: &str) -> Vec<(&str, crate::symbol_index::SymbolKind, u32, u32)> {
        self.symbols.iter()
            .filter(|(_, did, _, _, _)| did == doc_id)
            .map(|(name, _, kind, start, end)| (name.as_str(), *kind, *start, *end))
            .collect()
    }
}

/// The unified .said file — a complete portable brain.
/// Brain operating mode. Chosen at `create_with_mode()` time, immutable
/// for the life of the file. Portable brains embed external blob content;
/// Enterprise brains refuse content embeds and only accept pointer writes.
///
/// Licensing + runtime enforcement both key off this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrainMode {
    Portable,
    Enterprise,
}

impl BrainMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "portable" => Some(BrainMode::Portable),
            "enterprise" => Some(BrainMode::Enterprise),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            BrainMode::Portable => "portable",
            BrainMode::Enterprise => "enterprise",
        }
    }
}

impl Default for BrainMode {
    fn default() -> Self {
        BrainMode::Portable
    }
}

/// One Stream-1 (search) source document, as surfaced to the UI. `segments`
/// is the number of indexed paragraphs that belong to this document. The WASM
/// layer maps this into its own serde-serializable shape (mirroring how
/// `vault_list` builds `VaultDoc`).
#[derive(Debug, Clone)]
pub struct SearchDocument {
    pub filename: String,
    pub segments: usize,
}

pub struct SaidFile {
    /// File path
    path: PathBuf,
    /// File data — mmap for opened files, owned Vec for new files.
    /// mmap: OS pages in only the blocks you touch. Zero upfront copy.
    data: FileData,
    /// Frame storage (per-frame zstd + BLAKE3 + encryption)
    pub frames: FrameStore,
    /// SCA search engine (1-bit fingerprints + hybrid scoring)
    pub engine: ScaEngine,
    /// Immutable mode chosen at create time. Default `Portable` on open
    /// for files written before the field existed.
    mode: BrainMode,
    /// Audit log — append-only hash-chained record of mutating operations.
    /// Populated by `remember_with_salience`, `admin_restore`, `mark_frame_deleted`,
    /// legal-hold operations, etc. In-memory only in v7_2; persisted via
    /// a future AUDT section.
    audit: crate::audit::AuditLog,
    /// Whether there are unsaved changes
    dirty: bool,
    /// Cached corpus texts: (doc_ids, texts, texts_lower).
    /// Populated by build_index(). Used by recall() for grep re-rank
    /// without touching disk — matches the Python recall_fused path.
    corpus_ids: Vec<String>,
    corpus_texts: Vec<String>,
    corpus_texts_lower: Vec<String>,
    /// Optional LSP client — enable on the fly for code intelligence
    #[cfg(feature = "lsp")]
    lsp: Option<crate::lsp_client::LspClient>,
    /// Trigram inverted index for fast substring search (grep pre-filter).
    /// Built at compact time, loaded from TRGM section on open.
    /// Maps frame internal index (position in active_doc_ids) -> posting list.
    pub(crate) trigram_index: Option<crate::trigram_index::TrigramIndex>,
    /// Frame doc_id ordering used when the trigram index was built.
    /// Trigram posting lists use positional indices into this list, so we
    /// need to remember the exact order to translate index -> doc_id at
    /// query time. Regenerated whenever the trigram index is rebuilt.
    /// Also shared by the symbol_index — same doc_index space.
    pub(crate) trigram_doc_ids: Vec<String>,
    /// Symbol table: name -> locations (doc_index, kind, start_line, end_line).
    /// Built alongside trigram_index at init time from AST chunk metadata.
    /// Loaded from SYMS section on open.
    pub(crate) symbol_index: Option<crate::symbol_index::SymbolIndex>,
    /// Pending symbols added during init. Consumed when rebuild_trigram_index()
    /// runs at compact time, then moved into `symbol_index`. This lets cmd_init
    /// record AST chunk kind/lines at the moment of frame ingestion, so we
    /// don't need to re-parse files later.
    pub(crate) pending_symbols: Vec<(String, String, crate::symbol_index::SymbolKind, u32, u32)>,
    /// Second SCA engine indexing 512-word passages with 256-word stride.
    /// Used by `recall_fused_long` for queries ≥ 20 words (SummScreenFD,
    /// QMSum, NarrativeQA scale). Rebuilt alongside the doc-level index.
    /// In-memory only — not serialized to .said, rebuilt on first long query
    /// after open if empty. Keeping this off-disk avoids format churn and
    /// keeps the .said file footprint identical to v7_2.
    pub(crate) passage_engine: crate::recall::PassageEngine,
    /// Vault tombstone section — lazily allocated. None for personal
    /// brain files; Some for said-vault files after first access.
    /// Per said-vault Track B spec (Task 3 of the plan).
    vault_tombstones: Option<crate::vault_tombstone::VaultTombstoneStore>,
    /// Streaming-ingest spill budget (#4). When `Some(b)`, the remember/put
    /// path spills the FrameStore's in-RAM `pending` buffer to a scratch file
    /// on disk (then mmaps it) whenever `frames.pending_bytes() > b`, so `said
    /// init` ingests at CONSTANT memory instead of holding the whole corpus in
    /// RAM until save(). `None` = legacy behaviour (hold everything in RAM).
    /// Set via `set_stream_spill_budget`. See `spill_pending_to_disk`.
    stream_spill_budget: Option<usize>,
    /// Absolute byte offset of the current end of the spill scratch file, i.e.
    /// the base offset the NEXT spill batch will be written at. Starts at
    /// HEADER_SIZE_V7_1 (a placeholder header is laid down on the first spill so
    /// committed frame offsets are always > 0, matching save()'s `offset > 0`
    /// filter). Only meaningful while `stream_spill_budget` is Some.
    spill_offset: u64,
}

impl SaidFile {
    /// Create a new empty .said file (defaults to Portable mode).
    pub fn create(path: impl AsRef<Path>) -> Self {
        Self::create_with_mode(path, BrainMode::Portable)
    }

    /// Create a new empty .said file with an explicit mode. Mode is
    /// immutable for the life of the file.
    pub fn create_with_mode(path: impl AsRef<Path>, mode: BrainMode) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            data: FileData::Owned(Vec::new()),
            frames: FrameStore::new(),
            engine: ScaEngine::new(),
            mode,
            audit: crate::audit::AuditLog::new(),
            dirty: false,
            corpus_ids: Vec::new(),
            corpus_texts: Vec::new(),
            corpus_texts_lower: Vec::new(),
            #[cfg(feature = "lsp")]
            lsp: None,
            trigram_index: None,
            trigram_doc_ids: Vec::new(),
            symbol_index: None,
            pending_symbols: Vec::new(),
            passage_engine: crate::recall::PassageEngine::new(),
            vault_tombstones: None,
            stream_spill_budget: None,
            spill_offset: HEADER_SIZE_V7_1 as u64,
        }
    }

    /// Current mode of this brain (immutable for the life of the file).
    pub fn mode(&self) -> BrainMode {
        self.mode
    }

    /// Header flags u16 to write at bytes [6..8] on a fresh full save.
    /// Always sets the extended-header bit; sets the Enterprise-mode bit
    /// when this brain is Enterprise so the mode survives save/reload.
    fn header_flags(&self) -> u16 {
        let mut flags = FLAG_EXTENDED_HEADER;
        if self.mode == BrainMode::Enterprise {
            flags |= FLAG_ENTERPRISE_MODE;
        }
        flags
    }

    /// Returns true if the vault tombstone section has been allocated.
    /// Personal brain files always return false; vault files return true
    /// once any tombstone has been written.
    pub fn has_vault_tombstones(&self) -> bool {
        self.vault_tombstones.is_some()
    }

    /// Immutable accessor — returns None if the section was never allocated.
    pub fn vault_tombstones(&self) -> Option<&crate::vault_tombstone::VaultTombstoneStore> {
        self.vault_tombstones.as_ref()
    }

    /// Mutable accessor — lazily allocates the section on first call.
    /// Calling this on a personal brain file converts it to a vault-shaped
    /// file. Callers should only invoke this on files intended for vault use.
    pub fn vault_tombstones_mut(&mut self) -> &mut crate::vault_tombstone::VaultTombstoneStore {
        self.vault_tombstones.get_or_insert_with(crate::vault_tombstone::VaultTombstoneStore::new)
    }

    /// Record a symbol definition discovered during AST chunking.
    /// Called by `cmd_init` / `cmd_add` right after `remember_as` for each
    /// code chunk. Stored in `pending_symbols` until `rebuild_trigram_index()`
    /// consumes them at compact time (translating doc_ids to positions).
    pub fn record_symbol(
        &mut self,
        name: &str,
        doc_id: &str,
        kind: crate::symbol_index::SymbolKind,
        start_line: u32,
        end_line: u32,
    ) {
        self.pending_symbols.push((name.to_string(), doc_id.to_string(), kind, start_line, end_line));
    }

    /// Snapshot all symbols — returns a helper to query by doc_id.
    /// Used by `said snapshot` to copy symbols into module brains.
    /// Reads from both pending_symbols (during init) and the persisted
    /// symbol_index (after save/open).
    pub fn symbols_snapshot(&self) -> SymbolSnapshot {
        let mut symbols = self.pending_symbols.clone();

        // Also read from persisted symbol index (populated after save + open)
        if let Some(ref idx) = self.symbol_index {
            for (name, entries) in idx.all_entries() {
                for e in entries {
                    if let Some(did) = self.trigram_doc_ids.get(e.doc_index as usize) {
                        symbols.push((name.clone(), did.clone(), e.kind, e.start_line, e.end_line));
                    }
                }
            }
        }

        SymbolSnapshot { symbols }
    }

    /// Open an existing .said file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();

        // WAL check: if .said.tmp exists, previous write crashed
        let tmp_path = format!("{}.tmp", path.display());
        if Path::new(&tmp_path).exists() {
            eprintln!("[WAL] Found {}, previous write crashed. Using last good file.", tmp_path);
            let _ = std::fs::remove_file(&tmp_path);
        }

        // mmap the file — OS pages in only the blocks we touch
        let file = std::fs::File::open(&path)
            .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
        let mmap = unsafe { memmap2::Mmap::map(&file) }
            .map_err(|e| format!("mmap failed: {}", e))?;
        let data = FileData::Mmap(mmap);
        Self::parse_data(path, data)
    }

    /// Shared parse logic for `open()` (mmap) and `from_bytes()` (owned Vec).
    fn parse_data(path: std::path::PathBuf, data: FileData) -> Result<Self, String> {

        // Minimum header size is v7 (48 bytes). v7_1 files are 56 bytes with
        // an extended header (flag bit 0 set).
        if data.len() < HEADER_SIZE_V7 {
            return Err("File too small for .said header".into());
        }

        // Use slice for all parsing (works for both mmap and owned)
        let data_bytes = data.as_slice();

        // Verify magic
        if &data_bytes[0..4] != SAID_MAGIC {
            return Err("Not a .said file (bad magic)".into());
        }

        let version = u16::from_le_bytes([data_bytes[4], data_bytes[5]]);
        if version != SAID_VERSION && version != 6 {
            return Err(format!("Unsupported .said version {} (expected {} or 6)", version, SAID_VERSION));
        }

        // Verify CRC32 (last 4 bytes)
        if data_bytes.len() >= 4 {
            let stored_crc = u32::from_le_bytes(data_bytes[data_bytes.len()-4..].try_into().unwrap());
            let computed_crc = crate::state::crc32_simple(&data_bytes[..data_bytes.len()-4]);
            if stored_crc != computed_crc {
                return Err("CRC32 mismatch — .said file corrupted".into());
            }
        }

        // Parse header
        let flags = u16::from_le_bytes([data_bytes[6], data_bytes[7]]);
        let _frame_count = u32::from_le_bytes(data_bytes[8..12].try_into().unwrap());
        let scrm_offset = u64::from_le_bytes(data_bytes[12..20].try_into().unwrap()) as usize;
        let toc_offset = u64::from_le_bytes(data_bytes[20..28].try_into().unwrap()) as usize;
        let (dict_offset, blkt_offset) = if version >= 7 && data_bytes.len() >= 44 {
            (
                u64::from_le_bytes(data_bytes[28..36].try_into().unwrap()) as usize,
                u64::from_le_bytes(data_bytes[36..44].try_into().unwrap()) as usize,
            )
        } else {
            (0, 0) // v6: no offsets in header
        };
        // v7_1 extended header: trgm_offset + syms_offset + refs_offset (each u64).
        // Old v7 files have flag bit 0 = 0 and only a 48-byte header.
        let (trgm_offset, syms_offset, refs_offset): (usize, usize, usize) =
            if (flags & FLAG_EXTENDED_HEADER) != 0 && data_bytes.len() >= HEADER_SIZE_V7_1 {
                (
                    u64::from_le_bytes(data_bytes[44..52].try_into().unwrap()) as usize,
                    u64::from_le_bytes(data_bytes[52..60].try_into().unwrap()) as usize,
                    u64::from_le_bytes(data_bytes[60..68].try_into().unwrap()) as usize,
                )
            } else {
                (0, 0, 0)
            };
        let _ = refs_offset; // reserved for future REFS section

        // Load DICT section (offset from header — no scanning, no false positives)
        let mut zstd_dict: Option<Vec<u8>> = None;
        if dict_offset > 0 && dict_offset + 8 < data_bytes.len() {
            if &data_bytes[dict_offset..dict_offset+4] == b"DICT" {
                let dict_len = u32::from_le_bytes(data_bytes[dict_offset+4..dict_offset+8].try_into().unwrap()) as usize;
                if dict_offset + 8 + dict_len <= data_bytes.len() {
                    zstd_dict = Some(data_bytes[dict_offset+8..dict_offset+8+dict_len].to_vec());
                }
            }
        }

        // Load SCRM (SCA index)
        let mut engine = ScaEngine::new();
        if scrm_offset > 0 && scrm_offset < data_bytes.len() {
            if &data_bytes[scrm_offset..scrm_offset+4] == b"SCRM" {
                let _ = engine.core.deserialize_breadcrumbs(&data_bytes[scrm_offset..toc_offset.min(data_bytes.len())]);
            }
        }

        // Load Brain (BRAN section — between SCRM and FTOC)
        if let Some(bran_pos) = data_bytes[scrm_offset..].windows(4).position(|w| w == b"BRAN") {
            let abs_pos = scrm_offset + bran_pos;
            if let Ok(brain) = crate::brain::Brain::deserialize(&data_bytes[abs_pos..]) {
                engine.brain = brain;
            }
        }

        // Load Frame TOC
        let mut frames = if toc_offset > 0 && toc_offset < data_bytes.len() {
            FrameStore::deserialize_toc(&data_bytes[toc_offset..data_bytes.len()-4])
                .unwrap_or_else(|_| FrameStore::new())
        } else {
            FrameStore::new()
        };

        // Restore Zstd dictionary
        if let Some(dict) = zstd_dict {
            frames.set_dictionary(dict);
        }

        // Load BLKT section (offset from header — no scanning, no false positives)
        if blkt_offset > 0 && blkt_offset < data_bytes.len() {
            let _ = frames.deserialize_block_table(&data_bytes[blkt_offset..scrm_offset.min(data_bytes.len())]);
        }

        // Load TRGM section (v7_1 trigram inverted index).
        // Layout: b"TRGM" (4) | u64 uncompressed_len | u64 compressed_len | doc_ids_list | zstd(postings)
        // The doc_ids_list maps positional indices to frame doc_ids.
        let mut symbol_index: Option<crate::symbol_index::SymbolIndex> = None;
        let mut trigram_index: Option<crate::trigram_index::TrigramIndex> = None;
        let mut trigram_doc_ids: Vec<String> = Vec::new();
        if trgm_offset > 0 && trgm_offset + 20 < data_bytes.len() {
            if &data_bytes[trgm_offset..trgm_offset+4] == b"TRGM" {
                let mut p = trgm_offset + 4;
                // doc_ids count + per-id (u16 len + utf8 bytes)
                let n_ids = u32::from_le_bytes(data_bytes[p..p+4].try_into().unwrap()) as usize;
                p += 4;
                let mut ok_ids = true;
                for _ in 0..n_ids {
                    if p + 2 > data_bytes.len() { ok_ids = false; break; }
                    let id_len = u16::from_le_bytes(data_bytes[p..p+2].try_into().unwrap()) as usize;
                    p += 2;
                    if p + id_len > data_bytes.len() { ok_ids = false; break; }
                    trigram_doc_ids.push(String::from_utf8_lossy(&data_bytes[p..p+id_len]).to_string());
                    p += id_len;
                }
                // Now the compressed posting blob: u32 uncompressed_len, u32 compressed_len, bytes
                if ok_ids && p + 8 <= data_bytes.len() {
                    let uncompressed_len = u32::from_le_bytes(data_bytes[p..p+4].try_into().unwrap()) as usize;
                    let compressed_len = u32::from_le_bytes(data_bytes[p+4..p+8].try_into().unwrap()) as usize;
                    p += 8;
                    if p + compressed_len <= data_bytes.len() {
                        if let Ok(raw) = zstd::bulk::decompress(&data_bytes[p..p+compressed_len], uncompressed_len) {
                            if let Ok(idx) = crate::trigram_index::TrigramIndex::deserialize_raw(&raw) {
                                trigram_index = Some(idx);
                            }
                        }
                    }
                }
                // If parse failed, leave index as None — grep falls back to scan
                if trigram_index.is_none() {
                    trigram_doc_ids.clear();
                }
            }
        }

        // Load SYMS section (v7_1 symbol table).
        // Layout: b"SYMS" | u32 uncompressed_len | u32 compressed_len | zstd(raw)
        if syms_offset > 0 && syms_offset + 12 < data_bytes.len() {
            if &data_bytes[syms_offset..syms_offset+4] == b"SYMS" {
                let uncompressed_len = u32::from_le_bytes(data_bytes[syms_offset+4..syms_offset+8].try_into().unwrap()) as usize;
                let compressed_len = u32::from_le_bytes(data_bytes[syms_offset+8..syms_offset+12].try_into().unwrap()) as usize;
                let blob_start = syms_offset + 12;
                let blob_end = blob_start + compressed_len;
                if blob_end <= data_bytes.len() {
                    if let Ok(raw) = zstd::bulk::decompress(&data_bytes[blob_start..blob_end], uncompressed_len) {
                        if let Ok(si) = crate::symbol_index::SymbolIndex::deserialize_raw(&raw) {
                            symbol_index = Some(si);
                        }
                    }
                }
            }
        }

        // Load CTXT section (cached corpus texts — instant search without block decompression)
        let mut corpus_ids = Vec::new();
        let mut corpus_texts = Vec::new();
        let mut corpus_texts_lower = Vec::new();
        {
            let scan_start = scrm_offset;
            let scan_end = toc_offset.min(data_bytes.len());
            for pos in scan_start..scan_end.saturating_sub(12) {
                if &data_bytes[pos..pos+4] == b"CTXT" {
                    let compressed_len = u32::from_le_bytes(data_bytes[pos+4..pos+8].try_into().unwrap()) as usize;
                    let uncompressed_len = u32::from_le_bytes(data_bytes[pos+8..pos+12].try_into().unwrap()) as usize;
                    if pos + 12 + compressed_len <= scan_end {
                        if let Ok(raw) = zstd::bulk::decompress(&data_bytes[pos+12..pos+12+compressed_len], uncompressed_len) {
                            let mut rpos = 0;
                            if rpos + 4 <= raw.len() {
                                let n = u32::from_le_bytes(raw[rpos..rpos+4].try_into().unwrap()) as usize;
                                rpos += 4;
                                // Read corpus_ids + corpus_texts_lower
                                for _ in 0..n {
                                    if rpos + 2 > raw.len() { break; }
                                    let id_len = u16::from_le_bytes(raw[rpos..rpos+2].try_into().unwrap()) as usize;
                                    rpos += 2;
                                    if rpos + id_len > raw.len() { break; }
                                    let id = String::from_utf8_lossy(&raw[rpos..rpos+id_len]).to_string();
                                    corpus_ids.push(id.clone());
                                    // corpus_texts populated with empty — read from frames on demand
                                    corpus_texts.push(String::new());
                                    rpos += id_len;
                                    if rpos + 4 > raw.len() { break; }
                                    let text_len = u32::from_le_bytes(raw[rpos..rpos+4].try_into().unwrap()) as usize;
                                    rpos += 4;
                                    if rpos + text_len > raw.len() { break; }
                                    corpus_texts_lower.push(String::from_utf8_lossy(&raw[rpos..rpos+text_len]).to_string());
                                    rpos += text_len;
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }

        // Also rebuild engine's normalized texts from cached corpus (for entity matching)
        for text in &corpus_texts {
            engine.doc_texts_normalized.push(
                text.chars()
                    .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
                    .collect::<String>()
                    .to_lowercase()
            );
        }

        // Load VAULT_TOMBSTONES section (said-vault Track B).
        // Discovered by scanning forward from scrm_offset to toc_offset,
        // same strategy as CTXT. Magic b"VTS1" is self-identifying.
        let mut vault_tombstones: Option<crate::vault_tombstone::VaultTombstoneStore> = None;
        {
            let scan_start = scrm_offset;
            let scan_end = toc_offset.min(data_bytes.len());
            // Need at least 8 bytes for magic(4) + header. Match either the
            // current VTS2 (compressed) or legacy VTS1 (raw) magic.
            for pos in scan_start..scan_end.saturating_sub(8) {
                let m = &data_bytes[pos..pos+4];
                if m == crate::vault_tombstone::SECTION_MAGIC
                    || m == crate::vault_tombstone::SECTION_MAGIC_V1
                {
                    match crate::vault_tombstone::VaultTombstoneStore::from_bytes(&data_bytes[pos..scan_end]) {
                        Ok(store) => { vault_tombstones = Some(store); }
                        Err(e) => {
                            eprintln!("[said] VAULT_TOMBSTONES section failed to parse: {}; skipping", e);
                        }
                    }
                    break;
                }
            }
        }

        Ok(Self {
            path,
            data,
            frames,
            engine,
            // Mode is persisted in header flag bit 1 (FLAG_ENTERPRISE_MODE).
            // Old files (and any file written as Portable) have the bit clear
            // and load as Portable — backward-compatible.
            mode: if (flags & FLAG_ENTERPRISE_MODE) != 0 {
                BrainMode::Enterprise
            } else {
                BrainMode::Portable
            },
            audit: crate::audit::AuditLog::new(),
            dirty: false,
            corpus_ids,
            corpus_texts,
            corpus_texts_lower,
            #[cfg(feature = "lsp")]
            lsp: None,
            trigram_index,
            trigram_doc_ids,
            symbol_index,
            pending_symbols: Vec::new(),
            passage_engine: crate::recall::PassageEngine::new(),
            vault_tombstones,
            stream_spill_budget: None,
            spill_offset: HEADER_SIZE_V7_1 as u64,
        })
    }

    /// Remember something. Just text. The brain handles everything else.
    ///
    /// NEVER chunks. Stores the entire document as one frame. The passage
    /// engine handles long-document search at query time via sliding window.
    ///
    /// Benchmark (2M words, 13 MB):
    ///   No chunking: 74s ingest, 13 MB file, 1,140ms search
    ///   32K chunking: 161s ingest, 26 MB file, 3,321ms search
    ///   No-chunk is 2x faster ingest, 2x smaller file, 3x faster search.
    pub fn remember(&mut self, content: &str) -> u64 {
        let base_id = format!("mem_{}", self.frames.total_count());
        let frame_id = self.frames.put(&base_id, content.as_bytes(), None);
        self.dirty = true;
        frame_id
    }

    /// Remember with a specific ID. Never chunks — stores whole.
    pub fn remember_as(&mut self, doc_id: &str, content: &str, title: Option<&str>) -> u64 {
        // Parse [[wikilinks]] → link:<concept> tags so the CLI/dir ingest path also gets
        // build-graph edges (same as remember_with_salience). Single chokepoint for all
        // plain ingest. No links → no tags → identical to before.
        let links = crate::ask::parse_wikilinks(content);
        let frame_id = if links.is_empty() {
            self.frames.put(doc_id, content.as_bytes(), title)
        } else {
            let tags: Vec<String> = links.into_iter().map(|c| format!("link:{}", c)).collect();
            let opts = crate::frames::PutOptions {
                doc_id, content, title,
                memory_type: crate::frames::MemoryType::Episodic,
                memory_kind: crate::frames::MemoryKind::Fact,
                subject: crate::frames::MemorySubject::User,
                scope: crate::frames::MemoryScope::Personal,
                tags,
            };
            self.frames.put_with(&opts)
        };
        self.dirty = true;
        // Streaming-ingest spill (#4): `said init` ingests via remember_as, so
        // the budget must be honoured on this path too (not just the salience
        // facade). No-op unless set_stream_spill_budget was called.
        self.maybe_spill_pending();
        frame_id
    }

    /// One row per Stream-1 (search) source document.
    ///
    /// Search ingest writes one frame per paragraph as `{base}::para_{i}`,
    /// carrying the original filename as the frame title. This groups those
    /// per-paragraph frames back into their source document so the UI can show
    /// "what did I ingest for search" without exposing every paragraph. The
    /// display name is the frame title (the original filename) when present,
    /// else the `{base}` id. Non-`::para_` frames (ordinary memories) are
    /// ignored.
    pub fn list_search_documents(&self) -> Vec<SearchDocument> {
        use std::collections::BTreeMap;
        // base id -> (display filename, segment count)
        let mut groups: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for doc_id in self.frames.active_doc_ids() {
            let Some((base, _para)) = doc_id.split_once("::para_") else {
                continue;
            };
            let filename = self
                .frames
                .get_meta(doc_id)
                .and_then(|m| m.title.clone())
                .unwrap_or_else(|| base.to_string());
            let entry = groups
                .entry(base.to_string())
                .or_insert_with(|| (filename, 0));
            entry.1 += 1;
        }
        groups
            .into_values()
            .map(|(filename, segments)| SearchDocument { filename, segments })
            .collect()
    }

    /// Delete every Stream-1 (search) frame belonging to one source document,
    /// identified by its original filename. Search ingest keys frames as
    /// `{base}::para_{i}` where `base = filename.replace('.', '_')`, carrying
    /// the filename as the frame title. We match on EITHER the base prefix or
    /// the title so a doc ingested for search is fully purged (e.g. when its
    /// vault copy is erased for GDPR). Returns the number of frames removed.
    pub fn forget_search_document(&mut self, filename: &str) -> usize {
        let base = filename.replace('.', "_");
        let prefix = format!("{}::para_", base);
        let targets: Vec<String> = self
            .frames
            .active_doc_ids()
            .iter()
            .filter(|doc_id| {
                if !doc_id.contains("::para_") {
                    return false;
                }
                if doc_id.starts_with(&prefix) {
                    return true;
                }
                // Fall back to the frame title (handles filenames whose base
                // doesn't round-trip cleanly through the '.'→'_' rule).
                self.frames
                    .get_meta(doc_id)
                    .and_then(|m| m.title.as_deref())
                    == Some(filename)
            })
            .map(|s| s.to_string())
            .collect();
        let mut removed = 0;
        for id in targets {
            if self.forget(&id) {
                removed += 1;
            }
        }
        removed
    }

    /// Remember with an explicit pillar + tags + optional doc_id + title.
    ///
    /// Used by the per-pillar writer surface (session_end, tool_completion,
    /// forge, migrations, admin imports). The `doc_id` is optional — when
    /// None, an auto-generated `mem_<N>` id is assigned. The pillar is
    /// persisted via `FrameStore::set_pillar` after the frame is written.
    ///
    /// Returns the new frame id (opaque `u64`).
    pub fn remember_with_pillar(
        &mut self,
        doc_id: Option<&str>,
        content: &str,
        title: Option<&str>,
        pillar: crate::frames::Pillar,
        tags: Vec<String>,
    ) -> u64 {
        let resolved_id: String = match doc_id {
            Some(id) => id.to_string(),
            None => format!("mem_{}", self.frames.total_count()),
        };
        let opts = crate::frames::PutOptions {
            doc_id: &resolved_id,
            content,
            title,
            memory_type: crate::frames::MemoryType::Episodic,
            memory_kind: crate::frames::MemoryKind::Fact,
            subject: crate::frames::MemorySubject::User,
            scope: crate::frames::MemoryScope::Personal,
            tags,
        };
        let frame_id = self.frames.put_with_pillar(&opts, pillar);
        self.dirty = true;
        // Streaming-ingest spill (#4): once the in-RAM `pending` buffer crosses
        // the budget, flush it to a scratch file on disk and mmap it so reads
        // come from the OS page cache, not process RAM. This is the single
        // choke-point through which every remember_* path funnels, so it's the
        // one place the budget needs to be checked.
        self.maybe_spill_pending();
        frame_id
    }

    /// Set a streaming-ingest spill budget in bytes (#4). After this is set,
    /// the remember/put path keeps the in-RAM `pending` buffer near `bytes` by
    /// spilling to disk, so `said init` over a large corpus runs at constant
    /// memory instead of holding every frame in RAM until save().
    pub fn set_stream_spill_budget(&mut self, bytes: usize) {
        self.stream_spill_budget = Some(bytes);
    }

    /// Bytes currently held in the in-RAM `pending` frame buffer. Diagnostic /
    /// test hook for the streaming-spill path — should stay near the spill
    /// budget once one is set. Delegates to `FrameStore::pending_bytes`.
    pub fn pending_bytes(&self) -> usize {
        self.frames.pending_bytes()
    }

    /// Spill the pending buffer to disk if a budget is set and exceeded (#4).
    fn maybe_spill_pending(&mut self) {
        if let Some(budget) = self.stream_spill_budget {
            if self.frames.pending_bytes() > budget {
                if let Err(e) = self.spill_pending_to_disk() {
                    // Spill is an optimization, never a correctness requirement:
                    // the frames remain in `pending` and will be written by
                    // save() regardless. Warn and keep going (RAM may climb).
                    eprintln!("[SAID] pending spill failed (continuing in RAM): {}", e);
                }
            }
        }
    }

    /// Append the current pending frames' bytes to a scratch spill file and
    /// mmap it, moving those frames pending → committed (#4 streaming index).
    ///
    /// Invariants this relies on:
    ///   - `FrameStore::flush_pending(base)` writes each pending frame's bytes
    ///     contiguously from `base`, sets `meta.offset` to its ABSOLUTE file
    ///     offset, moves pending → committed, and rebuilds doc_id_map.
    ///   - `read_frame` resolves a committed frame by slicing `file_data`
    ///     (== `self.data`) at `[meta.offset .. meta.offset + compressed_len]`.
    ///     So as long as `self.data` mmaps the spill file and the frame bytes
    ///     live at `meta.offset` in it, spilled frames read back correctly —
    ///     from the OS page cache, NOT owned process RAM.
    ///   - save()'s uncompacted path and `compact_block_dict` both re-read
    ///     committed frame bytes from `self.data` at `meta.offset`; spilled
    ///     frames satisfy both, so the final save round-trips unchanged.
    ///
    /// On the FIRST spill the scratch file is SEEDED with the current
    /// `self.data` bytes — for a re-init over an existing brain that is the
    /// whole prior .said, whose committed frames carry offsets into it. We then
    /// replace `self.data` with the spill mmap, so those existing offsets MUST
    /// stay valid; seeding keeps byte N at byte N. New spilled frames append
    /// after. For a fresh `create()` (`self.data` empty) a HEADER_SIZE_V7_1
    /// zero placeholder is laid down instead so every committed offset is > 0
    /// (save()'s uncompacted path filters frames with `offset > 0`). The
    /// scratch file is NOT a valid .said — only a byte container for offset
    /// resolution — and is removed by save() after it copies the bytes out.
    fn spill_pending_to_disk(&mut self) -> Result<(), String> {
        use std::io::Write;

        let spill_path = self.spill_scratch_path();

        // We track end-of-file in `self.spill_offset` rather than stat-ing.
        let first_spill = !std::path::Path::new(&spill_path).exists();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spill_path)
            .map_err(|e| format!("open spill file: {}", e))?;
        if first_spill {
            let existing_len = self.data.len();
            if existing_len >= HEADER_SIZE_V7_1 {
                // Re-init: seed with the prior file so existing committed
                // offsets remain valid once we swap `self.data` to the spill.
                file.write_all(self.data.as_slice()).map_err(|e| format!("seed spill from existing data: {}", e))?;
                self.spill_offset = existing_len as u64;
            } else {
                // Fresh brain: zero placeholder header, never parsed.
                let header = vec![0u8; HEADER_SIZE_V7_1];
                file.write_all(&header).map_err(|e| format!("write spill header: {}", e))?;
                self.spill_offset = HEADER_SIZE_V7_1 as u64;
            }
        }

        // Commit pending → bytes at the current absolute end-of-file offset.
        let base = self.spill_offset;
        let bytes = self.frames.flush_pending(base);
        if bytes.is_empty() {
            return Ok(());
        }
        file.write_all(&bytes).map_err(|e| format!("append spill bytes: {}", e))?;
        file.flush().map_err(|e| format!("flush spill file: {}", e))?;
        self.spill_offset = base + bytes.len() as u64;
        drop(file);
        drop(bytes); // release the owned pending copy — this is the whole point

        // Re-mmap the now-larger scratch file so committed reads page in from
        // disk. Replaces any previous owned/mmap `data` view; the previous
        // mmap (if any) is dropped here.
        let f = std::fs::File::open(&spill_path)
            .map_err(|e| format!("reopen spill file: {}", e))?;
        let mmap = unsafe { memmap2::Mmap::map(&f) }
            .map_err(|e| format!("mmap spill file: {}", e))?;
        self.data = FileData::Mmap(mmap);
        Ok(())
    }

    /// Path of the scratch spill file used during streaming ingest (#4).
    /// Sits next to the target .said as `<path>.spill`.
    fn spill_scratch_path(&self) -> String {
        format!("{}.spill", self.path.display())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Decision-1..5 facade methods. These wire handler.rs / admin surface
    // to the underlying FrameStore / audit / salience / dream subsystems.
    // ─────────────────────────────────────────────────────────────────────

    /// Remember + compute salience + return both the frame id and the
    /// Salience scoring result. Caller-provided extra tags are merged
    /// with salience.tags; the band tag is added automatically.
    pub fn remember_with_salience(
        &mut self,
        doc_id: Option<&str>,
        content: &str,
        title: Option<&str>,
        pillar: crate::frames::Pillar,
        extra_tags: Vec<String>,
    ) -> (u64, crate::salience::Salience) {
        let scored = crate::salience::score_turn(content, pillar);
        let mut tags = extra_tags;
        let band = scored.band_tag();
        if !tags.iter().any(|t| t == &band) {
            tags.push(band);
        }
        for t in &scored.tags {
            if !tags.iter().any(|x| x == t) {
                tags.push(t.clone());
            }
        }
        // Parse [[wikilinks]] from the body into `link:<concept>` tags — the documented
        // build-graph edge (3.9). This is the OKF cross-link → .said edge bridge: a note
        // "Dr. Sarah is the cardiologist [[heart]]" gets a `link:heart` tag, so a query
        // about "heart" can reach it through the explicit concept edge even though the
        // body never says "heart" (out-of-scope for the bi-encoder; in-scope via the link).
        for concept in crate::ask::parse_wikilinks(content) {
            let tag = format!("link:{}", concept);
            if !tags.iter().any(|x| x == &tag) {
                tags.push(tag);
            }
        }
        let frame_id = self.remember_with_pillar(doc_id, content, title, pillar, tags);
        let target = doc_id.map(|s| s.to_string()).unwrap_or_else(|| format!("frame#{}", frame_id));
        self.audit.append("remember", &target, &format!("pillar={:?} salience={}", pillar, scored.score));
        (frame_id, scored)
    }

    /// Write an External-pillar pointer frame (Enterprise ingest path).
    /// Body is the caller-provided `summary_text`; the `uri` and optional
    /// MIME are stored as `external:uri=…` / `external:mime=…` tags, and
    /// an `external:pointer` marker tag is always added so the row can be
    /// cheaply distinguished from a content embed.
    pub fn remember_as_external_pointer(
        &mut self,
        doc_id: Option<&str>,
        uri: &str,
        mime: Option<&str>,
        title: Option<&str>,
        summary_text: &str,
        extra_tags: Vec<String>,
    ) -> u64 {
        let mut tags = extra_tags;
        tags.push("external:pointer".to_string());
        tags.push(format!("external:uri={}", uri));
        if let Some(m) = mime {
            tags.push(format!("external:mime={}", m));
        }
        self.remember_with_pillar(doc_id, summary_text, title, crate::frames::Pillar::External, tags)
    }

    /// Enterprise mode guard. Content-embedding ingest is only allowed in
    /// Portable mode. Returns an error describing the refusal for the
    /// caller to propagate.
    pub fn ensure_content_ingest_allowed(&self) -> Result<(), String> {
        match self.mode {
            BrainMode::Portable => Ok(()),
            BrainMode::Enterprise => Err(
                "Enterprise brain refuses content embed. Use pointer=true or switch to portable.".into(),
            ),
        }
    }

    /// Pillar-scoped semantic search. Returns `(doc_id, score)` pairs.
    /// When `pillars` is `None`, unfiltered full search. Otherwise
    /// post-filters the result set by `FrameMeta.pillar`.
    pub fn search_by_pillar(
        &mut self,
        query: &str,
        top_k: usize,
        pillars: Option<&std::collections::HashSet<crate::frames::Pillar>>,
    ) -> Vec<(String, f32)> {
        let hits = self.recall(query, top_k * 4);
        let filtered: Vec<(String, f32)> = hits
            .into_iter()
            .filter(|r| match pillars {
                None => true,
                Some(set) => self
                    .frames
                    .get_meta(&r.doc_id)
                    .map(|m| set.contains(&m.pillar))
                    .unwrap_or(false),
            })
            .take(top_k)
            .map(|r| (r.doc_id, r.score))
            .collect();
        filtered
    }

    /// Pillar-scoped recall. Builds `RecallResult` rows matching the shape
    /// `recall()` returns.
    pub fn recall_by_pillar(
        &mut self,
        query: &str,
        top_k: usize,
        pillars: Option<&std::collections::HashSet<crate::frames::Pillar>>,
    ) -> Vec<RecallResult> {
        let hits = self.search_by_pillar(query, top_k, pillars);
        let out: Vec<RecallResult> = hits.into_iter()
            .map(|(doc_id, score)| RecallResult {
                doc_id: doc_id.clone(),
                content: self.get(&doc_id).unwrap_or_default(),
                score,
            })
            .collect();
        // Auto-dream in core — so MCP `search` (which calls this, not ask()) evolves
        // brain state without a manual trigger in the handler.
        self.maybe_dream();
        out
    }

    /// Delete a specific frame (by `frame_id`, not `doc_id`). Used by
    /// admin tooling for fine-grained cleanup. Returns whether anything
    /// was changed.
    pub fn mark_frame_deleted(&mut self, frame_id: u64) -> bool {
        let changed = self.frames.mark_frame_deleted(frame_id);
        if changed {
            self.audit.append("frame_deleted", &format!("frame#{}", frame_id), "");
            self.dirty = true;
        }
        changed
    }

    /// List every tombstoned frame (admin surface).
    pub fn admin_tombstones(&self) -> Vec<&crate::frames::FrameMeta> {
        self.frames.admin_tombstone_records()
    }

    /// Restore a tombstoned frame. Returns `(restored_frame_id, displaced_active_id)`.
    /// If an active frame for the same doc_id exists, it is demoted to tombstone
    /// so the restore becomes the new head.
    pub fn admin_restore(&mut self, doc_id: &str) -> Result<(u64, Option<u64>), String> {
        let r = self.frames.admin_restore_tombstoned(doc_id)?;
        self.audit.append("restore", doc_id, &format!("restored frame#{}", r.0));
        self.dirty = true;
        Ok(r)
    }

    /// Place a legal hold on every frame for a doc_id. Returns count changed.
    pub fn admin_legal_hold_add(&mut self, doc_id: &str, case_id: &str) -> usize {
        let n = self.frames.admin_add_legal_hold(doc_id, case_id);
        if n > 0 {
            self.audit.append("legal_hold_add", doc_id, &format!("case={} n={}", case_id, n));
            self.dirty = true;
        }
        n
    }

    /// Release a legal hold. Returns count changed.
    pub fn admin_legal_hold_release(&mut self, doc_id: &str, case_id: &str) -> usize {
        let n = self.frames.admin_release_legal_hold(doc_id, case_id);
        if n > 0 {
            self.audit.append("legal_hold_release", doc_id, &format!("case={} n={}", case_id, n));
            self.dirty = true;
        }
        n
    }

    /// Access the audit log (read-only facade).
    pub fn audit(&self) -> &crate::audit::AuditLog {
        &self.audit
    }

    /// Run a content-consolidation dream pass. In v1 this was a no-op
    /// stub (Decision-5 v2 gutted the content half); returning a minimal
    /// report is the honest behaviour until the v3 implementation lands.
    pub fn run_dream_content(
        &mut self,
        cycle: crate::dream::DreamCycle,
        _params: &crate::dream::DreamParams,
    ) -> crate::dream::DreamReport {
        self.audit.append("dream_content", &format!("cycle={}", cycle), "noop-stub");
        crate::dream::DreamReport {
            cycle,
            ..Default::default()
        }
    }

    /// Recall: proven 300/300 pipeline (ported from recall_fused).
    ///
    /// Layer 1: SCA 1-bit search (top-50 from fingerprints, < 1ms, zero decompression)
    /// Layer 2: Grep re-rank on CACHED texts (morphological expansion + AND pairs)
    /// Layer 3: Iterative multi-hop (bridge entities for low-confidence)
    ///
    /// All grep operations use in-memory cached texts — zero disk I/O during search.
    /// Only the final content fetch decompresses from the .said file.
    pub fn recall(&mut self, query: &str, top_k: usize) -> Vec<RecallResult> {
        self.ensure_corpus_cached();
        if self.corpus_ids.is_empty() {
            return self.grep(query, top_k);
        }

        // Clone cached corpus to avoid borrow conflicts with &mut self.
        // (The raw-text clone here was dead — bound to `_texts`, never read — so it is
        // gone; that was a full-corpus allocation per recall() call. #4)
        let ids = self.corpus_ids.clone();
        let texts_lower = self.corpus_texts_lower.clone();

        // Layer 1: SCA top-50 (< 1ms, fingerprints only)
        let sca_results = self.search_internal(query, 50);
        if sca_results.is_empty() {
            return self.grep(query, top_k);
        }
        let max_sca = sca_results.first().map(|(_, s)| *s).unwrap_or(1.0);

        // Layer 2: Grep re-rank on cached texts (zero disk I/O)
        let q_clean = query
            .replace("'s", "").replace("\u{2019}s", "")
            .trim_end_matches('?').trim().to_string();

        // Extract grep phrases (exact port from recall_fused)
        let mut phrases: Vec<String> = Vec::new();

        // Film/song/movie/book titles
        for prefix in &["film ", "song ", "movie ", "book "] {
            if let Some(pos) = q_clean.to_lowercase().find(prefix) {
                let title = q_clean[pos + prefix.len()..].trim().to_string();
                if title.len() > 2 { phrases.push(title); }
            }
        }

        // "of X" entity
        {
            let q_lower = q_clean.to_lowercase();
            let mut best_of_pos: Option<usize> = None;
            let mut search_from = 0;
            while let Some(rel_pos) = q_lower[search_from..].find(" of ") {
                let abs_pos = search_from + rel_pos;
                let of_in_original = &q_clean[abs_pos + 1..abs_pos + 3];
                if of_in_original == "of" {
                    best_of_pos = Some(abs_pos);
                }
                search_from = abs_pos + 4;
            }
            if let Some(pos) = best_of_pos {
                let after = q_clean[pos + 4..].trim();
                let stop = ["born","died","buried","father","mother","husband",
                            "wife","study","earned","from"];
                let entity: String = after.split_whitespace()
                    .take_while(|w| !stop.contains(&w.to_lowercase().as_str()))
                    .collect::<Vec<_>>().join(" ");
                if entity.len() > 2 { phrases.push(entity); }
            }
        }

        // Uppercase sequences (multi-word proper nouns)
        let words: Vec<&str> = q_clean.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            let first_char = words[i].chars().next().unwrap_or('a');
            if first_char.is_uppercase() {
                let start = i;
                while i < words.len() {
                    let ch = words[i].chars().next().unwrap_or('a');
                    if ch.is_uppercase() || (words[i].contains('-') && words[i].chars().any(|c| c.is_uppercase())) {
                        i += 1;
                    } else { break; }
                }
                if i - start >= 2 {
                    let phrase = words[start..i].join(" ");
                    let stop = ["father","mother","husband","wife","born","died","study","earned",
                                "paternal","maternal","grandfather","grandmother"];
                    let clean: String = phrase.split_whitespace()
                        .take_while(|w| !stop.contains(&w.to_lowercase().as_str()))
                        .collect::<Vec<_>>().join(" ");
                    let skip = ["What", "Where", "Who", "Which", "How", "Are"];
                    if clean.len() > 2 && !skip.contains(&clean.as_str()) {
                        phrases.push(clean);
                    }
                }
            } else { i += 1; }
        }

        // Comma-separated entities
        for phrase in phrases.clone() {
            if phrase.contains(',') {
                for part in phrase.split(',') {
                    let p = part.trim().to_string();
                    if p.len() > 2 && p.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        phrases.push(p);
                    }
                }
            }
        }

        if phrases.is_empty() {
            let raw = q_clean.clone();
            if raw.len() > 2 { phrases.push(raw); }
        }

        // Score docs by phrase match on CACHED texts (zero disk)
        let mut grep_scores: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        let mut specific_docs: Vec<String> = Vec::new();
        for phrase in &phrases {
            let pl = phrase.to_lowercase();
            if pl.len() < 2 { continue; }
            let mut phrase_matches: Vec<(String, usize)> = Vec::new();
            for (idx, did) in ids.iter().enumerate() {
                let count = texts_lower[idx].matches(&pl).count();
                if count > 0 {
                    phrase_matches.push((did.clone(), count));
                }
            }
            let n_matches = phrase_matches.len();
            if n_matches <= 10 {
                let specificity = if n_matches <= 5 { 1.0 } else { 5.0 / n_matches as f32 };
                for (did, count) in &phrase_matches {
                    *grep_scores.entry(did.clone()).or_insert(0.0) += *count as f32 * specificity;
                }
            }
            if n_matches > 0 && n_matches <= 5 {
                for (did, _) in &phrase_matches {
                    specific_docs.push(did.clone());
                }
            }
        }
        let max_grep = grep_scores.values().copied().fold(1.0f32, f32::max);

        // Morphological word expansion + AND injection (exact port from recall_fused)
        {
            let stop_words: std::collections::HashSet<&str> = [
                "the","a","an","is","are","was","were","be","been","being","have","has","had",
                "do","does","did","will","would","shall","should","can","could","may","might",
                "must","to","of","in","for","on","at","by","with","from","as","into","through",
                "during","before","after","above","below","between","under","not","no","nor",
                "but","or","and","so","yet","both","either","neither","each","every","all","any",
                "few","many","some","most","much","such","own","other","another","only","very",
                "also","back","just","about","out","up","over","down","off","still","again",
                "further","then","once","here","there","when","where","why","how","more","these",
                "those","his","her","he","she","they","their","it","its","this","that","what",
                "who","which","you","your","we","our","them"
            ].iter().copied().collect();

            let q_lower = query.to_lowercase();
            let mut variants: Vec<String> = Vec::new();
            for w in q_lower.split_whitespace() {
                let clean: String = w.chars().filter(|c| c.is_ascii_lowercase()).collect();
                if clean.len() < 4 || stop_words.contains(clean.as_str()) { continue; }
                variants.push(clean.clone());
                if clean.ends_with("ers") && clean.len() > 5 {
                    variants.push(clean[..clean.len()-3].to_string());
                    variants.push(clean[..clean.len()-1].to_string());
                } else if clean.ends_with("ing") && clean.len() > 5 {
                    variants.push(clean[..clean.len()-3].to_string());
                } else if clean.ends_with("ed") && clean.len() > 4 {
                    variants.push(clean[..clean.len()-2].to_string());
                } else if clean.ends_with("es") && clean.len() > 4 {
                    variants.push(clean[..clean.len()-2].to_string());
                } else if clean.ends_with("s") && clean.len() > 4 {
                    variants.push(clean[..clean.len()-1].to_string());
                }
                if w.contains('-') {
                    for part in w.split('-') {
                        let p: String = part.chars().filter(|c| c.is_ascii_lowercase()).collect();
                        if p.len() >= 4 && !stop_words.contains(p.as_str()) { variants.push(p); }
                    }
                }
            }
            variants.sort();
            variants.dedup();

            let mut rare: Vec<(String, usize)> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for v in &variants {
                if v.len() < 4 || seen.contains(v) { continue; }
                let count = texts_lower.iter().filter(|t| t.contains(v.as_str())).count();
                if count > 0 && count <= 10 {
                    seen.insert(v.clone());
                    rare.push((v.clone(), count));
                }
            }
            rare.sort_by_key(|x| x.1);

            if rare.len() >= 2 {
                let sca_set: std::collections::HashSet<String> = sca_results.iter()
                    .map(|(d, _)| d.clone()).collect();
                let mut injected: std::collections::HashSet<String> = std::collections::HashSet::new();
                for ii in 0..rare.len().min(10) {
                    for jj in (ii+1)..rare.len().min(10) {
                        let w1 = &rare[ii].0;
                        let w2 = &rare[jj].0;
                        let matches: Vec<&String> = ids.iter().enumerate()
                            .filter(|(idx, _)| texts_lower[*idx].contains(w1.as_str()) && texts_lower[*idx].contains(w2.as_str()))
                            .map(|(_, did)| did)
                            .collect();
                        if matches.len() == 1 && sca_set.contains(matches[0]) && !injected.contains(matches[0]) {
                            specific_docs.push(matches[0].clone());
                            injected.insert(matches[0].clone());
                        }
                    }
                }
            }
        }

        // Build candidates: SCA + specific injection
        let mut candidates: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        for (doc_id, sca_score) in &sca_results {
            candidates.insert(doc_id.clone(), *sca_score);
        }
        for did in &specific_docs {
            if !candidates.contains_key(did) {
                candidates.insert(did.clone(), max_sca * 0.8);
            } else if *candidates.get(did).unwrap_or(&0.0) < max_sca * 0.5 {
                candidates.insert(did.clone(), max_sca * 0.8);
            }
        }

        // Re-rank: only apply grep boost to SCA top-10 (safe, can't inject wrong docs)
        let has_ultra_specific = !specific_docs.is_empty();
        let sca_top10: std::collections::HashSet<String> = sca_results.iter()
            .take(10).map(|(d, _)| d.clone()).collect();
        let mut reranked: Vec<(String, f32)> = candidates.iter().map(|(did, sca)| {
            if has_ultra_specific && (sca_top10.contains(did) || specific_docs.contains(did)) {
                let g = (grep_scores.get(did).copied().unwrap_or(0.0) / max_grep) * max_sca * 1.0;
                (did.clone(), sca + g)
            } else {
                (did.clone(), *sca)
            }
        }).collect();
        reranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        reranked.truncate(top_k);

        // Build results — only decompress the winning frames
        let mut results = Vec::new();
        for (doc_id, score) in &reranked {
            if let Some(content) = self.read(doc_id) {
                results.push(RecallResult {
                    doc_id: doc_id.clone(),
                    score: *score,
                    content,
                });
            }
        }
        results
    }

    /// Add a document with full taxonomy (internal/advanced use).
    pub fn put(&mut self, doc_id: &str, content: &str, title: Option<&str>) -> u64 {
        let frame_id = self.frames.put(doc_id, content.as_bytes(), title);
        self.dirty = true;
        frame_id
    }

    /// Add with full taxonomy classification (internal/advanced).
    pub fn put_with(&mut self, opts: &crate::frames::PutOptions) -> u64 {
        let frame_id = self.frames.put_with(opts);
        self.dirty = true;
        frame_id
    }

    /// Insert raw binary content (image/font bytes that may contain non-UTF-8
    /// sequences). Unlike `put()` which takes `&str`, this accepts arbitrary
    /// `&[u8]`. Delegates to FrameStore::put_with_pillar_raw which is the
    /// underlying binary path. Pillar must be specified explicitly.
    ///
    /// Used by said-vault for image/font/xml asset frames where the bytes
    /// are not guaranteed UTF-8 (and where base64 wrapping would inflate
    /// storage by 33%).
    pub fn put_binary(
        &mut self,
        doc_id: &str,
        content: &[u8],
        pillar: crate::frames::Pillar,
        tags: Vec<String>,
    ) -> u64 {
        use crate::frames::{MemoryKind, MemoryScope, MemorySubject, MemoryType};
        let frame_id = self.frames.put_with_pillar_raw(
            doc_id, content, None,
            MemoryType::Episodic, MemoryKind::Fact, MemorySubject::User, MemoryScope::Personal,
            tags, pillar,
        );
        self.dirty = true;
        frame_id
    }

    /// Read a single document's raw bytes by doc_id. Unlike `read()` which
    /// decodes as UTF-8 (returning None for non-UTF-8 sequences), this returns
    /// the raw bytes — correct for binary assets (images, fonts, xml).
    pub fn read_binary(&mut self, doc_id: &str) -> Option<Vec<u8>> {
        self.frames.read_frame(doc_id, self.data.as_slice())
    }

    /// Compute the semantic delta between two 1-bit SCA fingerprints.
    ///
    /// Both slices are byte-packed 1-bit vectors (8 bits/byte). Returns the
    /// Hamming distance normalized to [0.0, 1.0]:
    ///
    /// - 0.00 = identical fingerprints (no semantic change)
    /// - 0.50 = half the bits flipped (major rewrite)
    /// - 1.00 = every bit flipped (opposite direction)
    ///
    /// Used by `replace_frame` to tag tombstones with "how different is the
    /// new version from the old one." One XOR + one POPCNT per byte — cheap.
    pub fn semantic_delta_bytes(a: &[u8], b: &[u8]) -> f32 {
        let len = a.len().min(b.len());
        if len == 0 { return 0.0; }
        let mut hamming = 0u32;
        for i in 0..len {
            hamming += (a[i] ^ b[i]).count_ones();
        }
        let total_bits = (len * 8) as f32;
        (hamming as f32) / total_bits
    }

    /// Look up a doc's 1-bit SCA fingerprint from the quantized matrix.
    /// Returns None if the doc isn't in the SCA index yet.
    ///
    /// Docs can span multiple passages, so matrix rows are keyed by
    /// `passage_offsets[doc_idx]`, not `doc_idx` directly. We concatenate
    /// Diagnostic passthrough: per-structure heap usage of the lexical index (#4 OOM).
    pub fn lexical_mem_report(&self) -> String {
        // Append the FrameStore footprint so the streaming-spill (#4) is
        // observable: `pending=` is the RAW corpus bytes still held in RAM,
        // `data_owned=` is bytes of the .said held Owned (mmap counts as 0).
        // With a spill budget set, `pending` should hover near the budget
        // instead of climbing to the whole corpus size.
        format!(
            "{}\n  frame_store_mem: pending={:.1}MB data_owned={:.1}MB",
            self.engine.core.lexical_mem_report(),
            self.frames.pending_bytes() as f64 / (1024.0 * 1024.0),
            self.data.owned_len() as f64 / (1024.0 * 1024.0),
        )
    }

    /// SaidFile-level resident memory dump (the holders NOT in CrystallineCore's lexical
    /// report): the corpus text caches, the trigram index, and the file data handle.
    /// Used to find the full #4 memory picture beyond the lexical index.
    /// FrameStore resident-memory report (#4 scale) — passthrough for the SAID_MEM_REPORT path.
    pub fn frame_store_mem_report(&self) -> String {
        self.frames.frame_store_mem_report()
    }

    pub fn saidfile_mem_report(&self) -> String {
        let mb = |b: usize| (b as f64) / 1_048_576.0;
        let ct: usize = self.corpus_texts.iter().map(|s| s.len()).sum();
        let ctl: usize = self.corpus_texts_lower.iter().map(|s| s.len()).sum();
        let cids: usize = self.corpus_ids.iter().map(|s| s.len() + 24).sum();
        let dtn: usize = self.engine.doc_texts_normalized.iter().map(|s| s.len()).sum();
        let trg = self.trigram_index.as_ref().map(|t| t.approx_bytes()).unwrap_or(0);
        let data = match &self.data { FileData::Owned(v) => v.len(), FileData::Mmap(_) => 0 };
        format!(
            "saidfile_mem: corpus_texts={:.0}MB corpus_texts_lower={:.0}MB corpus_ids={:.0}MB doc_texts_normalized={:.0}MB trigram={:.0}MB data_owned={:.0}MB",
            mb(ct), mb(ctl), mb(cids), mb(dtn), mb(trg), mb(data))
    }

    /// Total approximate heap bytes of the lexical `_fast` index (#4 OOM driver).
    pub fn lexical_mem_bytes(&self) -> usize {
        self.engine.core.lexical_mem_bytes()
    }

    /// Heap bytes of the WORD-keyed lexical structures only (excludes raw doc text).
    /// The metric word-interning targets. See CrystallineCore::lexical_word_index_bytes.
    pub fn lexical_word_index_bytes(&self) -> usize {
        self.engine.core.lexical_word_index_bytes()
    }

    /// every passage belonging to this doc so the Hamming distance reflects
    /// the doc as a whole, not just its first passage.
    fn get_fingerprint(&self, doc_id: &str) -> Option<Vec<u8>> {
        let doc_idx = self.engine.core.get_doc_index(doc_id)? as usize;
        let bpp = self.engine.core.bytes_per_passage;
        if bpp == 0 { return None; }
        let matrix = &self.engine.core.matrix_quantized;
        let passage_offset = self.engine.core.passage_offsets
            .get(doc_idx).copied().unwrap_or(doc_idx);
        let passage_count = self.engine.core.passage_counts
            .get(doc_idx).copied().unwrap_or(1).max(1);
        let start = passage_offset * bpp;
        let end = start + passage_count * bpp;
        if end > matrix.len() { return None; }
        Some(matrix[start..end].to_vec())
    }

    /// Replace the Active frame for `doc_id` with new content, automatically
    /// recording the semantic delta as part of the cognitive lineage.
    ///
    /// This is the canonical "update a frame" entry point. Flow:
    ///
    /// 1. Snapshot the old Active frame's SCA fingerprint (if any)
    /// 2. Call `frames.put()` — this auto-tombstones the old frame and
    ///    inserts the new one as Active
    /// 3. Rebuild the SCA index so the new frame has a fingerprint
    /// 4. Compute the semantic delta between old_fp and new_fp via
    ///    Hamming distance on the 1-bit fingerprints
    /// 5. Patch the new frame's `semantic_delta` field on FrameMeta
    ///
    /// If the computed delta is 0.0 (fingerprint-identical), this would
    /// still tombstone the old frame but caller can check the returned
    /// delta and decide to roll back. In practice, `cmd_reindex` uses
    /// BLAKE3 content hash before calling replace_frame, so 0.0 deltas
    /// only happen with true edge cases (whitespace-only semantic changes).
    ///
    /// Returns `(new_frame_id, semantic_delta)`.
    pub fn replace_frame(&mut self, doc_id: &str, content: &str, title: Option<&str>) -> (u64, f32) {
        // 1. Snapshot old fingerprint before the rebuild wipes it.
        let old_fp = self.get_fingerprint(doc_id);

        // 2. Put the new frame. frames.put_internal will auto-tombstone
        //    the previous Active frame for this doc_id.
        let new_frame_id = self.frames.put(doc_id, content.as_bytes(), title);
        self.dirty = true;

        // 3. Rebuild SCA index so the new frame gets a fingerprint.
        //    (Full rebuild over all Active frames.)
        let _ = self.build_index();

        // 4. Compute delta between old and new fingerprints (if we had an old one).
        let delta = if let Some(old_fp_bytes) = old_fp {
            if let Some(new_fp_bytes) = self.get_fingerprint(doc_id) {
                Self::semantic_delta_bytes(&old_fp_bytes, &new_fp_bytes)
            } else {
                0.0
            }
        } else {
            0.0 // genesis frame — no predecessor
        };

        // 5. Patch the new frame's semantic_delta field so it shows up in history.
        self.frames.set_semantic_delta(new_frame_id, delta);

        (new_frame_id, delta)
    }

    /// Walk the cognitive lineage for a doc_id.
    ///
    /// Returns frames in chronological order (oldest first, newest last).
    /// Includes both Active (current) and Tombstone (superseded) frames.
    /// Used by `said history` to display the timeline.
    pub fn lineage(&self, doc_id: &str) -> Vec<&crate::frames::FrameMeta> {
        self.frames.lineage(doc_id)
    }

    /// Return the unique set of `source:` tag values across all Active frames.
    ///
    /// Foundation for a future `said watch` daemon — every frame ingested
    /// via a file-based pipeline (init, add, reindex) carries a
    /// `source:<abs_path>` tag pointing to the user-supplied root the
    /// frame originally came from. `watch` would register on each of these
    /// roots and auto-reindex on change.
    pub fn sources(&self) -> Vec<String> {
        use std::collections::HashSet;
        let mut seen: HashSet<String> = HashSet::new();
        let ids = self.frames.active_doc_ids();
        for did in &ids {
            if let Some(meta) = self.frames.get_meta(did) {
                for tag in &meta.tags {
                    if let Some(src) = tag.strip_prefix("source:") {
                        seen.insert(src.to_string());
                    }
                }
            }
        }
        let mut out: Vec<String> = seen.into_iter().collect();
        out.sort();
        out
    }

    /// Read a single document's text by doc_id.
    pub fn read(&mut self, doc_id: &str) -> Option<String> {
        self.frames.read_frame_text(doc_id, self.data.as_slice())
    }

    /// Delete/forget a memory.
    pub fn forget(&mut self, doc_id: &str) -> bool {
        let deleted = self.frames.delete(doc_id);
        if deleted { self.dirty = true; }
        deleted
    }

    /// Alias for forget (backward compat).
    pub fn delete(&mut self, doc_id: &str) -> bool {
        self.forget(doc_id)
    }

    /// Build/rebuild SCA search index from all active frames.
    /// Call after adding NEW documents. Encodes all docs → 1-bit fingerprints.
    /// On open(), the saved SCRM index is already loaded — no need to call this.
    pub fn build_index(&mut self) -> Result<(), String> {
        self.build_index_with_progress(|_, _, _| {})
    }

    /// Build search index with progress callback: (docs_done, total_docs, total_passages).
    /// Uses flat-memory mmap streaming inside `index_batch_with_progress` —
    /// RAM stays constant regardless of corpus size.
    pub fn build_index_with_progress<F>(&mut self, progress: F) -> Result<(), String>
    where
        F: FnMut(usize, usize, usize),
    {
        // Collect only the active doc IDs (cheap — ids, not text). The texts are read from
        // the mmap'd frames in CHUNKS below so the full corpus text is NEVER resident at
        // once (at 35k Wonga frames the raw text is ~3.8 GB — collecting it all up front was
        // the index-stage OOM, #4). We still need ALL ids to detect the incremental case.
        let all_active: Vec<String> = self.frames.active_doc_ids().iter().map(|s| s.to_string()).collect();
        let mut doc_ids: Vec<String> = Vec::with_capacity(all_active.len());
        for doc_id in &all_active {
            // keep an id only if its frame has non-empty text (matches prior behavior)
            if let Some(text) = self.frames.read_frame_text(doc_id, self.data.as_slice()) {
                if !text.is_empty() {
                    doc_ids.push(doc_id.clone());
                }
            }
        }

        // INCREMENTAL vs FULL — one path decides. If the index is already populated
        // (corpus_ids non-empty) and the only change is NEW frames appended (existing
        // ids unchanged), and the growth is below the recompute threshold, append just
        // the new frames against the persisted corpus mean (O(new) not O(all)).
        let indexed: std::collections::HashSet<&str> =
            self.corpus_ids.iter().map(|s| s.as_str()).collect();
        let prior = self.corpus_ids.len();
        let existing_still_present = !indexed.is_empty()
            && doc_ids.iter().filter(|d| indexed.contains(d.as_str())).count() == prior;
        let new_ids: Vec<String> = doc_ids.iter().filter(|d| !indexed.contains(d.as_str())).cloned().collect();
        let growth_ok = prior > 0 && (new_ids.len() as f32) <= (prior as f32) * RECOMPUTE_GROWTH;
        let can_incremental = existing_still_present && growth_ok && !new_ids.is_empty();

        if !doc_ids.is_empty() {
            #[cfg(feature = "static-embed")]
            {
                if self.engine.encode_query("test").is_none() {
                    let _ = self.engine.try_auto_load_encoder();
                }

                if can_incremental {
                    // Append ONLY the new frames; existing fingerprints untouched.
                    let _ = &progress;
                    let new_texts: Vec<String> = new_ids.iter()
                        .map(|id| self.frames.read_frame_text(id, self.data.as_slice()).unwrap_or_default())
                        .collect();
                    self.engine.index_batch_incremental(&new_ids, &new_texts)?;
                    // corpus_texts_lower is rebuilt fully from frames after the if-block.
                } else {
                    self.engine.clear();
                    // CHUNKED full build: first chunk does the full mean-establishing build;
                    // later chunks append incrementally against that mean (the documented
                    // recompute-on-growth design, applied within one init). Each chunk's raw
                    // text is read, encoded, lower-cached, then DROPPED before the next — so
                    // the full ~3.8 GB corpus text is NEVER resident at once.
                    // #4 encode streaming: chunk by BYTES, not a fixed doc count, so the
                    // per-chunk transient (the chunk's `texts` Vec + the passages encoded from
                    // it) is bounded by a byte budget REGARDLESS of doc size — aligned with the
                    // ingest spill, which also flushes by bytes. A fixed 512-doc chunk holds
                    // ~200MB when those 512 docs are big SQL files (1560 passages each), but a
                    // few hundred small C# files. Byte-budgeting packs FEWER big docs / MORE
                    // small docs per chunk → flat transient either way. A doc larger than the
                    // budget still forms its own chunk (index_batch already streams a single
                    // doc's passages internally, so one big doc is bounded). SAID_TEXT_CHUNK
                    // (a byte budget) overrides; SAID_TEXT_CHUNK_DOCS caps docs/chunk for the
                    // encoder-batch-efficiency floor on tiny-doc corpora.
                    let chunk_byte_budget: usize = std::env::var("SAID_TEXT_CHUNK").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(64 * 1024 * 1024);
                    let max_chunk_docs: usize = std::env::var("SAID_TEXT_CHUNK_DOCS").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(4096);
                    let mut first = true;
                    let mut i = 0usize;
                    while i < doc_ids.len() {
                        // Pack the next chunk: read frame texts until we hit the byte budget
                        // (or the doc cap). At least one doc per chunk (big docs go solo).
                        let mut chunk_ids: Vec<String> = Vec::new();
                        let mut texts: Vec<String> = Vec::new();
                        let mut bytes = 0usize;
                        while i < doc_ids.len() && chunk_ids.len() < max_chunk_docs {
                            let id = &doc_ids[i];
                            let t = self.frames.read_frame_text(id, self.data.as_slice()).unwrap_or_default();
                            let tlen = t.len();
                            // Stop before adding a doc that would blow the budget — UNLESS the
                            // chunk is still empty (a single over-budget doc must go through).
                            if !chunk_ids.is_empty() && bytes + tlen > chunk_byte_budget {
                                break;
                            }
                            chunk_ids.push(id.clone());
                            texts.push(t);
                            bytes += tlen;
                            i += 1;
                        }
                        if first {
                            self.engine.index_batch_with_progress(&chunk_ids, &texts, |_, _, _| {})?;
                            first = false;
                        } else {
                            self.engine.index_batch_incremental(&chunk_ids, &texts)?;
                        }
                        let _ = bytes;
                        // texts dropped here before the next chunk is read. corpus_texts_lower
                        // is NOT built here (#4) — it's built lazily on first query from frames.
                    }
                }
            }
            #[cfg(not(feature = "static-embed"))]
            {
                let _ = (&progress, can_incremental, &new_ids);
                self.engine.clear();
                const TEXT_CHUNK: usize = 2000;
                for chunk_ids in doc_ids.chunks(TEXT_CHUNK) {
                    let texts: Vec<String> = chunk_ids.iter()
                        .map(|id| self.frames.read_frame_text(id, self.data.as_slice()).unwrap_or_default())
                        .collect();
                    for (id, text) in chunk_ids.iter().zip(texts.iter()) {
                        self.engine.stream_index(id, text, 512);
                    }
                }
            }
        }

        // #4 memory: do NOT build corpus_texts_lower here. It is a full lowercased copy of
        // the corpus (~159MB on a text-heavy repo) and is the dominant init-time resident
        // spike. It's only needed by the query-time grep re-rank, so we leave it EMPTY and
        // let ensure_corpus_cached() build it lazily on the FIRST query, reading raw text
        // straight from the mmap'd frames (text stays stored ONCE on disk). corpus_texts
        // (raw) is likewise kept as empty placeholders, read from frames on demand.
        self.corpus_texts = vec![String::new(); doc_ids.len()];
        self.corpus_texts_lower = Vec::new();
        self.corpus_ids = doc_ids;

        self.engine.release_original_texts();

        let _ = progress;
        Ok(())
    }

    /// Build the passage-level engine on demand. Called by search_internal
    /// the first time a long query (≥ 20 words) comes in. The result is
    /// cached on `self.passage_engine` and reused across queries in the same
    /// process. Not serialized to disk, so the first long query after open
    /// pays a one-time rebuild cost (~0.5-1s on typical corpora).
    fn ensure_passage_engine(&mut self) {
        if !self.passage_engine.is_empty() { return; }
        if self.corpus_ids.is_empty() { return; }
        // Give the passage engine its own copy of the static encoder —
        // index_batch requires one. We reuse the path the main engine was
        // loaded from, or fall back to the known encoder paths.
        #[cfg(feature = "static-embed")]
        {
            let _ = self.passage_engine.engine.try_auto_load_encoder();
            let _ = self.passage_engine.rebuild(&self.corpus_ids, &self.corpus_texts);
        }
    }

    /// Ensure search is ready. Lazy-loads on first query/grep.
    /// Reads frame texts into memory for the grep re-rank layer.
    /// SCA fingerprints already loaded from SCRM on open() — zero re-encoding.
    /// For 4,627 docs: ~1-2 seconds (just block decompression + text caching).
    fn ensure_corpus_cached(&mut self) {
        // Already fully cached (ids + the lowercase grep cache). corpus_texts_lower can be
        // empty even when corpus_ids is set: build_index leaves it empty (#4 — it is a full
        // lowercased copy of the corpus, ~159MB on a text-heavy repo, and NOT needed during
        // the streaming init). We build it lazily here on the first query that needs it,
        // reading raw text straight from the mmap'd frames — text stays stored ONCE on disk.
        if !self.corpus_ids.is_empty() && !self.corpus_texts_lower.is_empty() { return; }

        let all_ids: Vec<String> = self.frames.active_doc_ids()
            .iter().map(|s| s.to_string()).collect();
        let mut doc_ids = Vec::new();
        let mut doc_texts = Vec::new();

        for doc_id in &all_ids {
            if let Some(text) = self.frames.read_frame_text(doc_id, self.data.as_slice()) {
                if !text.is_empty() {
                    doc_ids.push(doc_id.to_string());
                    doc_texts.push(text);
                }
            }
        }

        // Also rebuild engine's normalized texts for entity matching in recall pipeline.
        // This is lightweight — no encoding, just text normalization.
        for text in &doc_texts {
            self.engine.doc_texts_normalized.push(
                text.chars()
                    .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
                    .collect::<String>()
                    .to_lowercase()
            );
        }

        self.corpus_texts_lower = doc_texts.iter().map(|t| t.to_lowercase()).collect();
        self.corpus_ids = doc_ids;
        self.corpus_texts = doc_texts;
    }

    /// Look up a symbol (function, struct, class, etc.) by name.
    ///
    /// - `name` is matched exactly first. If no exact match, falls back to
    ///   prefix match (capped at 20 names), then case-insensitive contains.
    /// - Returns (doc_id, kind, start_line, end_line) tuples sorted so the
    ///   exact matches come first.
    pub fn sym(&self, name: &str, max_results: usize) -> Vec<SymbolResult> {
        let idx = match self.symbol_index.as_ref() {
            Some(i) => i,
            None => return Vec::new(),
        };
        let mut out: Vec<SymbolResult> = Vec::new();

        // Exact match first
        for e in idx.lookup_exact(name) {
            if let Some(did) = self.trigram_doc_ids.get(e.doc_index as usize) {
                out.push(SymbolResult {
                    name: name.to_string(),
                    doc_id: did.clone(),
                    kind: e.kind.as_str().to_string(),
                    start_line: e.start_line,
                    end_line: e.end_line,
                });
            }
            if out.len() >= max_results { return out; }
        }

        // Then prefix (skip anything we already included from exact)
        if out.len() < max_results {
            for (sym_name, entries) in idx.lookup_prefix(name, 20) {
                if sym_name == name { continue; } // already added
                for e in entries {
                    if let Some(did) = self.trigram_doc_ids.get(e.doc_index as usize) {
                        out.push(SymbolResult {
                            name: sym_name.to_string(),
                            doc_id: did.clone(),
                            kind: e.kind.as_str().to_string(),
                            start_line: e.start_line,
                            end_line: e.end_line,
                        });
                    }
                    if out.len() >= max_results { return out; }
                }
            }
        }

        // Then case-insensitive contains (fuzzy)
        if out.is_empty() {
            for (sym_name, entries) in idx.lookup_contains(name, 20) {
                for e in entries {
                    if let Some(did) = self.trigram_doc_ids.get(e.doc_index as usize) {
                        out.push(SymbolResult {
                            name: sym_name.to_string(),
                            doc_id: did.clone(),
                            kind: e.kind.as_str().to_string(),
                            start_line: e.start_line,
                            end_line: e.end_line,
                        });
                    }
                    if out.len() >= max_results { return out; }
                }
            }
        }

        out
    }

    /// List all symbols whose name starts with `prefix`. Used for browsing.
    pub fn sym_list(&self, prefix: &str, max_results: usize) -> Vec<SymbolResult> {
        let idx = match self.symbol_index.as_ref() {
            Some(i) => i,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        for (sym_name, entries) in idx.lookup_prefix(prefix, max_results) {
            for e in entries {
                if let Some(did) = self.trigram_doc_ids.get(e.doc_index as usize) {
                    out.push(SymbolResult {
                        name: sym_name.to_string(),
                        doc_id: did.clone(),
                        kind: e.kind.as_str().to_string(),
                        start_line: e.start_line,
                        end_line: e.end_line,
                    });
                }
                if out.len() >= max_results { return out; }
            }
        }
        out
    }

    /// Number of unique symbol names (for stats).
    pub fn symbol_count(&self) -> usize {
        self.symbol_index.as_ref().map(|s| s.num_names()).unwrap_or(0)
    }

    /// Grep: exact text search across ALL stored frames.
    /// No filesystem — searches the .said file's own content.
    pub fn grep(&mut self, pattern: &str, max_results: usize) -> Vec<RecallResult> {
        let pattern_lower = pattern.to_lowercase();
        let mut results = Vec::new();

        // Fast path: if we have a trigram index AND the pattern is >= 3 chars,
        // use it to narrow the candidate set to only frames that could possibly
        // contain the pattern. This turns O(n) scan into O(k × log n) + tiny
        // candidate verify, giving ~1000-10000x speedup on large brains.
        if pattern_lower.len() >= 3 {
            if let Some(candidate_doc_ids) = self.trigram_candidates(&pattern_lower) {
                for doc_id in &candidate_doc_ids {
                    if let Some(text) = self.read(doc_id) {
                        let lower = text.to_lowercase();
                        if lower.contains(&pattern_lower) {
                            let count = lower.matches(&pattern_lower).count();
                            results.push(RecallResult {
                                doc_id: doc_id.to_string(),
                                score: count as f32,
                                content: text,
                            });
                        }
                    }
                }
                results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
                results.truncate(max_results);
                return results;
            }
        }

        // Fallback: full scan (patterns < 3 chars, or no trigram index built).
        let all_ids: Vec<String> = self.frames.active_doc_ids().iter().map(|s| s.to_string()).collect();
        for doc_id in &all_ids {
            if let Some(text) = self.read(doc_id) {
                if text.to_lowercase().contains(&pattern_lower) {
                    // Count occurrences for scoring
                    let count = text.to_lowercase().matches(&pattern_lower).count();
                    results.push(RecallResult {
                        doc_id: doc_id.to_string(),
                        score: count as f32,
                        content: text,
                    });
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);
        results
    }

    /// Run the query through the trigram index (if available) and return the
    /// list of candidate doc_ids that could possibly contain the pattern.
    ///
    /// Returns `None` if:
    ///   - No trigram index is built
    ///   - The query is too short for trigrams (< 3 chars)
    ///   - The trigram_doc_ids list is out of sync with current frames
    ///
    /// In any of those cases the caller should fall back to a full scan.
    fn trigram_candidates(&self, pattern_lower: &str) -> Option<Vec<String>> {
        let idx = self.trigram_index.as_ref()?;
        if self.trigram_doc_ids.is_empty() { return None; }

        let candidate_positions = idx.candidates(pattern_lower)?;
        // Translate frame positions back to doc_ids. Positions came from the
        // active_doc_ids list at build time; we stored that exact list.
        let mut out: Vec<String> = Vec::with_capacity(candidate_positions.len());
        for pos in candidate_positions {
            if let Some(did) = self.trigram_doc_ids.get(pos as usize) {
                out.push(did.clone());
            }
        }
        Some(out)
    }

    /// Internal: full retrieval pipeline with brain learning.
    ///
    /// Delegates to `recall::search_full` — THE ONE canonical function that
    /// all callers use (said ask, MTEB harness, smoke tests). If you need to
    /// change retrieval logic, change `recall::search_full` in `recall.rs`.
    ///
    /// This method handles:
    ///   1. Encode the query embedding
    ///   2. Ensure corpus caches are populated (lazy load from mmap)
    ///   3. Ensure passage engine is built (lazy on first long query)
    ///   4. Call `recall::search_full` (the ONE function)
    ///   5. Brain wrap: recall_weight + S_slow boost + query log + dream
    fn search_internal(&mut self, query: &str, top_k: usize) -> Vec<(String, f32)> {
        let q_emb = match self.engine.encode_query(query) {
            Some(emb) => emb,
            None => return Vec::new(),
        };

        // Brain: write query embedding into slow memory
        self.engine.brain.s_slow_write(&q_emb);

        // Ensure corpus caches are populated
        self.ensure_corpus_cached();

        // Ensure passage engine is built for long queries
        if query.split_whitespace().count() >= 20 {
            self.ensure_passage_engine();
        }

        // Clone corpus caches for the recall pipeline (needs &mut engine
        // while holding read refs to corpus data).
        let ids = self.corpus_ids.clone();
        let texts_lower = self.corpus_texts_lower.clone();

        // Tag-scope detection: if the query contains a scoping token like
        // "version 4", narrow the corpus to only frames tagged `version:4`
        // BEFORE semantic scoring. This is what resolves version collision
        // (Chamber 3), client scoping, jurisdiction filtering, etc.
        let scope_doc_ids: Option<std::collections::HashSet<String>> =
            if let Some((ns, val)) = crate::recall::detect_scope_tag(query) {
                let tag = format!("{}:{}", ns, val);
                let matching: std::collections::HashSet<String> = ids.iter()
                    .filter(|did| {
                        self.frames.get_meta(did)
                            .map(|m| m.tags.iter().any(|t| t == &tag))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect();
                // Debug: uncomment to trace scope filtering
                // eprintln!("[scope] tag='{}' → {} matching out of {}", tag, matching.len(), ids.len());
                if !matching.is_empty() {
                    Some(matching)
                } else {
                    None
                }
            } else {
                None
            };

        // THE ONE CALL — recall::search_full_scoped handles all routing:
        //   NIAH (passkey/needle) → engine.search_niah
        //   Short (< 20 words)   → recall_fused
        //   Long  (≥ 20 words)   → passage blend pipeline
        //   + optional tag-scope pre-filter
        let hits = crate::recall::search_full_scoped(
            &mut self.engine,
            if self.passage_engine.is_empty() { None } else { Some(&mut self.passage_engine) },
            query,
            None,
            top_k,
            &ids,
            // Borrow the raw-text cache directly (disjoint field borrow from
            // &mut self.engine) instead of cloning the whole corpus each query. recall_fused
            // reads it for bridge-entity extraction (recall.rs:696), so it must be the real
            // text — but it needs no copy. (#4: was self.corpus_texts.clone())
            &self.corpus_texts,
            &texts_lower,
            scope_doc_ids.as_ref(),
        );

        // Brain wrap: recall_weight × s_slow_boost
        let s_slow_score = self.engine.brain.s_slow_read(&q_emb);
        let results: Vec<(String, f32)> = hits
            .into_iter()
            .map(|(doc_id, score)| {
                let recall_w = self.engine.brain.get_recall_weight(&doc_id);
                let s_slow_boost = if s_slow_score > 0.1 {
                    1.0 + (s_slow_score * 0.01).min(0.5)
                } else {
                    1.0
                };
                (doc_id, score * recall_w * s_slow_boost)
            })
            .collect();

        if let Some((doc_id, score)) = results.first() {
            self.engine.brain.log_query(query, doc_id, *score);
        }
        self.engine.brain.accumulate_query_embedding(&q_emb);
        self.engine.brain.s_slow_write(&q_emb);

        results
    }

    /// Rank documents by PURE SCA fingerprint similarity to `query` — the
    /// semantic-shape axis, NOT literal token overlap.
    ///
    /// Unlike `recall`/`ask` (which fuse symbol + grep + semantic and are
    /// therefore dominated by shared words), this forces the engine's
    /// `PureSemantic` route: embed the query, quantize to 1-bit, and rank by
    /// Hamming distance over fingerprints. The returned score is
    /// `1.0 - normalized_hamming` in `[0.0, 1.0]` — 1.0 = identical shape.
    ///
    /// This is what `suggest-fix` needs: "have I solved a ticket of this SHAPE
    /// before?" must not reward two unrelated tickets that merely share a class
    /// name, nor punish the same fix phrased differently. Read-only: it does not
    /// log queries or mutate brain state.
    pub fn rank_by_fingerprint(&mut self, query: &str, top_k: usize) -> Vec<(String, f32)> {
        // Ensure the static encoder is loaded — on a freshly-opened brain it is
        // lazy, so encode_query returns None until try_auto_load_encoder runs
        // (same guard build_index uses).
        #[cfg(feature = "static-embed")]
        {
            if self.engine.encode_query("test").is_none() {
                let _ = self.engine.try_auto_load_encoder();
            }
        }
        let q_emb = match self.engine.encode_query(query) {
            Some(emb) => emb,
            None => return Vec::new(),
        };
        self.ensure_corpus_cached();
        if self.corpus_ids.is_empty() {
            return Vec::new();
        }
        // Force pure-semantic routing so the score is fingerprint distance only
        // (no lexical/grep fusion), then restore the prior route.
        self.engine.core.set_force_route("PureSemantic");
        let results = self.engine.core.search_unified_quantized(&q_emb, query, top_k);
        // Reset routing to default (auto). "" / unknown maps to None in
        // set_force_route, which is the auto router.
        self.engine.core.set_force_route("");
        results
    }

    /// Auto-dream: fire a consolidation cycle if accumulated query count has crossed
    /// the corpus-scaled threshold. The SINGLE trigger for dreaming on the recall path
    /// — callers (CLI/MCP) no longer trigger dream manually; recall does it in core so
    /// every surface evolves brain state identically. Returns whether a cycle ran.
    pub fn maybe_dream(&mut self) -> bool {
        let active = self.stats().active_frames;
        let threshold = crate::ask::dynamic_dream_threshold(active);
        if self.stats().brain_pending_dream_queries >= threshold {
            self.dream(threshold)
        } else {
            false
        }
    }

    /// Run brain dream cycle (cross-timescale learning).
    pub fn dream(&mut self, min_queries: u64) -> bool {
        let corpus_mean = self.engine.core.get_corpus_mean().to_vec();
        let corpus_std = self.engine.core.get_corpus_std().to_vec();

        match self.engine.brain.dream(&corpus_mean, &corpus_std, min_queries) {
            Some((new_mean, new_std)) => {
                self.engine.core.set_corpus_mean(new_mean);
                self.engine.core.set_corpus_std(new_std);
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    /// Compact: block-compress all frames with Zstd dictionary (Block 256 standard).
    /// H.265 GOP-inspired: 256 frames per block with trained dictionary gives
    /// ~400KB decompression units at 1500 MB/s — on-the-fly random access.
    /// Returns (blocks_created, bytes_saved).
    pub fn compact(&mut self) -> (usize, u64) {
        let result = self.frames.compact(self.data.as_slice());
        if result.0 > 0 { self.dirty = true; }
        // Rebuild trigram index after compact — we now know the final frame set.
        // This is where grep gets its 10,000x speedup from.
        self.rebuild_trigram_index();
        result
    }

    /// Rebuild the trigram inverted index from all currently active frames.
    /// Called from compact() and any other point where the frame set becomes
    /// stable. Posting lists use positional indices into the active doc_ids
    /// list at the time of the build; we store that list in `trigram_doc_ids`
    /// so query-time lookups can translate index -> doc_id.
    /// Rebuild trigram index and symbol table from the cached corpus texts
    /// (populated by build_index()) plus pending_symbols (populated by
    /// record_symbol() calls during cmd_init AST chunking).
    ///
    /// Called during init when frames haven't been flushed to `self.data` yet
    /// — we use the in-memory texts directly. If the cache is empty (e.g.
    /// after reopening an existing .said file), fall back to reading from
    /// mmap via frames.read_frame_text().
    pub fn rebuild_trigram_index(&mut self) {
        // Pick the doc_id list and text source
        // Use the cached corpus text ONLY if it actually holds text. With the chunked
        // build_index, corpus_texts is N empty-string PLACEHOLDERS (raw text read from frames
        // on demand) — treat that as "not cached" so we read real text from the frames here,
        // otherwise the trigram + symbol index would be built from empty text.
        let corpus_texts_has_text = self.corpus_texts.iter().any(|t| !t.is_empty());
        let (doc_ids, texts_from_cache): (Vec<String>, bool) = if !self.corpus_ids.is_empty()
            && corpus_texts_has_text
            && self.corpus_ids.len() == self.corpus_texts.len()
        {
            (self.corpus_ids.clone(), true)
        } else {
            let active: Vec<String> = self.frames.active_doc_ids()
                .iter().map(|s| s.to_string()).collect();
            (active, false)
        };

        if doc_ids.is_empty() {
            self.trigram_index = None;
            self.trigram_doc_ids.clear();
            self.symbol_index = None;
            self.pending_symbols.clear();
            return;
        }

        // Build position lookup once — maps doc_id -> index (for symbol
        // translation). This is an O(n) build + O(1) lookups instead of
        // O(n^2) per-symbol linear scan. For 27K frames + 30K symbols that's
        // ~500x faster.
        use std::collections::HashMap;
        let mut doc_id_to_pos: HashMap<&str, u32> = HashMap::with_capacity(doc_ids.len());
        for (pos, did) in doc_ids.iter().enumerate() {
            doc_id_to_pos.insert(did.as_str(), pos as u32);
        }

        // 1) Trigram index (content search)
        let mut tidx = crate::trigram_index::TrigramIndex::new();
        if texts_from_cache {
            for (pos, text) in self.corpus_texts.iter().enumerate() {
                if !text.is_empty() {
                    tidx.add_frame(pos as u32, text);
                }
            }
        } else {
            for (frame_pos, doc_id) in doc_ids.iter().enumerate() {
                if let Some(text) = self.frames.read_frame_text(doc_id, self.data.as_slice()) {
                    if !text.is_empty() {
                        tidx.add_frame(frame_pos as u32, &text);
                    }
                }
            }
        }
        tidx.finalize();

        // 2) Symbol index — consume pending_symbols, resolving doc_id -> pos
        let mut sidx = crate::symbol_index::SymbolIndex::new();
        let pending = std::mem::take(&mut self.pending_symbols);
        for (name, doc_id, kind, start_line, end_line) in pending {
            if let Some(&pos) = doc_id_to_pos.get(doc_id.as_str()) {
                sidx.add(&name, pos, kind, start_line, end_line);
            }
            // Symbols whose doc_id isn't in the current active set are
            // silently dropped (frame was deleted after the symbol was
            // recorded, e.g. blake3 dedup kicked in).
        }

        self.trigram_index = Some(tidx);
        self.trigram_doc_ids = doc_ids;
        // Only install the symbol index if we actually got entries — empty
        // index adds no value and wastes a few bytes at save time.
        if sidx.num_names() > 0 {
            self.symbol_index = Some(sidx);
        } else {
            self.symbol_index = None;
        }
        self.dirty = true;
    }

    /// Number of frames waiting to be compressed.
    pub fn uncompressed_count(&self) -> usize {
        self.frames.uncompressed_count()
    }

    /// Run brain consolidation (decay cold recall weights).
    pub fn consolidate(&mut self) -> usize {
        let changed = self.engine.brain.consolidate();
        if changed > 0 { self.dirty = true; }
        changed
    }

    /// Save to disk — WAL-safe (write to .tmp, atomic rename).
    pub fn save(&mut self) -> Result<(), String> {
        let mut buf: Vec<u8> = Vec::new();

        // Header v7_1 (56 bytes) — always written for new files.
        // Flag bit 0 = FLAG_EXTENDED_HEADER signals the presence of trgm_offset.
        // Old v7 readers that check only the first 48 bytes will still work IF
        // they don't need trigram search; the flag bit tells v7_1 readers to
        // read the additional 8 bytes.
        //
        // [0..4]   magic "SAID"
        // [4..6]   version u16 (7)
        // [6..8]   flags u16        (bit 0 = extended header present)
        // [8..12]  frame_count u32
        // [12..20] scrm_offset u64
        // [20..28] toc_offset u64
        // [28..36] dict_offset u64
        // [36..44] blkt_offset u64
        // [44..52] trgm_offset u64  ← NEW in v7_1
        // [52..56] reserved
        buf.extend_from_slice(SAID_MAGIC);
        buf.extend_from_slice(&SAID_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.header_flags().to_le_bytes());
        buf.extend_from_slice(&(self.frames.active_count() as u32).to_le_bytes());
        let offsets_pos = buf.len();
        // 7 × u64 = 56 bytes of placeholders, then 4 bytes reserved to reach 72.
        buf.extend_from_slice(&[0u8; 60]);
        while buf.len() < HEADER_SIZE_V7_1 { buf.push(0); }

        let mut dict_offset: u64 = 0;
        let mut blkt_offset: u64 = 0;

        if self.frames.has_blocks() {
            let (block_data, block_table) = self.frames.flush_block_pending(buf.len() as u64);
            buf.extend_from_slice(&block_data);

            // DICT section — offset stored in header
            if let Some(dict) = self.frames.get_dictionary() {
                dict_offset = buf.len() as u64;
                buf.extend_from_slice(b"DICT");
                buf.extend_from_slice(&(dict.len() as u32).to_le_bytes());
                buf.extend_from_slice(dict);
            }

            // BLKT section — offset stored in header, no scanning needed
            if !block_table.is_empty() {
                blkt_offset = buf.len() as u64;
                buf.extend_from_slice(&block_table);
            }
        } else {
            // Uncompacted path: frames stored as plain (before compact() is called).
            // Preserve Active AND Tombstone frames (tombstones are lineage history),
            // but drop Deleted frames (user-removed, reclaimable).
            let existing: Vec<(u64, usize, usize)> = self.frames.get_all_frames().iter()
                .filter(|f| f.status != crate::frames::FrameStatus::Deleted)
                .filter(|f| f.offset > 0 && (f.offset as usize + f.compressed_len as usize) <= self.data.as_slice().len())
                .map(|f| (f.id, f.offset as usize, f.compressed_len as usize))
                .collect();

            for (id, start, len) in existing {
                let new_offset = buf.len() as u64;
                buf.extend_from_slice(&self.data.as_slice()[start..start+len]);
                self.frames.update_offset(id, new_offset);
            }

            let pending_data = self.frames.flush_pending(buf.len() as u64);
            buf.extend_from_slice(&pending_data);
        }

        // SCRM section
        let scrm_offset = buf.len() as u64;
        buf.extend_from_slice(&self.engine.core.serialize_breadcrumbs());

        // BRAN section
        buf.extend_from_slice(&self.engine.brain.serialize());

        // VAULT_TOMBSTONES section (said-vault Track B) — written only if the
        // vault tombstone section was ever allocated AND contains entries.
        // Discovered on open by scanning forward from scrm_offset, same pattern
        // as CTXT. No header offset slot (header is full at v7_1).
        if let Some(vts) = self.vault_tombstones.as_ref() {
            if vts.count() > 0 {
                buf.extend_from_slice(&vts.to_bytes());
            }
        }

        // TRGM section — write only if a built trigram index exists.
        // Layout:
        //   b"TRGM"            (4 bytes magic)
        //   u32 n_doc_ids
        //   per doc_id: u16 len, utf8 bytes
        //   u32 uncompressed_len of zstd blob
        //   u32 compressed_len of zstd blob
        //   zstd-compressed raw trigram index bytes
        let mut trgm_offset: u64 = 0;
        if let Some(ref idx) = self.trigram_index {
            if !self.trigram_doc_ids.is_empty() && idx.num_trigrams() > 0 {
                trgm_offset = buf.len() as u64;
                buf.extend_from_slice(b"TRGM");
                buf.extend_from_slice(&(self.trigram_doc_ids.len() as u32).to_le_bytes());
                for did in &self.trigram_doc_ids {
                    let id_bytes = did.as_bytes();
                    let len = id_bytes.len().min(u16::MAX as usize) as u16;
                    buf.extend_from_slice(&len.to_le_bytes());
                    buf.extend_from_slice(&id_bytes[..len as usize]);
                }
                let raw = idx.serialize_raw();
                let uncompressed_len = raw.len() as u32;
                let compressed = zstd::bulk::compress(&raw, 15)
                    .map_err(|e| format!("zstd compress TRGM failed: {}", e))?;
                buf.extend_from_slice(&uncompressed_len.to_le_bytes());
                buf.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
                buf.extend_from_slice(&compressed);
            }
        }

        // SYMS section — v7_1 symbol table.
        // Layout: b"SYMS" | u32 uncompressed_len | u32 compressed_len | zstd(raw)
        // Uses the same trigram_doc_ids list for doc_index translation — no
        // duplicate doc_id table is stored here.
        let mut syms_offset: u64 = 0;
        if let Some(ref sidx) = self.symbol_index {
            if sidx.num_names() > 0 {
                syms_offset = buf.len() as u64;
                let raw = sidx.serialize_raw();
                let uncompressed_len = raw.len() as u32;
                let compressed = zstd::bulk::compress(&raw, 15)
                    .map_err(|e| format!("zstd compress SYMS failed: {}", e))?;
                buf.extend_from_slice(b"SYMS");
                buf.extend_from_slice(&uncompressed_len.to_le_bytes());
                buf.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
                buf.extend_from_slice(&compressed);
            }
        }

        // REFS section — v7_1 reference edges (not yet implemented).
        // Reserved for LSP-derived cross-file references, frozen at init.
        let refs_offset: u64 = 0;

        // CTXT section — temporarily disabled to debug block corruption
        if false && !self.corpus_ids.is_empty() {
            let mut ctxt_raw = Vec::new();
            ctxt_raw.extend_from_slice(&(self.corpus_ids.len() as u32).to_le_bytes());
            for (id, text_lower) in self.corpus_ids.iter().zip(self.corpus_texts_lower.iter()) {
                let id_bytes = id.as_bytes();
                ctxt_raw.extend_from_slice(&(id_bytes.len() as u16).to_le_bytes());
                ctxt_raw.extend_from_slice(id_bytes);
                let text_bytes = text_lower.as_bytes();
                ctxt_raw.extend_from_slice(&(text_bytes.len() as u32).to_le_bytes());
                ctxt_raw.extend_from_slice(text_bytes);
            }
            if let Ok(compressed) = zstd::bulk::compress(&ctxt_raw, 19) {
                buf.extend_from_slice(b"CTXT");
                buf.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
                buf.extend_from_slice(&(ctxt_raw.len() as u32).to_le_bytes());
                buf.extend_from_slice(&compressed);
            }
        }

        // FTOC section
        let toc_offset = buf.len() as u64;
        buf.extend_from_slice(&self.frames.serialize_toc());

        // Write offsets into header (deterministic — no scanning on open).
        // v7_1 header has 7 u64 offsets followed by 4 reserved bytes.
        buf[offsets_pos..offsets_pos+8].copy_from_slice(&scrm_offset.to_le_bytes());
        buf[offsets_pos+8..offsets_pos+16].copy_from_slice(&toc_offset.to_le_bytes());
        buf[offsets_pos+16..offsets_pos+24].copy_from_slice(&dict_offset.to_le_bytes());
        buf[offsets_pos+24..offsets_pos+32].copy_from_slice(&blkt_offset.to_le_bytes());
        buf[offsets_pos+32..offsets_pos+40].copy_from_slice(&trgm_offset.to_le_bytes());
        buf[offsets_pos+40..offsets_pos+48].copy_from_slice(&syms_offset.to_le_bytes());
        buf[offsets_pos+48..offsets_pos+56].copy_from_slice(&refs_offset.to_le_bytes());

        // CRC32
        let crc = crate::state::crc32_simple(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());

        // WAL-safe write
        let tmp_path = format!("{}.tmp", self.path.display());
        std::fs::write(&tmp_path, &buf)
            .map_err(|e| format!("Write failed: {}", e))?;
        std::fs::rename(&tmp_path, &self.path)
            .map_err(|e| format!("Rename failed: {}", e))?;

        // Re-attach the in-memory view by MMAP'ing the file we just wrote, instead of
        // keeping `buf` as a second full copy in RAM (FileData::Owned(buf)). For a large
        // brain that double-hold doubles peak memory and was a primary contributor to the
        // save-time OOM (#4). Mmap is OS-paged from disk — reads touch only the blocks
        // they need, so we drop `buf` and hold zero owned file bytes. Falls back to Owned
        // only if the mmap fails (tiny/edge cases), preserving correctness.
        let buf_len = buf.len();
        drop(buf);
        self.data = match std::fs::File::open(&self.path)
            .and_then(|f| unsafe { memmap2::Mmap::map(&f) })
        {
            Ok(mmap) => FileData::Mmap(mmap),
            Err(_) => {
                // re-read into Owned as a last resort (still correct, just not bounded)
                match std::fs::read(&self.path) {
                    Ok(v) => FileData::Owned(v),
                    Err(e) => return Err(format!("re-open after save failed: {} ({} bytes written)", e, buf_len)),
                }
            }
        };
        // Streaming-spill cleanup (#4): the scratch file's bytes have now been
        // copied into the real .said by the save above, and `self.data` mmaps
        // the new file — the orphaned `.spill` is no longer referenced. Remove
        // it and reset the spill offset so a subsequent ingest starts fresh.
        let spill_path = self.spill_scratch_path();
        if std::path::Path::new(&spill_path).exists() {
            let _ = std::fs::remove_file(&spill_path);
        }
        self.spill_offset = HEADER_SIZE_V7_1 as u64;
        self.dirty = false;
        Ok(())
    }

    /// Serialize the brain to a byte vector without writing to disk.
    ///
    /// For in-memory use (WASM download, embedded). Mirrors `save()` but
    /// returns the serialized bytes instead of performing a WAL-safe file
    /// write. The internal `data` buffer and `dirty` flag are NOT updated;
    /// the caller receives an independent, self-contained snapshot.
    pub fn serialize_to_bytes(&mut self) -> Result<Vec<u8>, String> {
        let mut buf: Vec<u8> = Vec::new();

        buf.extend_from_slice(SAID_MAGIC);
        buf.extend_from_slice(&SAID_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.header_flags().to_le_bytes());
        buf.extend_from_slice(&(self.frames.active_count() as u32).to_le_bytes());
        let offsets_pos = buf.len();
        buf.extend_from_slice(&[0u8; 60]);
        while buf.len() < HEADER_SIZE_V7_1 { buf.push(0); }

        let mut dict_offset: u64 = 0;
        let mut blkt_offset: u64 = 0;

        if self.frames.has_blocks() {
            let (block_data, block_table) = self.frames.flush_block_pending(buf.len() as u64);
            buf.extend_from_slice(&block_data);

            if let Some(dict) = self.frames.get_dictionary() {
                dict_offset = buf.len() as u64;
                buf.extend_from_slice(b"DICT");
                buf.extend_from_slice(&(dict.len() as u32).to_le_bytes());
                buf.extend_from_slice(dict);
            }

            if !block_table.is_empty() {
                blkt_offset = buf.len() as u64;
                buf.extend_from_slice(&block_table);
            }
        } else {
            let existing: Vec<(u64, usize, usize)> = self.frames.get_all_frames().iter()
                .filter(|f| f.status != crate::frames::FrameStatus::Deleted)
                .filter(|f| f.offset > 0 && (f.offset as usize + f.compressed_len as usize) <= self.data.as_slice().len())
                .map(|f| (f.id, f.offset as usize, f.compressed_len as usize))
                .collect();

            for (id, start, len) in existing {
                let new_offset = buf.len() as u64;
                buf.extend_from_slice(&self.data.as_slice()[start..start+len]);
                self.frames.update_offset(id, new_offset);
            }

            let pending_data = self.frames.flush_pending(buf.len() as u64);
            buf.extend_from_slice(&pending_data);
        }

        let scrm_offset = buf.len() as u64;
        buf.extend_from_slice(&self.engine.core.serialize_breadcrumbs());

        buf.extend_from_slice(&self.engine.brain.serialize());

        if let Some(vts) = self.vault_tombstones.as_ref() {
            if vts.count() > 0 {
                buf.extend_from_slice(&vts.to_bytes());
            }
        }

        let mut trgm_offset: u64 = 0;
        if let Some(ref idx) = self.trigram_index {
            if !self.trigram_doc_ids.is_empty() && idx.num_trigrams() > 0 {
                trgm_offset = buf.len() as u64;
                buf.extend_from_slice(b"TRGM");
                buf.extend_from_slice(&(self.trigram_doc_ids.len() as u32).to_le_bytes());
                for did in &self.trigram_doc_ids {
                    let id_bytes = did.as_bytes();
                    let len = id_bytes.len().min(u16::MAX as usize) as u16;
                    buf.extend_from_slice(&len.to_le_bytes());
                    buf.extend_from_slice(&id_bytes[..len as usize]);
                }
                let raw = idx.serialize_raw();
                let uncompressed_len = raw.len() as u32;
                let compressed = zstd::bulk::compress(&raw, 15)
                    .map_err(|e| format!("zstd compress TRGM failed: {}", e))?;
                buf.extend_from_slice(&uncompressed_len.to_le_bytes());
                buf.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
                buf.extend_from_slice(&compressed);
            }
        }

        let mut syms_offset: u64 = 0;
        if let Some(ref sidx) = self.symbol_index {
            if sidx.num_names() > 0 {
                syms_offset = buf.len() as u64;
                let raw = sidx.serialize_raw();
                let uncompressed_len = raw.len() as u32;
                let compressed = zstd::bulk::compress(&raw, 15)
                    .map_err(|e| format!("zstd compress SYMS failed: {}", e))?;
                buf.extend_from_slice(b"SYMS");
                buf.extend_from_slice(&uncompressed_len.to_le_bytes());
                buf.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
                buf.extend_from_slice(&compressed);
            }
        }

        let refs_offset: u64 = 0;

        let toc_offset = buf.len() as u64;
        buf.extend_from_slice(&self.frames.serialize_toc());

        buf[offsets_pos..offsets_pos+8].copy_from_slice(&scrm_offset.to_le_bytes());
        buf[offsets_pos+8..offsets_pos+16].copy_from_slice(&toc_offset.to_le_bytes());
        buf[offsets_pos+16..offsets_pos+24].copy_from_slice(&dict_offset.to_le_bytes());
        buf[offsets_pos+24..offsets_pos+32].copy_from_slice(&blkt_offset.to_le_bytes());
        buf[offsets_pos+32..offsets_pos+40].copy_from_slice(&trgm_offset.to_le_bytes());
        buf[offsets_pos+40..offsets_pos+48].copy_from_slice(&syms_offset.to_le_bytes());
        buf[offsets_pos+48..offsets_pos+56].copy_from_slice(&refs_offset.to_le_bytes());

        let crc = crate::state::crc32_simple(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());

        Ok(buf)
    }

    /// Construct a `SaidFile` from raw bytes (for in-memory use — WASM, embedded).
    ///
    /// Mirrors `open()` but takes an owned byte buffer instead of a file path.
    /// The `path` field is set to the sentinel `"/in-memory/"`. Calling `save()`
    /// on the result will fail (the path is not writable); use
    /// `serialize_to_bytes()` to get the bytes back out.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        use std::path::PathBuf;
        let path = PathBuf::from("/in-memory/");
        let data = FileData::Owned(bytes);
        Self::parse_data(path, data)
    }

    /// Persist ONLY the brain state (BRAN section) without rewriting frames,
    /// blocks, dictionaries, or search indexes.
    ///
    /// This is the safe way to call save() during a query session: every
    /// `said ask` invocation can call this without risking corruption of
    /// the block-compressed frame data. Only the BRAN section is replaced;
    /// every other section is byte-copied verbatim from the existing file.
    ///
    /// Implementation:
    ///   1. Locate the existing BRAN section by scanning for its magic
    ///      starting at scrm_offset (same as open() does).
    ///   2. Determine where BRAN ends by finding the next non-zero section
    ///      offset after BRAN start (TRGM, SYMS, REFS, or TOC).
    ///   3. Build a new buffer:
    ///      - bytes [0..bran_start)   verbatim from old file
    ///      - fresh Brain::serialize() bytes
    ///      - bytes [old_bran_end..crc_start) verbatim from old file
    ///   4. Patch header offsets: any offset >= old_bran_end gets shifted
    ///      by (new_bran_len - old_bran_len).
    ///   5. Recompute CRC32 over the new buffer.
    ///   6. WAL-safe write (tmp + atomic rename).
    ///
    /// Returns Ok(()) on success. Falls back to a full save() if the file
    /// has no header (newly created, never saved) or no BRAN section yet.
    /// Snapshot the brain tensor state to a byte buffer.
    /// Used by `said init --incremental` to preserve learning across re-inits.
    pub fn brain_snapshot(&self) -> Vec<u8> {
        self.engine.brain.serialize()
    }

    /// Restore the brain tensor state from a previously taken snapshot.
    /// Silently keeps current brain on decode failure.
    pub fn restore_brain(&mut self, bytes: &[u8]) {
        if let Ok(b) = crate::brain::Brain::deserialize(bytes) {
            self.engine.brain = b;
            self.dirty = true;
        }
    }

    /// Append a tag to the Active frame with the given doc_id.
    pub fn add_tag(&mut self, doc_id: &str, tag: &str) {
        self.frames.add_tag(doc_id, tag);
        self.dirty = true;
    }

    /// Bytes of the file held OWNED in process RAM (0 when the file is mmap'd from disk).
    /// Diagnostic for the save-memory invariant: after a large save we should be mmap'd,
    /// not holding a full Owned copy. See tests/test_save_memory.rs.
    pub fn in_memory_data_len(&self) -> usize {
        self.data.owned_len()
    }

    /// Total bytes of raw corpus TEXT held resident in RAM across every cache
    /// (corpus_texts + corpus_texts_lower + engine.doc_texts_original +
    /// engine.doc_texts_normalized). Diagnostic for the index-memory invariant (#4):
    /// at scale these caches duplicate the corpus several times and drive the encode/index
    /// OOM. A bounded value means we are NOT holding the whole corpus N× in RAM.
    pub fn resident_text_bytes(&self) -> usize {
        let sum = |v: &[String]| v.iter().map(|s| s.len()).sum::<usize>();
        sum(&self.corpus_texts)
            + sum(&self.corpus_texts_lower)
            + self.engine.resident_text_bytes()
    }

    /// Active frames that carry a `link:<concept>` wikilink edge for `concept`
    /// (lowercased). The recall-time half of the build-graph path (3.9): used by
    /// `ask` to traverse explicit concept links so a query reaches a linked note even
    /// when the bridge word isn't in its body. Returns doc_ids.
    pub fn frames_linking_concept(&self, concept: &str) -> Vec<String> {
        let want = format!("link:{}", concept.to_lowercase());
        // INCLUDING pending: freshly-added frames live in the pre-flush buffer until
        // save, and recall must see them (a memory you just added is queryable now).
        self.frames.get_all_frames_with_pending().iter()
            .filter(|m| m.status == crate::frames::FrameStatus::Active)
            .filter(|m| m.tags.iter().any(|t| t == &want))
            .map(|m| m.doc_id.clone())
            .collect()
    }

    /// CODE GRAPH — what a symbol CALLS. Given a symbol name, return the doc_ids of the
    /// frames it references via `call:<name>` edges (extracted from the AST at ingest).
    /// `symbol` is matched against frame doc_ids/names; returns the callee doc_ids that
    /// exist in the brain. Walks one hop: symbol → its call targets.
    pub fn code_calls(&self, symbol: &str) -> Vec<String> {
        let sym_lower = symbol.to_lowercase();
        let frames = self.frames.get_all_frames_with_pending();
        // find the frame(s) whose doc_id/name matches the symbol
        let callees: std::collections::HashSet<String> = frames.iter()
            .filter(|m| m.status == crate::frames::FrameStatus::Active)
            .filter(|m| m.doc_id.to_lowercase().contains(&sym_lower)
                || m.title.as_deref().map(|t| t.to_lowercase().contains(&sym_lower)).unwrap_or(false))
            .flat_map(|m| m.tags.iter()
                .filter_map(|t| t.strip_prefix("call:").map(|s| s.to_lowercase())))
            .collect();
        // resolve callee names to actual doc_ids present in the brain
        frames.iter()
            .filter(|m| m.status == crate::frames::FrameStatus::Active)
            .filter(|m| {
                let name = m.doc_id.rsplit("::").nth(1).unwrap_or(&m.doc_id).to_lowercase();
                callees.iter().any(|c| name == *c || m.doc_id.to_lowercase().contains(c.as_str()))
            })
            .map(|m| m.doc_id.clone())
            .collect()
    }

    /// CODE GRAPH — who CALLS a symbol (reverse edges). Returns doc_ids of frames that
    /// carry a `call:<symbol>` edge — the callers of `symbol`.
    pub fn code_callers(&self, symbol: &str) -> Vec<String> {
        let want = format!("call:{}", symbol);
        let want_lower = format!("call:{}", symbol.to_lowercase());
        self.frames.get_all_frames_with_pending().iter()
            .filter(|m| m.status == crate::frames::FrameStatus::Active)
            .filter(|m| m.tags.iter().any(|t| t == &want || t.to_lowercase() == want_lower))
            .map(|m| m.doc_id.clone())
            .collect()
    }

    /// Every distinct `[[wikilink]]` concept in the brain, with how many memories carry
    /// each — sorted by count desc, then name. The dedup/convergence surface: a curating
    /// LLM calls this before adding a memory so it REUSES existing concepts (link `heart`,
    /// not a new `heart-health`) and the concept graph self-converges instead of
    /// fragmenting. `prefix` (lowercased) filters to concepts starting with it.
    pub fn list_concepts(&self, prefix: Option<&str>) -> Vec<(String, usize)> {
        let pfx = prefix.map(|p| p.to_lowercase());
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for m in self.frames.get_all_frames_with_pending() {
            if m.status != crate::frames::FrameStatus::Active { continue; }
            for t in &m.tags {
                if let Some(concept) = t.strip_prefix("link:") {
                    if let Some(ref p) = pfx {
                        if !concept.starts_with(p.as_str()) { continue; }
                    }
                    *counts.entry(concept.to_string()).or_insert(0) += 1;
                }
            }
        }
        let mut out: Vec<(String, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// Tombstone an Active frame (no replacement) — for source files that
    /// disappear on re-init. Returns true if a frame was tombstoned.
    pub fn tombstone_frame(&mut self, doc_id: &str) -> bool {
        let changed = self.frames.tombstone(doc_id);
        if changed { self.dirty = true; }
        changed
    }

    /// Drop all cognitive-lineage tombstones — converts every Tombstone frame
    /// to `Deleted` so the next compact reclaims their bytes. Active frames
    /// (current HEADs) are preserved. Returns the number of tombstones dropped.
    pub fn drop_history(&mut self) -> usize {
        let n = self.frames.drop_tombstones();
        if n > 0 { self.dirty = true; }
        n
    }

    /// Drop older tombstones but keep the `keep_per_doc` most recent per
    /// doc_id. Current HEADs are untouched. Returns the count dropped.
    pub fn drop_history_keep(&mut self, keep_per_doc: usize) -> usize {
        let n = self.frames.drop_tombstones_keep(keep_per_doc);
        if n > 0 { self.dirty = true; }
        n
    }

    /// Count of Tombstone frames currently stored.
    pub fn tombstone_count(&self) -> usize {
        self.frames.tombstone_count()
    }

    /// Sum of uncompressed bytes across all Tombstone frames.
    pub fn tombstone_bytes(&self) -> u64 {
        self.frames.tombstone_bytes()
    }

    /// Restore a past version of a doc as the new HEAD (git-style checkout).
    ///
    /// Reads the tombstoned frame's payload, then calls `replace_frame` so
    /// the CURRENT head becomes a tombstone and the restored content becomes
    /// the new Active frame with a fresh frame_id. The lineage chain grows
    /// by one: each checkout is a real event in the timeline, not a rewind.
    ///
    /// Returns `(new_frame_id, semantic_delta)` on success.
    pub fn checkout_version(&mut self, frame_id: u64) -> Result<(u64, f32), String> {
        let file_data = self.data.as_slice().to_vec();
        let meta = self.frames.get_meta_by_id(frame_id)
            .ok_or_else(|| format!("No frame with id {}", frame_id))?
            .clone();
        if meta.status == crate::frames::FrameStatus::Deleted {
            return Err(format!("Frame {} is deleted, cannot checkout", frame_id));
        }
        let bytes = self.frames.read_frame_by_id(frame_id, &file_data)
            .ok_or_else(|| format!("Failed to read frame {}", frame_id))?;
        let content = String::from_utf8(bytes)
            .map_err(|_| "Frame payload is not valid UTF-8".to_string())?;
        let title = meta.title.clone();
        // Snapshot tags before the mutation so we can propagate them to the
        // new HEAD. Source/ingest/hash tags should survive a checkout so
        // `--write` can still locate the origin file later.
        let inherited_tags: Vec<String> = meta.tags.clone();
        let (new_id, delta) = self.replace_frame(&meta.doc_id, &content, title.as_deref());
        for tag in &inherited_tags {
            self.frames.add_tag(&meta.doc_id, tag);
        }
        Ok((new_id, delta))
    }

    pub fn save_brain_only(&mut self) -> Result<(), String> {
        let old_bytes = self.data.as_slice();

        // Sanity: file must be a real saved .said with at least a header + CRC
        if old_bytes.len() < HEADER_SIZE_V7 + 4 {
            return self.save(); // never saved before — full save
        }

        // Parse the existing header so we know where each section lives
        if &old_bytes[0..4] != SAID_MAGIC {
            return self.save(); // not a valid file — full save will rebuild
        }
        let flags = u16::from_le_bytes([old_bytes[6], old_bytes[7]]);
        let scrm_offset = u64::from_le_bytes(old_bytes[12..20].try_into().unwrap()) as usize;
        let toc_offset = u64::from_le_bytes(old_bytes[20..28].try_into().unwrap()) as usize;
        let _dict_offset = u64::from_le_bytes(old_bytes[28..36].try_into().unwrap()) as usize;
        let _blkt_offset = u64::from_le_bytes(old_bytes[36..44].try_into().unwrap()) as usize;
        let (trgm_offset, syms_offset, refs_offset) =
            if (flags & FLAG_EXTENDED_HEADER) != 0 && old_bytes.len() >= HEADER_SIZE_V7_1 {
                (
                    u64::from_le_bytes(old_bytes[44..52].try_into().unwrap()) as usize,
                    u64::from_le_bytes(old_bytes[52..60].try_into().unwrap()) as usize,
                    u64::from_le_bytes(old_bytes[60..68].try_into().unwrap()) as usize,
                )
            } else {
                (0, 0, 0)
            };

        // Locate BRAN inside the SCRM..TOC region (same scan open() uses)
        let bran_search_end = toc_offset.min(old_bytes.len() - 4); // before CRC
        if scrm_offset == 0 || scrm_offset >= bran_search_end {
            return self.save(); // malformed regions — fall back to full save
        }
        let bran_rel = match old_bytes[scrm_offset..bran_search_end]
            .windows(4)
            .position(|w| w == b"BRAN")
        {
            Some(p) => p,
            None => return self.save(), // no BRAN section yet — full save creates it
        };
        let bran_start = scrm_offset + bran_rel;

        // BRAN ends at the next non-zero section offset after bran_start.
        // Could be TRGM, SYMS, REFS, or TOC. We pick the smallest offset
        // that's strictly greater than bran_start.
        let mut bran_end = toc_offset; // toc_offset always exists
        for &candidate in &[trgm_offset, syms_offset, refs_offset] {
            if candidate > bran_start && candidate < bran_end {
                bran_end = candidate;
            }
        }
        if bran_end <= bran_start || bran_end > old_bytes.len() - 4 {
            return self.save(); // sanity broke — fall back
        }

        // Compose the new BRAN bytes
        let new_bran = self.engine.brain.serialize();
        let old_bran_len = bran_end - bran_start;
        let new_bran_len = new_bran.len();
        let delta_signed: i64 = new_bran_len as i64 - old_bran_len as i64;

        // Build new buffer
        let crc_start = old_bytes.len() - 4;
        let mut buf: Vec<u8> = Vec::with_capacity(old_bytes.len() + new_bran_len);
        buf.extend_from_slice(&old_bytes[0..bran_start]);
        buf.extend_from_slice(&new_bran);
        buf.extend_from_slice(&old_bytes[bran_end..crc_start]);

        // Patch header offsets that point INTO the post-BRAN region.
        // toc_offset always shifts. trgm/syms/refs shift if non-zero AND >= bran_end.
        let shift = |orig: usize| -> usize {
            if orig >= bran_end {
                ((orig as i64) + delta_signed) as usize
            } else {
                orig
            }
        };
        let new_toc = shift(toc_offset) as u64;
        let new_trgm = if trgm_offset > 0 { shift(trgm_offset) as u64 } else { 0 };
        let new_syms = if syms_offset > 0 { shift(syms_offset) as u64 } else { 0 };
        let new_refs = if refs_offset > 0 { shift(refs_offset) as u64 } else { 0 };

        // Header offsets are at fixed positions [12..68].
        // scrm_offset (12..20) and dict/blkt (28..44) all live BEFORE BRAN — unchanged.
        buf[20..28].copy_from_slice(&new_toc.to_le_bytes());
        if (flags & FLAG_EXTENDED_HEADER) != 0 {
            buf[44..52].copy_from_slice(&new_trgm.to_le_bytes());
            buf[52..60].copy_from_slice(&new_syms.to_le_bytes());
            buf[60..68].copy_from_slice(&new_refs.to_le_bytes());
        }

        // Recompute CRC32 over the new buffer (everything except the CRC tail)
        let crc = crate::state::crc32_simple(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());

        // WAL-safe write
        let tmp_path = format!("{}.tmp", self.path.display());
        std::fs::write(&tmp_path, &buf)
            .map_err(|e| format!("Write failed: {}", e))?;
        std::fs::rename(&tmp_path, &self.path)
            .map_err(|e| format!("Rename failed: {}", e))?;

        // Replace in-memory data with the new buffer (drops old mmap)
        self.data = FileData::Owned(buf);
        self.dirty = false;
        Ok(())
    }

    /// Load the static encoder (required for semantic search).
    #[cfg(feature = "static-embed")]
    pub fn load_encoder(&mut self, path: &str) -> Result<(), String> {
        self.engine.load_static_encoder(path)
    }

    /// Auto-load the encoder: embedded compile-time model first, then well-known
    /// file paths. Returns true if an encoder is now available. Use this when no
    /// explicit path is known (tests, embedded deployments).
    pub fn auto_load_encoder(&mut self) -> bool {
        self.engine.try_auto_load_encoder()
    }

    /// Stub when static-embed feature is not enabled.
    #[cfg(not(feature = "static-embed"))]
    pub fn load_encoder(&mut self, _path: &str) -> Result<(), String> {
        Err("static-embed feature not enabled".to_string())
    }

    /// Load the static encoder from raw bytes (for in-memory / WASM use).
    ///
    /// Writes the three model files to a temporary directory and then calls
    /// `load_static_encoder()` on the directory path — identical to the
    /// `from_embedded()` pattern in `StaticEncoder`. The temp directory is
    /// cleaned up by the OS; it persists only for the lifetime of the process.
    #[cfg(feature = "static-embed")]
    pub fn load_encoder_from_bytes(
        &mut self,
        tokenizer: &[u8],
        safetensors: &[u8],
        config: &[u8],
    ) -> Result<(), String> {
        // Load straight from memory — no filesystem. The previous temp-file
        // round-trip panicked in wasm ("no filesystem on this platform").
        // model2vec's StaticModel::from_bytes loads the same model from bytes.
        self.engine.load_static_encoder_from_bytes(tokenizer, safetensors, config)
    }

    /// Stub when static-embed feature is not enabled.
    #[cfg(not(feature = "static-embed"))]
    pub fn load_encoder_from_bytes(
        &mut self,
        _tokenizer: &[u8],
        _safetensors: &[u8],
        _config: &[u8],
    ) -> Result<(), String> {
        Err("static-embed feature not enabled — load_encoder_from_bytes is a no-op".to_string())
    }

    /// Enable LSP — connect to a language server for cross-file code intelligence.
    /// Common servers: "rust-analyzer", "typescript-language-server", "pyright"
    /// Results get cached as searchable frames in the .said file.
    #[cfg(feature = "lsp")]
    pub fn enable_lsp(&mut self, server_cmd: &str, workspace: &str) -> Result<(), String> {
        let client = crate::lsp_client::LspClient::connect(server_cmd, workspace)?;
        self.lsp = Some(client);
        Ok(())
    }

    /// Disable LSP — disconnect from language server.
    #[cfg(feature = "lsp")]
    pub fn disable_lsp(&mut self) {
        self.lsp = None;
    }

    /// Is LSP connected?
    #[cfg(feature = "lsp")]
    pub fn has_lsp(&self) -> bool {
        self.lsp.is_some()
    }

    /// LSP go-to-definition — finds where a symbol is defined.
    /// Result is cached as a frame for instant future recall.
    #[cfg(feature = "lsp")]
    pub fn lsp_definition(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let lsp = self.lsp.as_mut().ok_or("LSP not enabled. Call enable_lsp() first.")?;
        let result = lsp.go_to_definition(file_path, line, character)?;
        if !result.is_empty() {
            crate::lsp_client::cache_lsp_result(self, "definition", file_path, line, &result);
            self.dirty = true;
        }
        Ok(result)
    }

    /// LSP find-references — finds all usages of a symbol.
    #[cfg(feature = "lsp")]
    pub fn lsp_references(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let lsp = self.lsp.as_mut().ok_or("LSP not enabled")?;
        let result = lsp.find_references(file_path, line, character)?;
        if !result.is_empty() {
            crate::lsp_client::cache_lsp_result(self, "references", file_path, line, &result);
            self.dirty = true;
        }
        Ok(result)
    }

    /// LSP hover — gets type info and documentation.
    #[cfg(feature = "lsp")]
    pub fn lsp_hover(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let lsp = self.lsp.as_mut().ok_or("LSP not enabled")?;
        lsp.hover(file_path, line, character)
    }

    /// LSP workspace symbol — search symbols across entire project.
    #[cfg(feature = "lsp")]
    pub fn lsp_workspace_symbol(&mut self, query: &str) -> Result<String, String> {
        let lsp = self.lsp.as_mut().ok_or("LSP not enabled")?;
        let result = lsp.workspace_symbol(query)?;
        if !result.is_empty() {
            crate::lsp_client::cache_lsp_result(self, "workspace_symbol", query, 0, &result);
            self.dirty = true;
        }
        Ok(result)
    }

    /// Is there unsaved data?
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Get file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get raw file data slice (for diagnostics).
    pub fn data_slice(&self) -> &[u8] {
        self.data.as_slice()
    }

    /// Get stats — includes frame counts, index presence, and brain state.
    pub fn stats(&self) -> SaidFileStats {
        let frame_stats = self.frames.stats();
        let brain_stats = self.engine.brain.stats();

        SaidFileStats {
            file_size: self.data.len(),
            active_frames: frame_stats.active_frames,
            deleted_frames: frame_stats.deleted_frames,
            compressed_bytes: frame_stats.total_compressed_bytes,
            uncompressed_bytes: frame_stats.total_uncompressed_bytes,
            compression_ratio: frame_stats.compression_ratio,
            index_docs: self.engine.core.get_doc_ids().len(),
            symbol_count: self.symbol_index.as_ref().map(|s| s.num_names()).unwrap_or(0),
            trigram_present: self.trigram_index.is_some(),
            brain_queries: brain_stats.query_log_size,
            brain_boosted: brain_stats.boosted_docs,
            brain_cycles: brain_stats.consolidation_cycles,
            brain_tracked_docs: brain_stats.tracked_docs,
            brain_total_recalls: brain_stats.total_recalls,
            brain_max_recall_weight: brain_stats.max_recall_weight,
            brain_s_slow_magnitude: brain_stats.s_slow_magnitude,
            brain_pending_dream_queries: self.engine.brain.query_emb_count,
        }
    }

    // =========================================================================
    // Vector database API (ChromaDB/Pinecone drop-in replacement)
    // add() / query() / get() / delete() / commit()
    // =========================================================================

    /// Add a document (ChromaDB/Pinecone compatible).
    /// Stores text + rebuilds SCA search index.
    pub fn add(&mut self, doc_id: &str, text: &str) {
        self.remember_as(doc_id, text, None);
        let _ = self.build_index();
    }

    /// Add a document with title.
    pub fn add_with_title(&mut self, doc_id: &str, text: &str, title: &str) {
        self.remember_as(doc_id, text, Some(title));
        let _ = self.build_index();
    }

    /// Query: find top-k semantically similar documents.
    /// Pure SCA fingerprint search — SCRM loaded on open(), zero text loading.
    /// Only decompresses the winning frames for content retrieval.
    pub fn query(&mut self, query_text: &str, top_k: usize) -> Vec<RecallResult> {
        // Pure SCA search: encode query → Hamming distance → top-K doc_ids.
        //
        // Over-fetch 3× so we can dedupe duplicate-content frames without
        // starving the result list. Codebases commonly contain the same file
        // copied to multiple paths (LAM/LAM/said-lam-rust/src/backup/model.rs,
        // LAM/LAM/said-lam/src/model.rs, etc.) — without dedup these dominate
        // top-10 and crowd out the actual answer.
        let overfetch = (top_k * 3).max(20);
        let sca_results = self.search_internal(query_text, overfetch);

        // Content-hash dedup: drop any frame whose first 256 chars match a
        // higher-ranked frame's first 256 chars. Keep highest-scoring instance.
        use std::collections::HashSet;
        let mut seen_prefixes: HashSet<u64> = HashSet::new();
        let mut results = Vec::with_capacity(top_k);

        for (doc_id, score) in &sca_results {
            if results.len() >= top_k { break; }
            if let Some(content) = self.read(doc_id) {
                // Hash first 256 chars (char-safe) for a cheap content signature.
                let prefix: String = content.chars().take(256).collect();
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                use std::hash::{Hash, Hasher};
                prefix.hash(&mut hasher);
                let sig = hasher.finish();

                if seen_prefixes.insert(sig) {
                    results.push(RecallResult {
                        doc_id: doc_id.clone(),
                        score: *score,
                        content,
                    });
                }
            }
        }
        results
    }

    /// Get a document by ID.
    pub fn get(&mut self, doc_id: &str) -> Option<String> {
        self.read(doc_id)
    }

    /// Commit all changes to disk (WAL-safe).
    pub fn commit(&mut self) -> Result<(), String> {
        self.save()
    }
}

/// .said file statistics — includes brain state for "brain is alive" observability.
#[derive(Debug)]
pub struct SaidFileStats {
    pub file_size: usize,
    pub active_frames: usize,
    pub deleted_frames: usize,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub compression_ratio: f64,
    pub index_docs: usize,
    /// Number of symbols in the SYMS section (0 if none built)
    pub symbol_count: usize,
    /// Whether the TRGM trigram index is present
    pub trigram_present: bool,
    // ── Brain state ─────────────────────────────────────────────────────
    /// Total queries logged (ring buffer of recent searches)
    pub brain_queries: usize,
    /// Docs with recall_weight > 1.01 (frequently-accessed, getting boosted)
    pub brain_boosted: usize,
    /// Number of completed dream() cycles
    pub brain_cycles: u32,
    /// Total number of frames the brain is actively tracking
    pub brain_tracked_docs: usize,
    /// Total individual recall events (sum of recall_counts across all docs)
    pub brain_total_recalls: u32,
    /// Highest recall_weight on any single frame (what's the most-loved doc)
    pub brain_max_recall_weight: f32,
    /// s_slow tensor magnitude (cross-document synthesis signal strength)
    pub brain_s_slow_magnitude: f32,
    /// Accumulated query embeddings waiting for next dream() cycle
    pub brain_pending_dream_queries: u64,
}

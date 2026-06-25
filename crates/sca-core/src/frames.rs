//! Per-frame document storage for .said files.
//!
//! Each document is stored as an independent frame with:
//! - zstd compression (level configurable, default 3 for speed)
//! - BLAKE3 checksum (32 bytes, cryptographic integrity)
//! - Optional AES-256-GCM encryption (per-frame, before compression)
//!
//! Frames are append-only. Deletes mark a tombstone flag.
//! Random access: seek to frame offset, read compressed bytes, decompress.
//!
//! This is NOT memvid code. Clean-room implementation for .said files.

use std::collections::HashMap;

// =========================================================================
// MEMORY TAXONOMY — backed by neuroscience + 2025-2026 arxiv surveys
// =========================================================================

/// What cognitive function this memory serves (Tulving 1972, extended).
/// Each type has a different decay rate in the brain layer.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum MemoryType {
    /// Events, conversations, experiences with temporal context. Decays fast (0.60).
    Episodic = 0,
    /// Facts, knowledge, world state. Moderate decay (0.85).
    Factual = 1,
    /// Skills, how-to, behavioral rules. Slow decay (0.90).
    Procedural = 2,
    /// Connections between entities. Moderate decay (0.80).
    Relational = 3,
    /// Self-reflections, lessons learned, metacognition. Moderate decay (0.80).
    Meta = 4,
}

impl MemoryType {
    pub fn from_byte(b: u8) -> Self {
        match b { 1 => Self::Factual, 2 => Self::Procedural, 3 => Self::Relational, 4 => Self::Meta, _ => Self::Episodic }
    }
    /// Type-based decay rate per consolidation cycle (from dual-memory formula).
    pub fn decay_rate(&self) -> f32 {
        match self { Self::Episodic => 0.60, Self::Factual => 0.85, Self::Procedural => 0.90, Self::Relational => 0.80, Self::Meta => 0.80 }
    }
}

/// What shape/kind of content this memory contains.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum MemoryKind {
    Fact = 0,
    Preference = 1,
    Event = 2,
    Profile = 3,
    Relationship = 4,
    Goal = 5,
    Other = 6,
}

impl MemoryKind {
    pub fn from_byte(b: u8) -> Self {
        match b { 1 => Self::Preference, 2 => Self::Event, 3 => Self::Profile, 4 => Self::Relationship, 5 => Self::Goal, 6 => Self::Other, _ => Self::Fact }
    }
}

/// Whose memory this is (MemGPT/Letta: human/persona blocks).
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum MemorySubject {
    /// About the agent itself (persona, capabilities, self-knowledge).
    Agent = 0,
    /// About the user (preferences, facts, history).
    User = 1,
    /// Domain knowledge, external facts, world state.
    World = 2,
    /// Multi-party context, shared knowledge.
    Shared = 3,
}

impl MemorySubject {
    pub fn from_byte(b: u8) -> Self {
        match b { 1 => Self::User, 2 => Self::World, 3 => Self::Shared, _ => Self::Agent }
    }
}

/// Where this memory is visible (privacy/access scope).
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum MemoryScope {
    /// Private to one user. Encrypted by default when encryption enabled.
    Personal = 0,
    /// Scoped to a project/workspace.
    Project = 1,
    /// Shared across an organization.
    Organization = 2,
    /// World knowledge, not sensitive.
    Public = 3,
}

impl MemoryScope {
    pub fn from_byte(b: u8) -> Self {
        match b { 1 => Self::Project, 2 => Self::Organization, 3 => Self::Public, _ => Self::Personal }
    }
}

/// Which CLS pillar this frame belongs to.
///
/// The four-pillar memory model (Episodic/Semantic/Procedural/External) plus
/// the two pragmatic code-intelligence pillars (Code/Memory) that already
/// exist in production frames. Set at put-time; derived from `memory_type`
/// for frames written before the pillar field existed (see `from_memory_type`).
///
/// Wire format: one u8 at the tail of each TOC entry (after semantic_delta).
/// Files written before this field default to the derived value at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Pillar {
    /// Raw turn-by-turn experience, append-only. Decays fast, weighted by
    /// recency at retrieval. Drives the conversation-memory track (LoCoMo).
    Episodic = 0,
    /// Distilled facts, dream-written only. Confidence-gated retrieval.
    Semantic = 1,
    /// Action sequences with outcomes. Task-match retrieval (Voyager-style).
    Procedural = 2,
    /// Pointers to docs/URLs/APIs (Enterprise mode) or embedded blob refs
    /// (Portable mode). Searched via summary fingerprint, never raw content.
    External = 3,
    /// AST-chunked source code frames from `said init`. Existing behavior.
    Code = 4,
    /// Output of the `remember` tool before the four-pillar split — legacy.
    /// New writes should choose Semantic or Episodic; Memory exists only so
    /// pre-existing frames load without losing their classification.
    Memory = 5,
    /// Vault asset / manifest frames per Track B spec (said-vault).
    /// Personal brain .said files never use this variant; vault .said files
    /// use it exclusively. Enables `said ask` filtering at vault open time.
    Document = 6,
}

impl Pillar {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Semantic,
            2 => Self::Procedural,
            3 => Self::External,
            4 => Self::Code,
            5 => Self::Memory,
            6 => Self::Document,
            _ => Self::Episodic,
        }
    }

    /// Default pillar derived from a frame's MemoryType. Used to backfill
    /// frames loaded from files written before the pillar field existed.
    ///
    /// Mapping rationale:
    /// - `MemoryType::Episodic` → `Pillar::Episodic` (direct).
    /// - `MemoryType::Factual` → `Pillar::Semantic` (distilled facts).
    /// - `MemoryType::Procedural` → `Pillar::Procedural` (direct).
    /// - `MemoryType::Relational` / `Meta` → `Pillar::Semantic` (both are
    ///   distilled, not raw — relational edges and meta-reflections belong
    ///   with facts, not with raw turns).
    pub fn from_memory_type(t: MemoryType) -> Self {
        match t {
            MemoryType::Episodic => Self::Episodic,
            MemoryType::Factual => Self::Semantic,
            MemoryType::Procedural => Self::Procedural,
            MemoryType::Relational => Self::Semantic,
            MemoryType::Meta => Self::Semantic,
        }
    }

    /// Inverse of `from_memory_type`: the MemoryType a frame should carry when stored under
    /// this pillar (so decay/lifecycle behave correctly + the pillar round-trips through the
    /// legacy memory_type field). Consistent with the documented forward map
    /// (docs/said-structure/04-four-pillars/memory.md):
    ///   Episodic→Episodic, Semantic→Factual (decay 0.85), Procedural→Procedural,
    ///   Memory→Relational (the doc's Relational/Meta→Memory safety net inverts to Relational).
    /// External/Code/Document have no dedicated legacy MemoryType → Factual (distilled
    /// knowledge, decay like facts). Used by `remember_with_pillar`.
    pub fn to_memory_type(self) -> MemoryType {
        match self {
            Self::Episodic => MemoryType::Episodic,
            Self::Semantic => MemoryType::Factual,
            Self::Procedural => MemoryType::Procedural,
            Self::Memory => MemoryType::Relational,
            Self::External => MemoryType::Factual,
            Self::Code => MemoryType::Factual,
            Self::Document => MemoryType::Factual,
        }
    }

    /// Lowercase pillar name (for the auto-added `pillar:<name>` tag).
    pub fn name(self) -> &'static str {
        match self {
            Self::Episodic => "episodic",
            Self::Semantic => "semantic",
            Self::Procedural => "procedural",
            Self::External => "external",
            Self::Code => "code",
            Self::Memory => "memory",
            Self::Document => "document",
        }
    }

    /// Short prefix for auto-generated doc_ids under this pillar (e.g. `ep_`, `sem_`).
    pub fn doc_id_prefix(self) -> &'static str {
        match self {
            Self::Episodic => "ep_",
            Self::Semantic => "sem_",
            Self::Procedural => "proc_",
            Self::External => "ext_",
            Self::Code => "code_",
            Self::Memory => "mem_",
            Self::Document => "doc_",
        }
    }
}

/// Options for putting a memory into the .said file.
pub struct PutOptions<'a> {
    pub doc_id: &'a str,
    pub content: &'a str,
    pub title: Option<&'a str>,
    pub memory_type: MemoryType,
    pub memory_kind: MemoryKind,
    pub subject: MemorySubject,
    pub scope: MemoryScope,
    pub tags: Vec<String>,
}

impl<'a> PutOptions<'a> {
    /// Quick constructor with defaults (Episodic, Fact, User, Personal).
    pub fn new(doc_id: &'a str, content: &'a str) -> Self {
        Self {
            doc_id, content, title: None,
            memory_type: MemoryType::Episodic,
            memory_kind: MemoryKind::Fact,
            subject: MemorySubject::User,
            scope: MemoryScope::Personal,
            tags: Vec::new(),
        }
    }

    pub fn with_title(mut self, title: &'a str) -> Self { self.title = Some(title); self }
    pub fn with_type(mut self, t: MemoryType) -> Self { self.memory_type = t; self }
    pub fn with_kind(mut self, k: MemoryKind) -> Self { self.memory_kind = k; self }
    pub fn with_subject(mut self, s: MemorySubject) -> Self { self.subject = s; self }
    pub fn with_scope(mut self, s: MemoryScope) -> Self { self.scope = s; self }
    pub fn with_tags(mut self, tags: Vec<String>) -> Self { self.tags = tags; self }
}

// =========================================================================
// FRAME ENCODING & STORAGE
// =========================================================================

/// Frame encoding type.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum FrameEncoding {
    Plain = 0,
    Zstd = 1,           // zstd (used for portable bulk format)
    ZstdEncrypted = 2,   // zstd + AES-256-GCM
    Brotli = 3,          // brotli Q11 (best per-frame: 3.1x ratio, 0.1ms decompress)
    BrotliEncrypted = 4, // brotli + AES-256-GCM
    ZstdDict = 5,        // zstd with trained dictionary (H.265-inspired: shared context across frames)
    ZstdDictBlock = 6,   // frame lives inside a block-compressed blob (block_id + intra-block offset)
}

impl FrameEncoding {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Zstd,
            2 => Self::ZstdEncrypted,
            3 => Self::Brotli,
            4 => Self::BrotliEncrypted,
            5 => Self::ZstdDict,
            6 => Self::ZstdDictBlock,
            _ => Self::Plain,
        }
    }
}

/// Frame status — 3 states for lineage + user-deletion.
///
/// - `Active`: current version, returned by `active_doc_ids()` and indexed
///   by SCA/trigram/symbol. This is what search finds.
/// - `Tombstone`: superseded by a newer version (put() with same doc_id
///   tombstoned this one). NOT searchable, but kept in the file as part of
///   the cognitive lineage. `said history` walks these.
/// - `Deleted`: user-deleted via `forget()` / `delete()`. Not searchable,
///   not part of lineage, space reclaimable on the next compact.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum FrameStatus {
    Active = 0,
    Deleted = 1,    // user-deleted, reclaimable on compact
    Tombstone = 2,  // superseded by a newer version (lineage history)
}

/// Metadata for a single frame in the TOC.
#[derive(Debug, Clone)]
pub struct FrameMeta {
    /// Unique frame ID (sequential, never reused)
    pub id: u64,
    /// Document ID (user-provided, for SCA index mapping)
    pub doc_id: String,
    /// Byte offset in the data section where compressed payload starts
    pub offset: u64,
    /// Length of compressed payload in bytes
    pub compressed_len: u32,
    /// Length of original uncompressed payload
    pub uncompressed_len: u32,
    /// BLAKE3 checksum of COMPRESSED payload (verifies on-disk integrity)
    pub checksum: [u8; 32],
    /// Encoding type
    pub encoding: FrameEncoding,
    /// Frame status
    pub status: FrameStatus,
    /// Optional title/label
    pub title: Option<String>,
    /// Unix timestamp (seconds) when frame was created
    pub created_at: u64,
    /// Memory taxonomy
    pub memory_type: MemoryType,
    pub memory_kind: MemoryKind,
    pub subject: MemorySubject,
    pub scope: MemoryScope,
    /// Tags for filtering/categorization
    pub tags: Vec<String>,
    // ── Lineage (v7_2) ───────────────────────────────────────────────────
    /// If this frame is a `Tombstone`, the ID of the frame that replaced it.
    /// `None` for Active frames and for the oldest version in a lineage chain.
    /// Walk a lineage by following this pointer backward from the Active head.
    pub superseded_by: Option<u64>,
    /// Semantic delta (0.0..=1.0) from the frame that was being replaced when
    /// this frame was inserted. Computed at `put()` time from the XOR/POPCNT
    /// of the two frames' 1-bit SCA fingerprints, normalized to [0,1].
    ///
    /// - 0.00 means the new content is fingerprint-identical to the old
    ///   version (e.g. whitespace-only change). put() should skip these to
    ///   avoid lineage noise.
    /// - 0.10 means a small change (one line edited, typo fix)
    /// - 0.50 means half the semantic dimensions flipped (major rewrite)
    /// - 1.00 means every dimension flipped (completely different function)
    ///
    /// `0.0` is also used for the genesis frame (first time a doc_id appears).
    pub semantic_delta: f32,
    // ── Four-pillar CLS memory (Decision 1, 2026-04-21) ─────────────────
    /// Which CLS pillar this frame belongs to. Set at put-time for new
    /// frames; derived from `memory_type` via `Pillar::from_memory_type`
    /// for frames loaded from pre-pillar files. Zero behavior change in
    /// Decision 1 — retrieval ignores this field until Decision 2.
    pub pillar: Pillar,
}

/// A compressed block containing multiple frames (H.265 GOP-inspired).
/// Decompressing one block gives you all frames within it.
#[derive(Debug, Clone)]
pub struct CompressedBlock {
    /// Block ID (sequential)
    pub id: u32,
    /// Byte offset in the data section where this block's compressed blob starts
    pub offset: u64,
    /// Compressed size of the entire block
    pub compressed_len: u32,
    /// Uncompressed size of the entire block
    pub uncompressed_len: u32,
    /// Number of frames in this block
    pub frame_count: u16,
    /// Per-frame offsets within the UNCOMPRESSED block: (intra_offset, len)
    pub frame_offsets: Vec<(u32, u32)>,
}

/// In-memory frame store — manages the TOC and provides read/write.
/// The actual bytes live in the .said file; this tracks metadata.
pub struct FrameStore {
    /// All frame metadata (the TOC)
    frames: Vec<FrameMeta>,
    /// Fast lookup: doc_id → frame index
    doc_id_map: HashMap<String, usize>,
    /// Next frame ID to assign
    next_id: u64,
    /// Pending frame data (not yet flushed to file)
    pending: Vec<PendingFrame>,
    /// Compression level (default 3 for speed, 19 for max ratio)
    pub compression_level: i32,
    /// Trained Zstd dictionary (H.265-inspired shared context across frames).
    /// Trained from corpus samples; dramatically improves per-frame compression
    /// by eliminating cold-start overhead for domain-specific vocabulary.
    zstd_dict: Option<Vec<u8>>,
    /// Compressed blocks (H.265 GOP-inspired: multiple frames per block).
    /// For ZstdDictBlock-encoded frames, frame.offset = block's offset in file,
    /// and frame.compressed_len = block's compressed_len. The intra-block offset
    /// is stored in the block table.
    blocks: Vec<CompressedBlock>,
    /// Map: frame_id → (block_index, intra_block_frame_index)
    block_map: HashMap<u64, (usize, usize)>,
    /// Decompressed block cache: block_index → raw bytes.
    /// Avoids re-decompressing the same block on every frame read.
    /// ~400KB per block × 28 blocks = ~11MB for WikiMQA — trivial.
    block_cache: HashMap<usize, Vec<u8>>,
    /// Encryption key (AES-256, 32 bytes). None = no encryption.
    #[cfg(feature = "encryption")]
    encryption_key: Option<[u8; 32]>,
}

/// A frame waiting to be written to disk.
struct PendingFrame {
    meta: FrameMeta,
    compressed_data: Vec<u8>,
}

/// Is this frame under a `legal_hold:<id>` tag? Enterprise compliance guard —
/// `drop_tombstones` and `drop_tombstones_keep` skip any frame that carries
/// one. Hold is released by stripping the tag (admin action).
///
/// We accept any tag starting with `legal_hold:` so multiple concurrent
/// holds on one frame (case A and case B) each block deletion independently.
fn is_under_legal_hold(tags: &[String]) -> bool {
    tags.iter().any(|t| t.starts_with("legal_hold:") || t.as_str() == "legal_hold")
}

impl FrameStore {
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            doc_id_map: HashMap::new(),
            next_id: 0,
            pending: Vec::new(),
            compression_level: 3, // fast default
            zstd_dict: None,
            blocks: Vec::new(),
            block_map: HashMap::new(),
            block_cache: HashMap::new(),
            #[cfg(feature = "encryption")]
            encryption_key: None,
        }
    }

    /// Set encryption key (AES-256-GCM, 32 bytes).
    #[cfg(feature = "encryption")]
    pub fn set_encryption_key(&mut self, key: [u8; 32]) {
        self.encryption_key = Some(key);
    }

    /// Bytes currently held in the in-RAM `pending` buffer (sum of each
    /// pending frame's stored payload length). This is THE memory floor during
    /// ingest: every not-yet-saved frame keeps its RAW content here until
    /// save() compacts it. The streaming-spill path (#4) watches this against a
    /// byte budget and flushes pending → disk (mmap) when it grows too large,
    /// keeping `said init` at constant memory instead of holding the whole
    /// corpus in RAM until save().
    pub fn pending_bytes(&self) -> usize {
        self.pending.iter().map(|p| p.compressed_data.len()).sum()
    }

    /// Number of active (non-deleted) frames.
    pub fn active_count(&self) -> usize {
        self.frames.iter().filter(|f| f.status == FrameStatus::Active).count()
            + self.pending.iter().filter(|p| p.meta.status == FrameStatus::Active).count()
    }

    /// Total frames including deleted.
    pub fn total_count(&self) -> usize {
        self.frames.len() + self.pending.len()
    }

    /// Add a document with full taxonomy. Returns frame ID.
    pub fn put_with(&mut self, opts: &PutOptions) -> u64 {
        self.put_internal(opts.doc_id, opts.content.as_bytes(), opts.title,
            opts.memory_type, opts.memory_kind, opts.subject, opts.scope, opts.tags.clone())
    }

    /// `put_with` + explicit pillar override in one call.
    ///
    /// Without this, direct `put_with` callers that want a non-Semantic pillar
    /// (Code, Procedural, External) would hit the `from_memory_type(Factual) →
    /// Semantic` default and end up with the wrong `FrameMeta.pillar` on disk.
    /// The `remember_as_*` wrappers on `SaidFile` use this pattern; external
    /// callers (document_ingest, code_search, whisper_ingest, …) can switch
    /// to this helper when they care about pillar accuracy.
    ///
    /// Returns the new frame id. See also `set_pillar` for post-hoc override
    /// on frames already written.
    pub fn put_with_pillar(&mut self, opts: &PutOptions, pillar: Pillar) -> u64 {
        let frame_id = self.put_with(opts);
        self.set_pillar(frame_id, pillar);
        frame_id
    }

    /// Binary-friendly variant of put_with_pillar. Same internal path but
    /// takes raw bytes instead of going through the text-shaped PutOptions.
    ///
    /// Used by said-vault for image/font/xml asset frames where the bytes
    /// are not guaranteed UTF-8 (and where base64 wrapping would inflate
    /// storage by 33%).
    pub fn put_with_pillar_raw(
        &mut self,
        doc_id: &str,
        content: &[u8],
        title: Option<&str>,
        memory_type: MemoryType,
        memory_kind: MemoryKind,
        subject: MemorySubject,
        scope: MemoryScope,
        tags: Vec<String>,
        pillar: Pillar,
    ) -> u64 {
        let frame_id = self.put_internal(
            doc_id, content, title, memory_type, memory_kind, subject, scope, tags,
        );
        self.set_pillar(frame_id, pillar);
        frame_id
    }

    /// Add a document as a new frame (simple API — defaults to Episodic/Fact/User/Personal).
    /// Stores PLAIN initially (fast writes). Call compact() later to compress.
    pub fn put(&mut self, doc_id: &str, content: &[u8], title: Option<&str>) -> u64 {
        self.put_internal(doc_id, content, title,
            MemoryType::Episodic, MemoryKind::Fact, MemorySubject::User, MemoryScope::Personal, Vec::new())
    }

    fn put_internal(&mut self, doc_id: &str, content: &[u8], title: Option<&str>,
        memory_type: MemoryType, memory_kind: MemoryKind, subject: MemorySubject,
        scope: MemoryScope, tags: Vec<String>) -> u64 {
        let frame_id = self.next_id;
        self.next_id += 1;

        let timestamp = crate::time_compat::unix_secs();

        let uncompressed_len = content.len() as u32;

        // Store plain initially — compress lazily via compact()
        let final_data = content.to_vec();
        let encoding = FrameEncoding::Plain;

        // BLAKE3 checksum of on-disk data
        let checksum = *blake3::hash(&final_data).as_bytes();

        let meta = FrameMeta {
            id: frame_id,
            doc_id: doc_id.to_string(),
            offset: 0, // set during flush
            compressed_len: final_data.len() as u32,
            uncompressed_len,
            checksum,
            encoding,
            status: FrameStatus::Active,
            title: title.map(|s| s.to_string()),
            created_at: timestamp,
            memory_type,
            memory_kind,
            subject,
            scope,
            tags,
            superseded_by: None,   // set later by tombstone_for_replacement
            semantic_delta: 0.0,   // set later if this frame supersedes another
            // Decision 1: derive pillar from memory_type for every new frame.
            // Decision 3+ will let callers pick a pillar explicitly, but today
            // this preserves existing semantics exactly — Episodic→Episodic,
            // Factual→Semantic, Procedural→Procedural, Relational/Meta→Semantic.
            pillar: Pillar::from_memory_type(memory_type),
        };

        // Lineage: if a frame with this doc_id is already Active, tombstone
        // it — the new frame supersedes it. The doc_id_map will be updated
        // below to point at the new frame. The old Active frame becomes a
        // Tombstone and stays in the file as part of the cognitive history.
        // Semantic delta is filled in later by SaidFile::replace_frame() once
        // the SCA fingerprint for the new frame is available.
        if let Some(&old_idx) = self.doc_id_map.get(doc_id) {
            let frame_count = self.frames.len();
            if old_idx < frame_count {
                if self.frames[old_idx].status == FrameStatus::Active {
                    self.frames[old_idx].status = FrameStatus::Tombstone;
                    self.frames[old_idx].superseded_by = Some(frame_id);
                }
            } else {
                let pending_idx = old_idx - frame_count;
                if pending_idx < self.pending.len()
                    && self.pending[pending_idx].meta.status == FrameStatus::Active
                {
                    self.pending[pending_idx].meta.status = FrameStatus::Tombstone;
                    self.pending[pending_idx].meta.superseded_by = Some(frame_id);
                }
            }
        }

        self.doc_id_map.insert(doc_id.to_string(), self.frames.len() + self.pending.len());
        self.pending.push(PendingFrame { meta, compressed_data: final_data });

        frame_id
    }

    /// Compact: block-compress all frames with Zstd dictionary (Block 256).
    /// This is the standard compression path — H.265 GOP-inspired block compression
    /// with trained dictionary. 256 frames per block gives ~400KB decompression units
    /// at zstd's 1500 MB/s, enabling on-the-fly access.
    /// Returns (frames_compressed, bytes_saved).
    pub fn compact(&mut self, file_data: &[u8]) -> (usize, u64) {
        // Level 15: good balance between speed and size (~3x faster than level 19,
        // ~0.4% larger file). Full-project compact: 51s -> 27s on 27K frames.
        let (blocks, compressed_bytes, _dict_size) = self.compact_block_dict(file_data, 256, 112, 15);
        // Return in the same format as before for backward compatibility
        let total_uncompressed: u64 = self.frames.iter()
            .filter(|f| f.status == FrameStatus::Active)
            .map(|f| f.uncompressed_len as u64)
            .sum::<u64>()
            + self.pending.iter()
                .filter(|p| p.meta.status == FrameStatus::Active)
                .map(|p| p.meta.uncompressed_len as u64)
                .sum::<u64>();
        let saved = total_uncompressed.saturating_sub(compressed_bytes);
        (blocks, saved)
    }

    /// Block compression: group frames into blocks of `block_size`, compress each
    /// block as one unit with the trained dictionary. This is the H.265 GOP approach:
    /// individual frames are tiny and compress poorly alone, but a group of related
    /// frames compresses nearly as well as the whole corpus.
    ///
    /// Returns (blocks_created, total_compressed_bytes, dict_size).
    pub fn compact_block_dict(&mut self, file_data: &[u8], block_size: usize, dict_kb: usize, level: i32) -> (usize, u64, usize) {
        // Step 1: Collect all raw payloads (decompress any already-compressed frames)
        let mut raw_frames: Vec<(usize, Vec<u8>)> = Vec::new(); // (frame_index_in_pending, raw_bytes)

        // Move all committed frames to pending (with raw data) for re-packing.
        // Keep both Active AND Tombstone frames — tombstones are part of the
        // cognitive lineage and must survive compaction. Only genuinely
        // Deleted frames (user-removed) are dropped here.
        let mut to_move: Vec<usize> = Vec::new();
        for (i, frame) in self.frames.iter().enumerate() {
            if frame.status == FrameStatus::Active || frame.status == FrameStatus::Tombstone {
                to_move.push(i);
            }
        }
        for &i in to_move.iter().rev() {
            let frame = &self.frames[i];
            let raw = match frame.encoding {
                FrameEncoding::Plain => {
                    let start = frame.offset as usize;
                    let end = start + frame.compressed_len as usize;
                    if end > file_data.len() { continue; }
                    file_data[start..end].to_vec()
                }
                FrameEncoding::Zstd => {
                    let start = frame.offset as usize;
                    let end = start + frame.compressed_len as usize;
                    if end > file_data.len() { continue; }
                    match zstd::bulk::decompress(&file_data[start..end], frame.uncompressed_len as usize) {
                        Ok(d) => d, Err(_) => continue,
                    }
                }
                FrameEncoding::Brotli => {
                    let start = frame.offset as usize;
                    let end = start + frame.compressed_len as usize;
                    if end > file_data.len() { continue; }
                    match Self::brotli_decompress(&file_data[start..end], frame.uncompressed_len as usize) {
                        Some(d) => d, None => continue,
                    }
                }
                FrameEncoding::ZstdDictBlock => {
                    // Block-compressed frame: decompress via the block cache.
                    // read_block_frame needs &mut self for the cache, but we
                    // can't borrow mutably while iterating. Clone the meta and
                    // use a temporary self-borrow after the loop.
                    // For now: decompress inline using the block table + dict.
                    match self.read_block_frame_raw(frame, file_data) {
                        Some(d) => d, None => continue,
                    }
                }
                _ => continue,
            };
            let mut meta = self.frames.remove(i);
            meta.uncompressed_len = raw.len() as u32;
            meta.encoding = FrameEncoding::Plain;
            meta.compressed_len = raw.len() as u32;
            meta.offset = 0;
            self.pending.push(PendingFrame { meta, compressed_data: raw });
        }

        // Also decompress any already-compressed pending frames.
        // Keep Active AND Tombstone frames — tombstones are lineage history.
        for pending in &mut self.pending {
            if pending.meta.status == FrameStatus::Deleted { continue; }
            match pending.meta.encoding {
                FrameEncoding::Plain => {} // already raw
                FrameEncoding::Zstd | FrameEncoding::ZstdDict => {
                    if let Ok(raw) = zstd::bulk::decompress(&pending.compressed_data, pending.meta.uncompressed_len as usize) {
                        pending.compressed_data = raw;
                        pending.meta.encoding = FrameEncoding::Plain;
                    }
                }
                FrameEncoding::Brotli => {
                    if let Some(raw) = Self::brotli_decompress(&pending.compressed_data, pending.meta.uncompressed_len as usize) {
                        pending.compressed_data = raw;
                        pending.meta.encoding = FrameEncoding::Plain;
                    }
                }
                _ => {}
            }
        }

        // Collect indices of pending frames to re-pack into blocks.
        // Both Active and Tombstone participate so lineage is preserved.
        for (i, pending) in self.pending.iter().enumerate() {
            if pending.meta.status != FrameStatus::Deleted
                && pending.meta.encoding == FrameEncoding::Plain
            {
                raw_frames.push((i, pending.compressed_data.clone()));
            }
        }

        if raw_frames.len() < 10 {
            return (0, 0, 0);
        }

        // Step 2: Train dictionary from sampled frames.
        // Training on all 27K frames (~299MB) takes ~24s but only marginally
        // improves compression vs training on ~30MB of stride-sampled frames.
        // Capped sample keeps dict training O(constant) regardless of corpus size.
        const MAX_SAMPLE_BYTES: usize = 30 * 1024 * 1024;
        let total_bytes: usize = raw_frames.iter().map(|(_, d)| d.len()).sum();
        let (sample_sizes, flat_samples): (Vec<usize>, Vec<u8>) = if total_bytes <= MAX_SAMPLE_BYTES {
            let sizes = raw_frames.iter().map(|(_, d)| d.len()).collect();
            let flat = raw_frames.iter().flat_map(|(_, d)| d.iter().copied()).collect();
            (sizes, flat)
        } else {
            let stride = (total_bytes / MAX_SAMPLE_BYTES).max(1);
            let mut sizes = Vec::new();
            let mut flat = Vec::new();
            for (i, (_, data)) in raw_frames.iter().enumerate() {
                if i % stride == 0 {
                    sizes.push(data.len());
                    flat.extend_from_slice(data);
                    if flat.len() >= MAX_SAMPLE_BYTES { break; }
                }
            }
            (sizes, flat)
        };
        let dict = match zstd::dict::from_continuous(&flat_samples, &sample_sizes, dict_kb * 1024) {
            Ok(d) => d,
            Err(_) => return (0, 0, 0),
        };
        let dict_size = dict.len();
        drop(flat_samples);

        // Step 3: Group frames into blocks and compress each block.
        //
        // #4 throughput: block compression (zstd level-15) was ~99% of the save phase and fully
        // SERIAL (measured 18.6s on Amortization). Each block compresses INDEPENDENTLY with the
        // same shared dictionary, so we compress all blocks in PARALLEL (par_iter; each task
        // builds its own Compressor with the shared dict — Compressor isn't Sync), collecting
        // results IN BLOCK ORDER, then do the cheap serial merge into self.blocks/block_map/
        // pending. The output bytes are byte-identical to the serial version (same dict, same
        // level, same block grouping) — only the wall-clock changes. zstd is deterministic.
        use rayon::prelude::*;
        self.blocks.clear();
        self.block_map.clear();

        // Per-block compression (parallel). Each entry: (frame_offsets, uncompressed_len,
        // compressed_bytes) or None if this block's compressor failed (skipped, as before).
        struct BlockOut {
            frame_offsets: Vec<(u32, u32)>,
            uncompressed_len: u32,
            compressed: Vec<u8>,
        }
        let chunks: Vec<&[(usize, Vec<u8>)]> = raw_frames.chunks(block_size).collect();
        let dict_ref: &[u8] = &dict;
        let compressed_blocks: Vec<Option<BlockOut>> = chunks
            .par_iter()
            .map(|chunk| {
                // One compressor per task (with the shared dictionary).
                let mut compressor = zstd::bulk::Compressor::with_dictionary(level, dict_ref).ok()?;
                let mut block_raw = Vec::new();
                let mut frame_offsets: Vec<(u32, u32)> = Vec::new();
                for (_, data) in chunk.iter() {
                    let offset = block_raw.len() as u32;
                    let len = data.len() as u32;
                    frame_offsets.push((offset, len));
                    block_raw.extend_from_slice(data);
                }
                let uncompressed_len = block_raw.len() as u32;
                let compressed = compressor.compress(&block_raw).ok()?;
                Some(BlockOut { frame_offsets, uncompressed_len, compressed })
            })
            .collect();

        // Serial merge (in block order) — identical state to the old serial loop.
        let mut blocks_created = 0usize;
        let mut total_compressed: u64 = 0;
        for (chunk, out) in chunks.iter().zip(compressed_blocks.into_iter()) {
            let Some(out) = out else { continue }; // compressor failed → skip (as before)

            let block_id = blocks_created as u32;
            let block_idx = self.blocks.len();

            self.blocks.push(CompressedBlock {
                id: block_id,
                offset: 0, // set during flush
                compressed_len: out.compressed.len() as u32,
                uncompressed_len: out.uncompressed_len,
                frame_count: chunk.len() as u16,
                frame_offsets: out.frame_offsets,
            });

            // Update each frame's metadata to point to this block
            for (intra_idx, (pending_idx, _)) in chunk.iter().enumerate() {
                let pending = &mut self.pending[*pending_idx];
                pending.meta.encoding = FrameEncoding::ZstdDictBlock;
                self.block_map.insert(pending.meta.id, (block_idx, intra_idx));
            }

            // First frame in the chunk carries the compressed block bytes; the rest are
            // cleared (skipped during flush).
            let first_pending_idx = chunk[0].0;
            total_compressed += out.compressed.len() as u64;
            self.pending[first_pending_idx].compressed_data = out.compressed;
            for (pending_idx, _) in chunk.iter().skip(1) {
                self.pending[*pending_idx].compressed_data = Vec::new();
            }
            blocks_created += 1;
        }

        // Store dictionary
        self.zstd_dict = Some(dict);

        // Rebuild doc_id_map
        self.doc_id_map.clear();
        for (i, frame) in self.frames.iter().enumerate() {
            if frame.status == FrameStatus::Active {
                self.doc_id_map.insert(frame.doc_id.clone(), i);
            }
        }
        for (i, pending) in self.pending.iter().enumerate() {
            if pending.meta.status == FrameStatus::Active {
                self.doc_id_map.insert(pending.meta.doc_id.clone(), self.frames.len() + i);
            }
        }

        (blocks_created, total_compressed, dict_size)
    }

    /// Flush pending block-compressed data. Call this instead of flush_pending()
    /// when blocks are active. Returns (block_data_bytes, block_table_bytes).
    pub fn flush_block_pending(&mut self, base_offset: u64) -> (Vec<u8>, Vec<u8>) {
        self.flush_block_pending_with_source(base_offset, &[])
    }

    /// Same as `flush_block_pending`, but also copies forward the bytes of any
    /// existing (already-persisted) blocks by reading them from `source_data`
    /// (the caller's mmap of the old file). This is essential when `save()` is
    /// called on a brain opened from disk: without this, pre-existing blocks
    /// never get re-written, and their frame offsets point at garbage in the
    /// new file.
    pub fn flush_block_pending_with_source(
        &mut self,
        base_offset: u64,
        source_data: &[u8],
    ) -> (Vec<u8>, Vec<u8>) {
        let mut data = Vec::new();
        let mut current_offset = base_offset;

        // Collect which pending indices are "block leaders" (first frame in each block)
        let mut block_leaders: HashMap<u32, usize> = HashMap::new();
        for (frame_id, (block_idx, intra_idx)) in &self.block_map {
            if *intra_idx == 0 {
                for (i, p) in self.pending.iter().enumerate() {
                    if p.meta.id == *frame_id {
                        block_leaders.insert(*block_idx as u32, i);
                        break;
                    }
                }
            }
        }

        // Write each block's compressed data. A block is either:
        //   1. A pending block (compressed bytes in self.pending[leader].compressed_data), OR
        //   2. An existing block (compressed bytes already on disk — copy from source_data)
        for (block_idx, block) in self.blocks.iter_mut().enumerate() {
            if let Some(&leader_idx) = block_leaders.get(&(block_idx as u32)) {
                block.offset = current_offset;
                let compressed = &self.pending[leader_idx].compressed_data;
                data.extend_from_slice(compressed);
                current_offset += compressed.len() as u64;
            } else {
                // Existing block — must preserve by reading from source.
                let old_off = block.offset as usize;
                let comp_len = block.compressed_len as usize;
                if !source_data.is_empty()
                    && old_off > 0
                    && old_off + comp_len <= source_data.len()
                {
                    data.extend_from_slice(&source_data[old_off..old_off + comp_len]);
                    block.offset = current_offset;
                    current_offset += comp_len as u64;
                }
                // If source_data is empty/too small, we can't recover this block —
                // caller (save()) must supply a valid mmap for round-trips.
            }
        }

        // Update frame offsets: all frames in a block share the block's offset
        for pending in &mut self.pending {
            if pending.meta.encoding == FrameEncoding::ZstdDictBlock {
                if let Some(&(block_idx, _)) = self.block_map.get(&pending.meta.id) {
                    let block = &self.blocks[block_idx];
                    pending.meta.offset = block.offset;
                    pending.meta.compressed_len = block.compressed_len;
                }
            }
        }

        // Plain-encoded pending frames (e.g. frames added by `remember` /
        // `replace_frame` after the brain was already compacted) aren't part
        // of any block. They need to be written inline as raw payloads here,
        // with the frame's offset + compressed_len updated to point at the
        // new bytes. Without this step, `replace_frame` and `checkout` silently
        // corrupt the brain: the frame's metadata goes in but the payload
        // never reaches disk, so subsequent reads hit the wrong offset and
        // fail the BLAKE3 checksum.
        for pending in &mut self.pending {
            if pending.meta.encoding == FrameEncoding::Plain {
                let off = current_offset;
                let bytes = &pending.compressed_data;  // actually raw plaintext for Plain encoding
                data.extend_from_slice(bytes);
                current_offset += bytes.len() as u64;
                pending.meta.offset = off;
                pending.meta.compressed_len = bytes.len() as u32;
            }
        }

        // Move pending to committed
        for pending in self.pending.drain(..) {
            self.frames.push(pending.meta);
        }

        // Rebuild block_map with committed frame indices
        // (frame IDs haven't changed, just their location)

        // Rebuild doc_id_map
        self.doc_id_map.clear();
        for (i, frame) in self.frames.iter().enumerate() {
            if frame.status == FrameStatus::Active {
                self.doc_id_map.insert(frame.doc_id.clone(), i);
            }
        }

        // Serialize block table
        let block_table = self.serialize_block_table();

        (data, block_table)
    }

    /// Serialize block table for .said file.
    pub fn serialize_block_table(&self) -> Vec<u8> {
        if self.blocks.is_empty() { return Vec::new(); }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"BLKT"); // Block Table magic
        buf.extend_from_slice(&(self.blocks.len() as u32).to_le_bytes());

        for block in &self.blocks {
            buf.extend_from_slice(&block.id.to_le_bytes());
            buf.extend_from_slice(&block.offset.to_le_bytes());
            buf.extend_from_slice(&block.compressed_len.to_le_bytes());
            buf.extend_from_slice(&block.uncompressed_len.to_le_bytes());
            buf.extend_from_slice(&block.frame_count.to_le_bytes());
            for &(off, len) in &block.frame_offsets {
                buf.extend_from_slice(&off.to_le_bytes());
                buf.extend_from_slice(&len.to_le_bytes());
            }
        }

        // Frame-to-block mapping
        buf.extend_from_slice(&(self.block_map.len() as u32).to_le_bytes());
        for (&frame_id, &(block_idx, intra_idx)) in &self.block_map {
            buf.extend_from_slice(&frame_id.to_le_bytes());
            buf.extend_from_slice(&(block_idx as u32).to_le_bytes());
            buf.extend_from_slice(&(intra_idx as u32).to_le_bytes());
        }

        buf
    }

    /// Deserialize block table from .said file.
    pub fn deserialize_block_table(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 8 || &data[0..4] != b"BLKT" {
            return Err("Invalid BLKT".into());
        }
        let n_blocks = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let mut pos = 8;

        self.blocks.clear();
        for _ in 0..n_blocks {
            if pos + 18 > data.len() { break; }
            let id = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()); pos += 4;
            let offset = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap()); pos += 8;
            let compressed_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()); pos += 4;
            let uncompressed_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()); pos += 4;
            let frame_count = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()); pos += 2;

            let mut frame_offsets = Vec::new();
            for _ in 0..frame_count {
                if pos + 8 > data.len() { break; }
                let off = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()); pos += 4;
                let len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()); pos += 4;
                frame_offsets.push((off, len));
            }

            self.blocks.push(CompressedBlock { id, offset, compressed_len, uncompressed_len, frame_count, frame_offsets });
        }

        // Frame-to-block mapping
        if pos + 4 <= data.len() {
            let n_mappings = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;
            self.block_map.clear();
            for _ in 0..n_mappings {
                if pos + 16 > data.len() { break; }
                let frame_id = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap()); pos += 8;
                let block_idx = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize; pos += 4;
                let intra_idx = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize; pos += 4;
                self.block_map.insert(frame_id, (block_idx, intra_idx));
            }
        }

        Ok(())
    }

    /// Read a frame from a block-compressed blob. Uses block cache to avoid
    /// re-decompressing the same block on every frame read.
    fn read_block_frame(&mut self, frame: &FrameMeta, file_data: &[u8]) -> Option<Vec<u8>> {
        let &(block_idx, intra_idx) = self.block_map.get(&frame.id)?;
        let block = self.blocks.get(block_idx)?;
        let intra_off = block.frame_offsets.get(intra_idx)?.0 as usize;
        let intra_len = block.frame_offsets.get(intra_idx)?.1 as usize;
        let block_uncomp_len = block.uncompressed_len as usize;
        let block_offset = block.offset as usize;
        let block_comp_len = block.compressed_len as usize;

        // Check cache first
        if let Some(cached) = self.block_cache.get(&block_idx) {
            let frame_end = intra_off + intra_len;
            if frame_end <= cached.len() {
                return Some(cached[intra_off..frame_end].to_vec());
            }
        }

        // Cache miss: decompress block and cache it
        let end = block_offset + block_comp_len;
        if end > file_data.len() { return None; }
        let compressed = &file_data[block_offset..end];

        let dict = self.zstd_dict.as_ref()?;
        let mut decompressor = zstd::bulk::Decompressor::with_dictionary(dict).ok()?;
        let decompressed = decompressor.decompress(compressed, block_uncomp_len).ok()?;

        // Extract frame slice
        let frame_end = intra_off + intra_len;
        if frame_end > decompressed.len() { return None; }
        let result = decompressed[intra_off..frame_end].to_vec();

        // Cache the decompressed block
        self.block_cache.insert(block_idx, decompressed);

        Some(result)
    }

    /// Decompress a block-encoded frame WITHOUT caching. Used by
    /// compact_block_dict to extract raw payloads from already-block-compressed
    /// committed frames during re-compaction. Takes &self (no cache write).
    fn read_block_frame_raw(&self, frame: &FrameMeta, file_data: &[u8]) -> Option<Vec<u8>> {
        let &(block_idx, intra_idx) = self.block_map.get(&frame.id)?;
        let block = self.blocks.get(block_idx)?;
        let intra_off = block.frame_offsets.get(intra_idx)?.0 as usize;
        let intra_len = block.frame_offsets.get(intra_idx)?.1 as usize;
        let block_uncomp_len = block.uncompressed_len as usize;
        let block_offset = block.offset as usize;
        let block_comp_len = block.compressed_len as usize;

        let end = block_offset + block_comp_len;
        if end > file_data.len() { return None; }
        let compressed = &file_data[block_offset..end];

        let dict = self.zstd_dict.as_ref()?;
        let mut decompressor = zstd::bulk::Decompressor::with_dictionary(dict).ok()?;
        let decompressed = decompressor.decompress(compressed, block_uncomp_len).ok()?;

        let frame_end = intra_off + intra_len;
        if frame_end > decompressed.len() { return None; }
        Some(decompressed[intra_off..frame_end].to_vec())
    }

    /// Check if blocks are active (any frame uses block encoding).
    /// Check if frame is in block_map.
    pub fn block_map_lookup(&self, frame_id: u64) -> Option<(usize, usize)> {
        self.block_map.get(&frame_id).copied()
    }

    /// Number of entries in block_map.
    pub fn block_map_len(&self) -> usize {
        self.block_map.len()
    }

    pub fn has_blocks(&self) -> bool {
        !self.blocks.is_empty()
    }

    /// Count of uncompressed frames that would benefit from compact().
    pub fn uncompressed_count(&self) -> usize {
        let committed = self.frames.iter()
            .filter(|f| f.encoding == FrameEncoding::Plain && f.status == FrameStatus::Active && f.uncompressed_len >= 256)
            .count();
        let pending = self.pending.iter()
            .filter(|p| p.meta.encoding == FrameEncoding::Plain && p.meta.status == FrameStatus::Active && p.meta.uncompressed_len >= 256)
            .count();
        committed + pending
    }

    /// Mark a frame as deleted (tombstone). Does not reclaim space.
    pub fn delete(&mut self, doc_id: &str) -> bool {
        // User delete = RECOVERABLE (Tombstone), not a hard purge. Tombstone keeps the
        // payload on disk so `admin restore` can bring the memory back (the documented
        // Recycle Bin). Setting Deleted here purged the bytes on save, so a restored
        // frame failed its BLAKE3 check on reopen. Permanent removal is the explicit
        // `compact --drop-history` path (drop_tombstones), which converts to Deleted.
        if let Some(&idx) = self.doc_id_map.get(doc_id) {
            if idx < self.frames.len() {
                self.frames[idx].status = FrameStatus::Tombstone;
            } else {
                let pending_idx = idx - self.frames.len();
                if pending_idx < self.pending.len() {
                    self.pending[pending_idx].meta.status = FrameStatus::Tombstone;
                }
            }
            self.doc_id_map.remove(doc_id);
            true
        } else {
            false
        }
    }

    /// Get frame metadata by doc_id.
    pub fn get_meta(&self, doc_id: &str) -> Option<&FrameMeta> {
        if let Some(&idx) = self.doc_id_map.get(doc_id) {
            if idx < self.frames.len() {
                Some(&self.frames[idx])
            } else {
                let pending_idx = idx - self.frames.len();
                self.pending.get(pending_idx).map(|p| &p.meta)
            }
        } else {
            None
        }
    }

    /// Get all active doc_ids.
    pub fn active_doc_ids(&self) -> Vec<&str> {
        self.doc_id_map.keys().map(|s| s.as_str()).collect()
    }

    /// Walk the lineage of a doc_id backward from its current Active frame.
    ///
    /// Returns a Vec of frame metadata in chronological order (oldest first):
    ///
    /// - Index 0 is the genesis (first time this doc_id was inserted)
    /// - Each subsequent frame is a replacement that tombstoned the previous one
    /// - The last entry is the current Active frame (if any)
    ///
    /// The walk follows `superseded_by` pointers: starting from the Active
    /// frame, we look for any tombstone whose `superseded_by == active.id`,
    /// then recurse back from that tombstone, and so on.
    ///
    /// If `doc_id` has no frames at all (not in store), returns empty Vec.
    /// If `doc_id` has only tombstones (orphan lineage), walks them all.
    pub fn lineage(&self, doc_id: &str) -> Vec<&FrameMeta> {
        // Collect every frame (Active + Tombstone) matching this doc_id.
        // We walk committed + pending, filtering by exact doc_id match.
        let mut frames_for_doc: Vec<&FrameMeta> = Vec::new();
        for f in &self.frames {
            if f.doc_id == doc_id
                && (f.status == FrameStatus::Active || f.status == FrameStatus::Tombstone)
            {
                frames_for_doc.push(f);
            }
        }
        for p in &self.pending {
            if p.meta.doc_id == doc_id
                && (p.meta.status == FrameStatus::Active || p.meta.status == FrameStatus::Tombstone)
            {
                frames_for_doc.push(&p.meta);
            }
        }

        if frames_for_doc.is_empty() {
            return Vec::new();
        }

        // Sort by creation timestamp (chronological). The superseded_by chain
        // should match this ordering for well-formed files, but timestamp is
        // a robust secondary signal.
        frames_for_doc.sort_by_key(|f| f.created_at);
        frames_for_doc
    }

    /// Get a frame's metadata by frame_id (not doc_id).
    /// Returns None if no frame has that id (regardless of status).
    pub fn get_meta_by_id(&self, frame_id: u64) -> Option<&FrameMeta> {
        for f in &self.frames {
            if f.id == frame_id { return Some(f); }
        }
        for p in &self.pending {
            if p.meta.id == frame_id { return Some(&p.meta); }
        }
        None
    }

    /// Patch a frame's semantic_delta in place (used by replace_frame after
    /// the caller has computed the delta from the SCA fingerprints).
    pub fn set_semantic_delta(&mut self, frame_id: u64, delta: f32) {
        for f in &mut self.frames {
            if f.id == frame_id { f.semantic_delta = delta; return; }
        }
        for p in &mut self.pending {
            if p.meta.id == frame_id { p.meta.semantic_delta = delta; return; }
        }
    }

    /// Convert all Tombstone frames to Deleted status, so the next compact
    /// pass physically reclaims their storage. Active frames (current HEADs)
    /// are untouched — only cognitive lineage history is dropped.
    ///
    /// Frames tagged `legal_hold:<id>` are PRESERVED and skipped — this is
    /// the enterprise compliance hook. GDPR retention policies + legal
    /// holds never race: a hold placed before retention fires keeps the
    /// frame intact until the hold is explicitly lifted.
    ///
    /// Returns the number of tombstones dropped.
    pub fn drop_tombstones(&mut self) -> usize {
        let mut count = 0;
        for f in &mut self.frames {
            if f.status == FrameStatus::Tombstone && !is_under_legal_hold(&f.tags) {
                f.status = FrameStatus::Deleted;
                count += 1;
            }
        }
        for p in &mut self.pending {
            if p.meta.status == FrameStatus::Tombstone && !is_under_legal_hold(&p.meta.tags) {
                p.meta.status = FrameStatus::Deleted;
                count += 1;
            }
        }
        count
    }

    /// Drop older tombstones but keep the `keep_per_doc` most recent per
    /// doc_id. Groups tombstones by doc_id, sorts each group by created_at
    /// descending (newest first), keeps the first N, marks the rest as
    /// Deleted. Active frames are never touched.
    ///
    /// Returns the number of tombstones dropped.
    pub fn drop_tombstones_keep(&mut self, keep_per_doc: usize) -> usize {
        use std::collections::HashMap;

        // Gather (source, index, doc_id, created_at, frame_id).
        // source: 0 = committed, 1 = pending.
        // frame_id breaks created_at ties — it's monotonically assigned at
        // put() time, so higher id == later insertion even when timestamps
        // collide (common on fast reindex chains).
        let mut tombstones: Vec<(u8, usize, String, u64, u64)> = Vec::new();
        for (i, f) in self.frames.iter().enumerate() {
            if f.status == FrameStatus::Tombstone {
                tombstones.push((0, i, f.doc_id.clone(), f.created_at, f.id));
            }
        }
        for (i, p) in self.pending.iter().enumerate() {
            if p.meta.status == FrameStatus::Tombstone {
                tombstones.push((1, i, p.meta.doc_id.clone(), p.meta.created_at, p.meta.id));
            }
        }

        // Group by doc_id, sort each group newest-first (ts desc, then id desc
        // to break ties), mark everything past `keep_per_doc` for deletion.
        let mut by_doc: HashMap<String, Vec<(u8, usize, u64, u64)>> = HashMap::new();
        for (src, idx, doc_id, ts, fid) in tombstones {
            by_doc.entry(doc_id).or_default().push((src, idx, ts, fid));
        }

        let mut count = 0;
        for (_doc_id, mut group) in by_doc {
            group.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)));
            for (src, idx, _ts, _fid) in group.into_iter().skip(keep_per_doc) {
                // Same legal-hold guard as drop_tombstones — individual frames
                // can be preserved across retention sweeps.
                let tags: &[String] = if src == 0 {
                    &self.frames[idx].tags
                } else {
                    &self.pending[idx].meta.tags
                };
                if is_under_legal_hold(tags) { continue; }
                if src == 0 {
                    self.frames[idx].status = FrameStatus::Deleted;
                } else {
                    self.pending[idx].meta.status = FrameStatus::Deleted;
                }
                count += 1;
            }
        }
        count
    }

    /// Count Tombstone frames (cognitive lineage overhead in frame count).
    pub fn tombstone_count(&self) -> usize {
        let committed = self.frames.iter().filter(|f| f.status == FrameStatus::Tombstone).count();
        let pending = self.pending.iter().filter(|p| p.meta.status == FrameStatus::Tombstone).count();
        committed + pending
    }

    /// Sum of uncompressed_len across all Tombstone frames — visible overhead.
    pub fn tombstone_bytes(&self) -> u64 {
        let mut bytes: u64 = 0;
        for f in &self.frames {
            if f.status == FrameStatus::Tombstone {
                bytes += f.uncompressed_len as u64;
            }
        }
        for p in &self.pending {
            if p.meta.status == FrameStatus::Tombstone {
                bytes += p.meta.uncompressed_len as u64;
            }
        }
        bytes
    }

    /// Tombstone the Active frame with this doc_id (no replacement).
    /// Used when a source file disappears on re-init — its lineage is preserved
    /// as history but no new HEAD takes its place.
    pub fn tombstone(&mut self, doc_id: &str) -> bool {
        let Some(&idx) = self.doc_id_map.get(doc_id) else { return false };
        let frame_count = self.frames.len();
        if idx < frame_count {
            let f = &mut self.frames[idx];
            if f.status == FrameStatus::Active {
                f.status = FrameStatus::Tombstone;
                return true;
            }
        } else {
            let p_idx = idx - frame_count;
            if p_idx < self.pending.len() {
                let m = &mut self.pending[p_idx].meta;
                if m.status == FrameStatus::Active {
                    m.status = FrameStatus::Tombstone;
                    return true;
                }
            }
        }
        false
    }

    /// Append a tag to the Active frame with this doc_id if not already present.
    pub fn add_tag(&mut self, doc_id: &str, tag: &str) {
        let Some(&idx) = self.doc_id_map.get(doc_id) else { return };
        let frame_count = self.frames.len();
        if idx < frame_count {
            let f = &mut self.frames[idx];
            if f.status == FrameStatus::Active && !f.tags.iter().any(|t| t == tag) {
                f.tags.push(tag.to_string());
            }
        } else {
            let p_idx = idx - frame_count;
            if p_idx < self.pending.len() {
                let m = &mut self.pending[p_idx].meta;
                if m.status == FrameStatus::Active && !m.tags.iter().any(|t| t == tag) {
                    m.tags.push(tag.to_string());
                }
            }
        }
    }

    /// Get active doc_ids filtered by scope.
    pub fn doc_ids_by_scope(&self, scope: MemoryScope) -> Vec<&str> {
        let mut result = Vec::new();
        for (_i, frame) in self.frames.iter().enumerate() {
            if frame.status == FrameStatus::Active && frame.scope == scope {
                result.push(frame.doc_id.as_str());
            }
        }
        for pending in &self.pending {
            if pending.meta.status == FrameStatus::Active && pending.meta.scope == scope {
                result.push(pending.meta.doc_id.as_str());
            }
        }
        result
    }

    /// Get active doc_ids filtered by memory type.
    pub fn doc_ids_by_type(&self, memory_type: MemoryType) -> Vec<&str> {
        let mut result = Vec::new();
        for frame in &self.frames {
            if frame.status == FrameStatus::Active && frame.memory_type == memory_type {
                result.push(frame.doc_id.as_str());
            }
        }
        for pending in &self.pending {
            if pending.meta.status == FrameStatus::Active && pending.meta.memory_type == memory_type {
                result.push(pending.meta.doc_id.as_str());
            }
        }
        result
    }

    /// Get active doc_ids filtered by tag.
    pub fn doc_ids_by_tag(&self, tag: &str) -> Vec<&str> {
        let mut result = Vec::new();
        for frame in &self.frames {
            if frame.status == FrameStatus::Active && frame.tags.iter().any(|t| t == tag) {
                result.push(frame.doc_id.as_str());
            }
        }
        for pending in &self.pending {
            if pending.meta.status == FrameStatus::Active && pending.meta.tags.iter().any(|t| t == tag) {
                result.push(pending.meta.doc_id.as_str());
            }
        }
        result
    }

    /// Read and decompress a frame's content from the raw file data.
    /// `file_data` is the entire .said file (or mmap'd region).
    /// Read the raw decompressed payload of a frame by its frame_id.
    /// Works for Active AND Tombstone frames — used by `said checkout` to
    /// restore a past version.
    pub fn read_frame_by_id(&mut self, frame_id: u64, file_data: &[u8]) -> Option<Vec<u8>> {
        // Try pending first
        let pending_match = self.pending.iter().find(|p| p.meta.id == frame_id)
            .map(|p| (p.meta.clone(), p.compressed_data.clone()));
        if let Some((meta, on_disk)) = pending_match {
            if meta.encoding == FrameEncoding::ZstdDictBlock {
                return self.read_block_frame(&meta, file_data);
            }
            return match meta.encoding {
                FrameEncoding::Plain => Some(on_disk),
                FrameEncoding::Zstd | FrameEncoding::ZstdEncrypted =>
                    zstd::bulk::decompress(&on_disk, meta.uncompressed_len as usize).ok(),
                FrameEncoding::Brotli | FrameEncoding::BrotliEncrypted =>
                    Self::brotli_decompress(&on_disk, meta.uncompressed_len as usize),
                FrameEncoding::ZstdDict =>
                    self.zstd_dict_decompress(&on_disk, meta.uncompressed_len as usize),
                FrameEncoding::ZstdDictBlock => unreachable!(),
            };
        }

        // Committed: scan by id (lineage lookups are rare so linear scan is fine)
        let meta = self.frames.iter().find(|f| f.id == frame_id)?.clone();
        if meta.status == FrameStatus::Deleted { return None; }

        if meta.encoding == FrameEncoding::ZstdDictBlock {
            return self.read_block_frame(&meta, file_data);
        }

        let start = meta.offset as usize;
        let end = start + meta.compressed_len as usize;
        if end > file_data.len() { return None; }
        let on_disk = &file_data[start..end];

        match meta.encoding {
            FrameEncoding::Plain => Some(on_disk.to_vec()),
            FrameEncoding::Zstd | FrameEncoding::ZstdEncrypted =>
                zstd::bulk::decompress(on_disk, meta.uncompressed_len as usize).ok(),
            FrameEncoding::Brotli | FrameEncoding::BrotliEncrypted =>
                Self::brotli_decompress(on_disk, meta.uncompressed_len as usize),
            FrameEncoding::ZstdDict =>
                self.zstd_dict_decompress(on_disk, meta.uncompressed_len as usize),
            FrameEncoding::ZstdDictBlock => unreachable!(),
        }
    }

    pub fn read_frame(&mut self, doc_id: &str, file_data: &[u8]) -> Option<Vec<u8>> {
        // Check pending frames first (not yet flushed to file)
        // Clone data out to avoid borrow conflict with &mut self for block cache
        let pending_match = self.pending.iter().find(|p| {
            p.meta.doc_id == doc_id && p.meta.status == FrameStatus::Active
        }).map(|p| (p.meta.clone(), p.compressed_data.clone()));

        if let Some((meta, on_disk)) = pending_match {
            if meta.encoding == FrameEncoding::ZstdDictBlock {
                return self.read_block_frame(&meta, file_data);
            }
            let computed = *blake3::hash(&on_disk).as_bytes();
            if computed != meta.checksum {
                eprintln!("[SAID] BLAKE3 mismatch for pending frame {} ({})", meta.id, meta.doc_id);
                return None;
            }
            return match meta.encoding {
                FrameEncoding::Plain => Some(on_disk),
                FrameEncoding::Zstd | FrameEncoding::ZstdEncrypted =>
                    zstd::bulk::decompress(&on_disk, meta.uncompressed_len as usize).ok(),
                FrameEncoding::Brotli | FrameEncoding::BrotliEncrypted =>
                    Self::brotli_decompress(&on_disk, meta.uncompressed_len as usize),
                FrameEncoding::ZstdDict =>
                    self.zstd_dict_decompress(&on_disk, meta.uncompressed_len as usize),
                FrameEncoding::ZstdDictBlock => unreachable!(),
            };
        }

        // Clone committed frame metadata to release borrow
        let meta = self.get_meta(doc_id)?.clone();
        if meta.status == FrameStatus::Deleted { return None; }

        // Block-encoded frames: skip per-frame BLAKE3 (integrity checked by zstd at block level)
        if meta.encoding == FrameEncoding::ZstdDictBlock {
            return self.read_block_frame(&meta, file_data);
        }

        let start = meta.offset as usize;
        let end = start + meta.compressed_len as usize;
        if end > file_data.len() { return None; }

        let on_disk = &file_data[start..end];

        // Verify BLAKE3
        let computed = *blake3::hash(on_disk).as_bytes();
        if computed != meta.checksum {
            eprintln!("[SAID] BLAKE3 mismatch for frame {} ({})", meta.id, meta.doc_id);
            return None;
        }

        // Decrypt if needed
        let compressed = match meta.encoding {
            #[cfg(feature = "encryption")]
            FrameEncoding::ZstdEncrypted => {
                if let Some(key) = &self.encryption_key {
                    Self::decrypt_aes256gcm(key, on_disk, meta.id).ok()?
                } else {
                    return None;
                }
            }
            _ => on_disk.to_vec(),
        };

        // Decompress (legacy formats for reading old files)
        match meta.encoding {
            FrameEncoding::Plain => Some(compressed),
            FrameEncoding::Zstd | FrameEncoding::ZstdEncrypted =>
                zstd::bulk::decompress(&compressed, meta.uncompressed_len as usize).ok(),
            FrameEncoding::Brotli | FrameEncoding::BrotliEncrypted =>
                Self::brotli_decompress(&compressed, meta.uncompressed_len as usize),
            FrameEncoding::ZstdDict =>
                self.zstd_dict_decompress(&compressed, meta.uncompressed_len as usize),
            FrameEncoding::ZstdDictBlock => unreachable!(),
        }
    }

    /// Read frame as UTF-8 text.
    pub fn read_frame_text(&mut self, doc_id: &str, file_data: &[u8]) -> Option<String> {
        let bytes = self.read_frame(doc_id, file_data)?;
        String::from_utf8(bytes).ok()
    }

    // =========================================================================
    // LEGACY DECOMPRESSION (for reading old .said files with Brotli/Zstd)
    // =========================================================================

    fn brotli_decompress(data: &[u8], max_size: usize) -> Option<Vec<u8>> {
        let mut output = Vec::with_capacity(max_size);
        brotli::BrotliDecompress(
            &mut std::io::Cursor::new(data),
            &mut output,
        ).ok()?;
        Some(output)
    }

    // =========================================================================
    // ZSTD DICTIONARY (H.265-inspired: shared context eliminates cold-start)
    // =========================================================================

    /// Train a Zstd dictionary from all active frame payloads.
    /// The dictionary captures domain-specific patterns (like H.265's CABAC
    /// learns statistical properties of video data). Typically 32-112KB.
    /// Returns dictionary size in bytes, or 0 if training failed/not enough data.
    pub fn train_dictionary(&mut self, file_data: &[u8], max_dict_size: usize) -> usize {
        let mut samples: Vec<Vec<u8>> = Vec::new();

        // Collect raw payloads from committed frames
        for frame in &self.frames {
            if frame.status != FrameStatus::Active { continue; }
            let start = frame.offset as usize;
            let end = start + frame.compressed_len as usize;
            if end > file_data.len() { continue; }
            let raw = match frame.encoding {
                FrameEncoding::Plain => file_data[start..end].to_vec(),
                FrameEncoding::Zstd => {
                    zstd::bulk::decompress(&file_data[start..end], frame.uncompressed_len as usize)
                        .unwrap_or_default()
                }
                FrameEncoding::Brotli => {
                    Self::brotli_decompress(&file_data[start..end], frame.uncompressed_len as usize)
                        .unwrap_or_default()
                }
                _ => continue, // skip encrypted/dict-compressed for training
            };
            if !raw.is_empty() { samples.push(raw); }
        }

        // Collect from pending frames
        for pending in &self.pending {
            if pending.meta.status != FrameStatus::Active { continue; }
            let raw = match pending.meta.encoding {
                FrameEncoding::Plain => pending.compressed_data.clone(),
                FrameEncoding::Zstd => {
                    zstd::bulk::decompress(&pending.compressed_data, pending.meta.uncompressed_len as usize)
                        .unwrap_or_default()
                }
                FrameEncoding::Brotli => {
                    Self::brotli_decompress(&pending.compressed_data, pending.meta.uncompressed_len as usize)
                        .unwrap_or_default()
                }
                _ => continue,
            };
            if !raw.is_empty() { samples.push(raw); }
        }

        // Need enough samples for meaningful dictionary training
        if samples.len() < 10 {
            return 0;
        }

        // Train dictionary using zstd's built-in algorithm
        let sample_sizes: Vec<usize> = samples.iter().map(|s| s.len()).collect();
        let flat: Vec<u8> = samples.into_iter().flatten().collect();

        match zstd::dict::from_continuous(&flat, &sample_sizes, max_dict_size) {
            Ok(dict) => {
                let size = dict.len();
                self.zstd_dict = Some(dict);
                size
            }
            Err(_) => 0,
        }
    }

    /// Decompress data using the trained dictionary (legacy per-frame + block reads).
    fn zstd_dict_decompress(&self, data: &[u8], max_size: usize) -> Option<Vec<u8>> {
        let dict = self.zstd_dict.as_ref()?;
        let mut decompressor = zstd::bulk::Decompressor::with_dictionary(dict).ok()?;
        decompressor.decompress(data, max_size).ok()
    }

    /// Get the trained dictionary bytes (for serialization into .said file).
    pub fn get_dictionary(&self) -> Option<&[u8]> {
        self.zstd_dict.as_deref()
    }

    /// Set dictionary from deserialized .said file.
    pub fn set_dictionary(&mut self, dict: Vec<u8>) {
        self.zstd_dict = Some(dict);
    }

    // =========================================================================
    // ENCRYPTION (AES-256-GCM)
    // =========================================================================

    #[cfg(feature = "encryption")]
    fn encrypt_aes256gcm(key: &[u8; 32], data: &[u8], frame_id: u64) -> Vec<u8> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        use aes_gcm::aead::Aead;

        let cipher = Aes256Gcm::new(key.into());
        // Nonce: 12 bytes from frame_id (deterministic per frame, never reused if IDs are unique)
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&frame_id.to_le_bytes());
        let nonce = Nonce::from_slice(&nonce_bytes);

        cipher.encrypt(nonce, data).unwrap_or_else(|_| data.to_vec())
    }

    #[cfg(feature = "encryption")]
    fn decrypt_aes256gcm(key: &[u8; 32], data: &[u8], frame_id: u64) -> Result<Vec<u8>, String> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        use aes_gcm::aead::Aead;

        let cipher = Aes256Gcm::new(key.into());
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&frame_id.to_le_bytes());
        let nonce = Nonce::from_slice(&nonce_bytes);

        cipher.decrypt(nonce, data).map_err(|e| format!("AES decrypt failed: {}", e))
    }

    // =========================================================================
    // SERIALIZATION — TOC for .said file
    // =========================================================================

    /// Serialize all frame data (pending) into a byte buffer.
    /// Returns (frame_data_bytes, updated_metas_with_offsets).
    /// Call this to get the bytes to write to the .said file data section.
    pub fn flush_pending(&mut self, base_offset: u64) -> Vec<u8> {
        let mut data = Vec::new();
        let mut current_offset = base_offset;

        for pending in &mut self.pending {
            pending.meta.offset = current_offset;
            data.extend_from_slice(&pending.compressed_data);
            current_offset += pending.compressed_data.len() as u64;
        }

        // Move pending to committed
        for pending in self.pending.drain(..) {
            self.frames.push(pending.meta);
        }

        // Rebuild doc_id_map
        self.doc_id_map.clear();
        for (i, frame) in self.frames.iter().enumerate() {
            if frame.status == FrameStatus::Active {
                self.doc_id_map.insert(frame.doc_id.clone(), i);
            }
        }

        data
    }

    /// Serialize TOC (frame metadata table) to bytes.
    pub fn serialize_toc(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"FTOC"); // Frame TOC magic
        buf.extend_from_slice(&(self.frames.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.next_id.to_le_bytes());

        for frame in &self.frames {
            buf.extend_from_slice(&frame.id.to_le_bytes());
            let doc_bytes = frame.doc_id.as_bytes();
            buf.extend_from_slice(&(doc_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(doc_bytes);
            buf.extend_from_slice(&frame.offset.to_le_bytes());
            buf.extend_from_slice(&frame.compressed_len.to_le_bytes());
            buf.extend_from_slice(&frame.uncompressed_len.to_le_bytes());
            buf.extend_from_slice(&frame.checksum);
            buf.push(frame.encoding as u8);
            buf.push(frame.status as u8);
            buf.extend_from_slice(&frame.created_at.to_le_bytes());
            // Title (optional)
            if let Some(ref title) = frame.title {
                let tb = title.as_bytes();
                buf.extend_from_slice(&(tb.len() as u16).to_le_bytes());
                buf.extend_from_slice(tb);
            } else {
                buf.extend_from_slice(&0u16.to_le_bytes());
            }
            // Taxonomy (4 bytes)
            buf.push(frame.memory_type as u8);
            buf.push(frame.memory_kind as u8);
            buf.push(frame.subject as u8);
            buf.push(frame.scope as u8);
            // Tags
            buf.extend_from_slice(&(frame.tags.len() as u16).to_le_bytes());
            for tag in &frame.tags {
                let tb = tag.as_bytes();
                buf.extend_from_slice(&(tb.len() as u16).to_le_bytes());
                buf.extend_from_slice(tb);
            }
            // Lineage (v7_2): superseded_by + semantic_delta.
            // Encoded as 8 bytes (u64, 0 = None) + 4 bytes (f32).
            // Old readers stop at the end of tags — new readers detect the
            // extra bytes by position.
            let superseded = frame.superseded_by.unwrap_or(0);
            buf.extend_from_slice(&superseded.to_le_bytes());
            buf.extend_from_slice(&frame.semantic_delta.to_le_bytes());
            // Pillar (Decision 1, 2026-04-21): 1 byte appended after lineage.
            // Readers that stop earlier default to Pillar::from_memory_type().
            buf.push(frame.pillar as u8);
        }

        buf
    }

    /// Deserialize TOC from bytes.
    pub fn deserialize_toc(data: &[u8]) -> Result<Self, String> {
        if data.len() < 12 || &data[0..4] != b"FTOC" {
            return Err("Invalid FTOC".into());
        }

        let n_frames = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let next_id = u64::from_le_bytes(data[8..16].try_into().unwrap());
        let mut pos = 16;

        let mut store = FrameStore::new();
        store.next_id = next_id;

        for _ in 0..n_frames {
            if pos + 8 > data.len() { break; }
            let id = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            pos += 8;

            if pos + 2 > data.len() { break; }
            let doc_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
            pos += 2;
            if pos + doc_len > data.len() { break; }
            let doc_id = String::from_utf8_lossy(&data[pos..pos+doc_len]).to_string();
            pos += doc_len;

            if pos + 8 + 4 + 4 + 32 + 1 + 1 + 8 > data.len() { break; }
            let offset = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            pos += 8;
            let compressed_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            pos += 4;
            let uncompressed_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            pos += 4;
            let mut checksum = [0u8; 32];
            checksum.copy_from_slice(&data[pos..pos+32]);
            pos += 32;
            let encoding = FrameEncoding::from_byte(data[pos]);
            pos += 1;
            let status = match data[pos] {
                1 => FrameStatus::Deleted,
                2 => FrameStatus::Tombstone,  // v7_2
                _ => FrameStatus::Active,
            };
            pos += 1;
            let created_at = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            pos += 8;

            // Title
            let title = if pos + 2 <= data.len() {
                let tlen = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                pos += 2;
                if tlen > 0 && pos + tlen <= data.len() {
                    let t = String::from_utf8_lossy(&data[pos..pos+tlen]).to_string();
                    pos += tlen;
                    Some(t)
                } else {
                    None
                }
            } else {
                None
            };

            // Taxonomy (4 bytes, backward compatible — defaults if missing)
            let memory_type = if pos < data.len() { let v = MemoryType::from_byte(data[pos]); pos += 1; v } else { MemoryType::Episodic };
            let memory_kind = if pos < data.len() { let v = MemoryKind::from_byte(data[pos]); pos += 1; v } else { MemoryKind::Fact };
            let subject = if pos < data.len() { let v = MemorySubject::from_byte(data[pos]); pos += 1; v } else { MemorySubject::User };
            let scope = if pos < data.len() { let v = MemoryScope::from_byte(data[pos]); pos += 1; v } else { MemoryScope::Personal };

            // Tags
            let mut tags = Vec::new();
            if pos + 2 <= data.len() {
                let n_tags = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                pos += 2;
                for _ in 0..n_tags {
                    if pos + 2 > data.len() { break; }
                    let tlen = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                    pos += 2;
                    if pos + tlen <= data.len() {
                        tags.push(String::from_utf8_lossy(&data[pos..pos+tlen]).to_string());
                        pos += tlen;
                    }
                }
            }

            // Lineage fields (v7_2 — backwards compatible):
            //   u64 superseded_by (0 = None)
            //   f32 semantic_delta
            // If fewer bytes remain, we're reading a v7 file and defaults apply.
            let (superseded_by, semantic_delta) = if pos + 12 <= data.len() {
                let sup = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
                pos += 8;
                let delta = f32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
                pos += 4;
                (if sup == 0 { None } else { Some(sup) }, delta)
            } else {
                (None, 0.0)
            };

            // Pillar (Decision 1, 2026-04-21): 1 byte after lineage.
            // Pre-pillar files stop here — derive from memory_type.
            let pillar = if pos < data.len() {
                let p = Pillar::from_byte(data[pos]);
                pos += 1;
                p
            } else {
                Pillar::from_memory_type(memory_type)
            };

            let meta = FrameMeta {
                id, doc_id: doc_id.clone(), offset, compressed_len, uncompressed_len,
                checksum, encoding, status, title, created_at,
                memory_type, memory_kind, subject, scope, tags,
                superseded_by, semantic_delta,
                pillar,
            };

            // Only Active frames win the doc_id_map — Tombstones stay in the
            // file but don't participate in lookup-by-doc_id. Deleted frames
            // are filtered out of iteration by active_doc_ids().
            if status == FrameStatus::Active {
                store.doc_id_map.insert(doc_id, store.frames.len());
            }
            store.frames.push(meta);
        }

        Ok(store)
    }

    /// Admin view — every frame that's in Tombstone or Deleted status,
    /// newest first, across both committed and pending. Includes status,
    /// tags, `superseded_by`, and `created_at` so callers can render a
    /// full deletion log. Active frames are excluded.
    pub fn admin_tombstone_records(&self) -> Vec<&FrameMeta> {
        // Recycle Bin = recoverable frames only (Tombstone keeps its payload). Deleted
        // frames were hard-purged by `compact --drop-history` and cannot be restored, so
        // they are not shown here (showing them implied a restore that BLAKE3-fails).
        let mut out: Vec<&FrameMeta> = self.frames.iter()
            .chain(self.pending.iter().map(|p| &p.meta))
            .filter(|m| m.status == FrameStatus::Tombstone)
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        out
    }

    /// Admin restore — find a Tombstone frame for `doc_id` and flip it back
    /// to Active. Only the most-recent tombstone for that doc_id is restored
    /// (by `created_at` then `id`); earlier tombstones stay as lineage.
    /// If the doc_id currently has an Active HEAD, that HEAD is tombstoned
    /// first so there's still exactly one Active version per doc_id.
    ///
    /// Returns `Ok((restored_frame_id, displaced_active_id))` on success.
    /// `displaced_active_id` is `Some` when a current HEAD was demoted.
    /// Returns `Err` when no Tombstone for that doc_id exists.
    pub fn admin_restore_tombstoned(&mut self, doc_id: &str) -> Result<(u64, Option<u64>), String> {
        // Restore the newest TOMBSTONE for doc_id. Tombstones keep their payload, so
        // they're recoverable. (Deleted = compact --drop-history purged the bytes; those
        // are intentionally unrecoverable and are excluded from the Recycle Bin view.)
        let restorable = |s: FrameStatus| s == FrameStatus::Tombstone;
        let mut best: Option<(u8, usize, u64, u64)> = None;  // (src, idx, ts, fid)
        for (i, f) in self.frames.iter().enumerate() {
            if restorable(f.status) && f.doc_id == doc_id {
                match best {
                    None => best = Some((0, i, f.created_at, f.id)),
                    Some((_, _, ts, id)) if (f.created_at, f.id) > (ts, id) =>
                        best = Some((0, i, f.created_at, f.id)),
                    _ => {}
                }
            }
        }
        for (i, p) in self.pending.iter().enumerate() {
            if restorable(p.meta.status) && p.meta.doc_id == doc_id {
                match best {
                    None => best = Some((1, i, p.meta.created_at, p.meta.id)),
                    Some((_, _, ts, id)) if (p.meta.created_at, p.meta.id) > (ts, id) =>
                        best = Some((1, i, p.meta.created_at, p.meta.id)),
                    _ => {}
                }
            }
        }
        let (src, idx, _ts, restored_id) = best
            .ok_or_else(|| format!("'{}' is not in the recycle bin (never deleted, or already permanently cleared)", doc_id))?;

        // Displace the current Active head (if any) so there's exactly one Active.
        let mut displaced: Option<u64> = None;
        if let Some(&active_idx) = self.doc_id_map.get(doc_id) {
            if self.frames[active_idx].status == FrameStatus::Active {
                displaced = Some(self.frames[active_idx].id);
                self.frames[active_idx].status = FrameStatus::Tombstone;
                self.frames[active_idx].superseded_by = Some(restored_id);
            }
        }

        // Flip the chosen tombstone back to Active + clear superseded_by.
        if src == 0 {
            self.frames[idx].status = FrameStatus::Active;
            self.frames[idx].superseded_by = None;
            self.doc_id_map.insert(doc_id.to_string(), idx);
        } else {
            self.pending[idx].meta.status = FrameStatus::Active;
            self.pending[idx].meta.superseded_by = None;
        }

        Ok((restored_id, displaced))
    }

    /// Set the pillar on a specific frame after `put_with` — used by the
    /// `remember_as_*` family so explicit pillar intents (Code, Procedural,
    /// External, Episodic) override the default `from_memory_type` derivation
    /// (which always maps Factual → Semantic). Pending frames are also
    /// searched. Returns `true` if the frame existed and was updated.
    pub fn set_pillar(&mut self, frame_id: u64, pillar: Pillar) -> bool {
        for f in &mut self.frames {
            if f.id == frame_id { f.pillar = pillar; return true; }
        }
        for p in &mut self.pending {
            if p.meta.id == frame_id { p.meta.pillar = pillar; return true; }
        }
        false
    }

    /// Admin action — mark a single frame as Deleted by frame_id, regardless
    /// of current status. Used by the retention sweep after legal-hold and
    /// keep-per-doc filters have been applied by the caller. Returns `true`
    /// when a frame was flipped, `false` when no matching frame existed.
    pub fn mark_frame_deleted(&mut self, frame_id: u64) -> bool {
        for f in &mut self.frames {
            if f.id == frame_id && f.status != FrameStatus::Deleted {
                // Respect the legal-hold invariant — callers already filter,
                // but double-check here so a bug in the caller can't reap a
                // held frame.
                if is_under_legal_hold(&f.tags) { return false; }
                f.status = FrameStatus::Deleted;
                return true;
            }
        }
        for p in &mut self.pending {
            if p.meta.id == frame_id && p.meta.status != FrameStatus::Deleted {
                if is_under_legal_hold(&p.meta.tags) { return false; }
                p.meta.status = FrameStatus::Deleted;
                return true;
            }
        }
        false
    }

    /// Admin action — add a `legal_hold:<case_id>` tag to every frame
    /// (Active, Tombstone, Deleted — all status) with this doc_id. The
    /// tag blocks `drop_tombstones` / `drop_tombstones_keep` from reaping
    /// any frame in that lineage. Returns the number of frames tagged.
    /// Idempotent — re-adding the same hold is a no-op.
    pub fn admin_add_legal_hold(&mut self, doc_id: &str, case_id: &str) -> usize {
        let tag = format!("legal_hold:{}", case_id);
        let mut count = 0;
        for f in &mut self.frames {
            if f.doc_id == doc_id && !f.tags.iter().any(|t| t == &tag) {
                f.tags.push(tag.clone());
                count += 1;
            }
        }
        for p in &mut self.pending {
            if p.meta.doc_id == doc_id && !p.meta.tags.iter().any(|t| t == &tag) {
                p.meta.tags.push(tag.clone());
                count += 1;
            }
        }
        count
    }

    /// Admin action — remove the `legal_hold:<case_id>` tag (lift the hold)
    /// from every frame with this doc_id. Returns frames affected.
    pub fn admin_release_legal_hold(&mut self, doc_id: &str, case_id: &str) -> usize {
        let tag = format!("legal_hold:{}", case_id);
        let mut count = 0;
        for f in &mut self.frames {
            if f.doc_id == doc_id {
                let before = f.tags.len();
                f.tags.retain(|t| t != &tag);
                if f.tags.len() < before { count += 1; }
            }
        }
        for p in &mut self.pending {
            if p.meta.doc_id == doc_id {
                let before = p.meta.tags.len();
                p.meta.tags.retain(|t| t != &tag);
                if p.meta.tags.len() < before { count += 1; }
            }
        }
        count
    }

    /// Get all frame metadata (for save/rewrite).
    pub fn get_all_frames(&self) -> Vec<&FrameMeta> {
        self.frames.iter().collect()
    }

    /// Get all frame metadata INCLUDING pending (pre-flush) frames. Dream
    /// calls this so newly-written Episodic frames are visible for
    /// consolidation before save. Everything else should keep using
    /// `get_all_frames` to see only flushed, persisted state.
    pub fn get_all_frames_with_pending(&self) -> Vec<&FrameMeta> {
        let mut out: Vec<&FrameMeta> = self.frames.iter().collect();
        for p in &self.pending {
            out.push(&p.meta);
        }
        out
    }

    /// Update a frame's offset (after re-writing file).
    pub fn update_offset(&mut self, frame_id: u64, new_offset: u64) {
        for frame in &mut self.frames {
            if frame.id == frame_id {
                frame.offset = new_offset;
                return;
            }
        }
    }

    /// Fix offsets for frames that were just flushed from pending (had offset=0).
    pub fn fix_pending_offsets(&mut self, _base: u64) {
        // Pending frames are already flushed to self.frames by flush_pending().
        // Their offsets were set during flush_pending(). This is a no-op now
        // since we handle offset updates in update_offset().
    }

    /// Get storage stats.
    pub fn stats(&self) -> FrameStoreStats {
        let active = self.frames.iter().filter(|f| f.status == FrameStatus::Active).count();

        // For block-compressed files, use block-level stats (not per-frame which double-counts)
        let (total_compressed, total_uncompressed) = if !self.blocks.is_empty() {
            let comp: u64 = self.blocks.iter().map(|b| b.compressed_len as u64).sum();
            let uncomp: u64 = self.blocks.iter().map(|b| b.uncompressed_len as u64).sum();
            (comp, uncomp)
        } else {
            let comp: u64 = self.frames.iter()
                .filter(|f| f.status == FrameStatus::Active)
                .map(|f| f.compressed_len as u64).sum();
            let uncomp: u64 = self.frames.iter()
                .filter(|f| f.status == FrameStatus::Active)
                .map(|f| f.uncompressed_len as u64).sum();
            (comp, uncomp)
        };

        FrameStoreStats {
            active_frames: active,
            deleted_frames: self.frames.len() - active,
            total_compressed_bytes: total_compressed,
            total_uncompressed_bytes: total_uncompressed,
            compression_ratio: if total_compressed > 0 {
                total_uncompressed as f64 / total_compressed as f64
            } else { 1.0 },
            pending_frames: self.pending.len(),
        }
    }

    /// Approximate resident heap bytes of the FrameStore's in-RAM holders (#4 scale).
    /// `pending` holds every not-yet-flushed frame's COMPRESSED content until save() — on a
    /// fresh full init that is the entire corpus, the dominant memory floor. Gated diagnostic.
    pub fn frame_store_mem_report(&self) -> String {
        let pending_bytes: usize = self.pending.iter()
            .map(|p| p.compressed_data.len() + std::mem::size_of::<PendingFrame>()).sum();
        let blocks_bytes: usize = self.blocks.iter().map(|b| b.compressed_len as usize).sum();
        let cache_bytes: usize = self.block_cache.values().map(|v| v.len()).sum();
        let frames_meta: usize = self.frames.len() * std::mem::size_of::<FrameMeta>();
        let mb = |b: usize| (b as f64) / 1_048_576.0;
        format!(
            "frame_store_mem: pending={:.0}MB ({} frames) blocks={:.0}MB block_cache={:.0}MB \
             frames_meta={:.0}MB",
            mb(pending_bytes), self.pending.len(), mb(blocks_bytes),
            mb(cache_bytes), mb(frames_meta),
        )
    }
}

/// Frame store statistics.
#[derive(Debug)]
pub struct FrameStoreStats {
    pub active_frames: usize,
    pub deleted_frames: usize,
    pub total_compressed_bytes: u64,
    pub total_uncompressed_bytes: u64,
    pub compression_ratio: f64,
    pub pending_frames: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pillar_document_variant_roundtrips_via_u8() {
        let pillar = Pillar::Document;
        let tag = pillar as u8;
        assert_eq!(tag, 6, "Pillar::Document must use u8 tag 6 (next after Memory=5)");
        let restored = Pillar::from_byte(tag);
        assert_eq!(restored, Pillar::Document);
    }

    #[test]
    fn pillar_document_wire_tag_pins_at_6_and_roundtrips() {
        // Verify the on-disk representation of Document is stable at 6.
        // The TOC serializer writes `frame.pillar as u8` and the deserializer
        // reads it back via `Pillar::from_byte`. This test proves the round-trip
        // for the Document variant specifically, and also pins every pre-existing
        // variant's tag so any accidental renumbering is caught immediately.
        let encoded: u8 = Pillar::Document as u8;
        assert_eq!(encoded, 6);
        let decoded = Pillar::from_byte(6);
        assert_eq!(decoded, Pillar::Document);
        // Confirm all other variants are unaffected by the addition.
        assert_eq!(Pillar::Episodic as u8, 0,
            "Pillar::Episodic wire tag changed from 0 — breaks existing .said files");
        assert_eq!(Pillar::Semantic as u8, 1,
            "Pillar::Semantic wire tag changed from 1 — breaks existing .said files");
        assert_eq!(Pillar::Procedural as u8, 2,
            "Pillar::Procedural wire tag changed from 2 — breaks existing .said files");
        assert_eq!(Pillar::External as u8, 3,
            "Pillar::External wire tag changed from 3 — breaks existing .said files");
        assert_eq!(Pillar::Code as u8, 4,
            "Pillar::Code wire tag changed from 4 — breaks existing .said files");
        assert_eq!(Pillar::Memory as u8, 5,
            "Pillar::Memory wire tag changed from 5 — breaks existing .said files");
    }
}

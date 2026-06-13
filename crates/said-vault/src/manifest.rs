//! Document manifest — JSON content of a Pillar::Document frame tagged
//! `vault:manifest`. Records everything needed for byte-faithful rebuild:
//! asset hashes in zip-entry order, ingest provenance, optional tombstone
//! hash for restore-path verification.

use serde::{Deserialize, Serialize};

/// One entry in the ordered zip-entry list. Order is load-bearing for
/// byte-faithful rebuild — do NOT sort.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZipEntry {
    /// Original entry path inside the source archive (e.g. "word/document.xml").
    pub name: String,
    /// BLAKE3 hex of the entry's canonicalized content. Looks up the asset
    /// frame in the vault.
    pub asset_hash: String,
    /// Asset kind discriminator: "paragraph" | "image" | "font" | "xml".
    /// Not a Rust enum — kept as String so future kinds don't break old
    /// manifests.
    pub kind: String,
}

/// One ingested document's manifest. The JSON content of a
/// `vault:manifest:<doc_id>` frame in the vault's `.said` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// BLAKE3 hex of the original source file bytes. Globally identifies
    /// this document across the vault.
    pub doc_id: String,
    /// Source format: "docx" | "pdf" (extensible — kept as String).
    pub format: String,
    /// Original filename for human-readable display + export.
    pub filename: String,
    /// Source file size in bytes (pre-dedup).
    pub size_bytes: u64,
    /// RFC 3339 timestamp of ingestion.
    pub ingested_at: String,
    /// Authenticated user identity that performed the ingest.
    pub ingested_by: String,
    /// Caller-supplied tags applied to this manifest (access control,
    /// classification, department, customer, case, etc.).
    pub tags: Vec<String>,
    /// Asset references in their original zip-entry order. Order is
    /// load-bearing — the rebuilder re-emits entries in this exact order.
    pub zip_entries: Vec<ZipEntry>,
    /// BLAKE3 hex of the byte-exact original. `None` means the manifest
    /// was ingested in slim mode (no tombstone stored, restore unavailable).
    pub tombstone_hash: Option<String>,
}

impl Manifest {
    /// Parse a manifest from its JSON content. Errors as serde_json::Error.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Serialize to JSON. Infallible by construction (every field is
    /// trivially serializable).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Manifest serde must not fail")
    }
}

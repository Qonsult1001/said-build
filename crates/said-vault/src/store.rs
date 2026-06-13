//! SaidStore — adapter from vault-rust's Store API onto sca_core::SaidFile.
//!
//! Vault assets become Pillar::Document frames with doc_id of shape
//! "vault:{kind-prefix}:{hash}". Block compression and storage efficiency
//! come for free from the underlying SaidFile.
//!
//! Binary kinds (image, font, xml) use put_binary to avoid base64
//! inflation. Text kinds (paragraph) also use put_binary for
//! consistency — UTF-8 text is valid binary and roundtrips cleanly via
//! read_binary.

use std::collections::HashMap;

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

pub struct SaidStore {
    brain: SaidFile,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub total_objects: u64,
    pub total_data_size: u64,
    pub objects_by_kind: HashMap<String, u64>,
}

impl SaidStore {
    pub fn create(path: &str) -> Self {
        let brain = SaidFile::create(path);
        Self { brain }
    }

    pub fn open(path: &str) -> Result<Self, String> {
        let brain = SaidFile::open(path)?;
        Ok(Self { brain })
    }

    /// Doc_id naming: "vault:{prefix}:{hash}" with prefix derived from kind:
    /// paragraph → para, image → img, font → font, xml → xml.
    fn make_doc_id(kind: &str, hash: &str) -> String {
        let prefix = match kind {
            "paragraph"            => "para",
            "image"                => "img",
            "font"                 => "font",
            "xml" | "docx_xml"     => "xml",
            other                  => other,
        };
        format!("vault:{}:{}", prefix, hash)
    }

    /// Store a vault asset. Idempotent on (kind, hash) collisions — a second
    /// put for an existing doc_id is a no-op (mirrors SQLite INSERT OR IGNORE).
    pub fn put_object(&mut self, hash: &str, kind: &str, data: &[u8]) -> Result<(), String> {
        let doc_id = Self::make_doc_id(kind, hash);
        // Dedup: if the frame is already active, skip.
        if self.brain.frames.get_meta(&doc_id).is_some() {
            return Ok(());
        }
        let tags = vec![
            "vault:asset".into(),
            format!("vault:kind:{}", kind),
        ];
        self.brain.put_binary(&doc_id, data, Pillar::Document, tags);
        Ok(())
    }

    /// Fetch a vault asset's raw bytes by either bare hash or full doc_id.
    pub fn get_object(&mut self, hash_or_doc_id: &str) -> Result<Option<Vec<u8>>, String> {
        // If caller passed a full doc_id, look it up directly.
        if hash_or_doc_id.starts_with("vault:") {
            return Ok(self.brain.read_binary(hash_or_doc_id));
        }
        // Otherwise iterate the 4 known kind prefixes and return the first hit.
        for prefix in &["para", "xml", "img", "font"] {
            let doc_id = format!("vault:{}:{}", prefix, hash_or_doc_id);
            if let Some(bytes) = self.brain.read_binary(&doc_id) {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    /// Kind-aware fetch — constructs the exact doc_id from (kind, hash) and reads
    /// it directly. Use this from rebuild paths where the manifest already carries
    /// `kind`; it avoids the prefix-iteration in `get_object` and rules out
    /// hash-collision miss-routing across kinds.
    pub fn get_object_kinded(&mut self, kind: &str, hash: &str) -> Result<Option<Vec<u8>>, String> {
        let doc_id = Self::make_doc_id(kind, hash);
        Ok(self.brain.read_binary(&doc_id))
    }

    pub fn object_exists(&mut self, hash: &str) -> Result<bool, String> {
        Ok(self.get_object(hash)?.is_some())
    }

    pub fn object_count(&self) -> Result<u64, String> {
        let count = self.brain.frames.active_doc_ids().iter()
            .filter(|did| {
                did.starts_with("vault:") &&
                (did.contains(":para:") || did.contains(":xml:")
                 || did.contains(":img:") || did.contains(":font:"))
            })
            .count() as u64;
        Ok(count)
    }

    pub fn stats(&self) -> Result<Stats, String> {
        let mut stats = Stats::default();
        for did in self.brain.frames.active_doc_ids() {
            let kind = if did.contains(":para:") { "paragraph" }
                else if did.contains(":xml:") { "xml" }
                else if did.contains(":img:") { "image" }
                else if did.contains(":font:") { "font" }
                else { continue };
            stats.total_objects += 1;
            *stats.objects_by_kind.entry(kind.into()).or_insert(0) += 1;
            if let Some(meta) = self.brain.frames.get_meta(did) {
                stats.total_data_size += meta.uncompressed_len as u64;
            }
        }
        Ok(stats)
    }

    pub fn brain(&self) -> &SaidFile { &self.brain }
    pub fn brain_mut(&mut self) -> &mut SaidFile { &mut self.brain }
}

//! Vault tombstone storage — byte-exact original document blobs for
//! said-vault enterprise tier. Distinct from the existing personal-memory
//! tombstone section per said-vault Track B spec. Lazily allocated on
//! a SaidFile in Task 3 of the plan.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct VaultTombstone {
    pub doc_id: String,
    pub blake3: [u8; 32],
    pub compressed_bytes: Vec<u8>,
    pub original_size: u64,
}

#[derive(Debug, Default)]
pub struct VaultTombstoneStore {
    entries: HashMap<String, VaultTombstone>,
    dirty: bool,
}

impl VaultTombstoneStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a tombstone keyed by doc_id. Returns the BLAKE3 of the
    /// ORIGINAL (uncompressed) bytes so callers can record the hash in
    /// the corresponding manifest frame.
    ///
    /// Bytes are held raw in memory; the whole section is zstd block-
    /// compressed in `to_bytes()` so cross-document redundancy (shared
    /// fonts, templates, logos embedded in every DOCX) is captured — a
    /// per-entry compress would miss that and barely dent already-zipped
    /// DOCX/PDF blobs. The field name `compressed_bytes` is historical;
    /// it holds the raw original bytes in memory.
    pub fn put(&mut self, doc_id: &str, bytes: &[u8]) -> [u8; 32] {
        let blake3 = *blake3::hash(bytes).as_bytes();
        let key = doc_id.to_string();
        let entry = VaultTombstone {
            doc_id: key.clone(),
            blake3,
            compressed_bytes: bytes.to_vec(),
            original_size: bytes.len() as u64,
        };
        self.entries.insert(key, entry);
        self.dirty = true;
        blake3
    }

    /// Fetch a tombstone's original bytes. Returns None if not present
    /// OR if the stored bytes fail BLAKE3 verification (corruption check
    /// happens here, not at higher layers).
    pub fn get(&self, doc_id: &str) -> Option<Vec<u8>> {
        let entry = self.entries.get(doc_id)?;
        let computed = *blake3::hash(&entry.compressed_bytes).as_bytes();
        if computed != entry.blake3 {
            eprintln!("[vault_tombstone] BLAKE3 mismatch on get({}); refusing to return bytes", doc_id);
            return None;
        }
        Some(entry.compressed_bytes.clone())
    }

    pub fn remove(&mut self, doc_id: &str) -> Result<(), String> {
        if self.entries.remove(doc_id).is_none() {
            return Err(format!("tombstone not found: {}", doc_id));
        }
        self.dirty = true;
        Ok(())
    }

    pub fn iter_doc_ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn total_bytes(&self) -> u64 {
        self.entries.values().map(|e| e.original_size).sum()
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(crate) fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Serialize the section for inclusion in a .said file. The section body
    /// (all entries, sorted by doc_id) is zstd block-compressed so cross-
    /// document redundancy is captured. Layout:
    ///   [0..4]  magic "VTS2"
    ///   [4..12] uncompressed body length (u64 LE)
    ///   [12..]  zstd-compressed body
    /// The body is the same entry encoding the legacy VTS1 raw format used
    /// inline; `from_bytes` reads both VTS1 (raw) and VTS2 (compressed).
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.encode_body();
        let compressed = zstd::bulk::compress(&body, COMPRESSION_LEVEL)
            .unwrap_or_else(|_| body.clone());
        let mut buf = Vec::with_capacity(12 + compressed.len());
        buf.extend_from_slice(SECTION_MAGIC);
        buf.extend_from_slice(&(body.len() as u64).to_le_bytes());
        buf.extend_from_slice(&compressed);
        buf
    }

    /// Encode all entries (sorted by doc_id) into the raw section body. This
    /// is the payload zstd compresses in `to_bytes`, and the VTS1 legacy raw
    /// format minus its 4-byte magic prefix.
    fn encode_body(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.total_bytes() as usize + self.entries.len() * 80);
        buf.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());

        let mut sorted: Vec<&VaultTombstone> = self.entries.values().collect();
        sorted.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));

        for entry in sorted {
            buf.extend_from_slice(&entry.blake3);
            buf.extend_from_slice(&entry.original_size.to_le_bytes());
            let id_bytes = entry.doc_id.as_bytes();
            buf.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(id_bytes);
            buf.extend_from_slice(&(entry.compressed_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(&entry.compressed_bytes);
        }
        buf
    }

    /// Deserialize from bytes (read from the .said file's VAULT_TOMBSTONES
    /// section). Accepts both VTS2 (zstd-compressed body) and the legacy VTS1
    /// (raw inline) layouts. Returns Err on malformed input.
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() >= 4 && &data[..4] == SECTION_MAGIC {
            // VTS2: magic(4) + uncompressed_len(8) + zstd body
            if data.len() < 12 {
                return Err("VAULT_TOMBSTONES (VTS2) too short for header".into());
            }
            let body_len = u64::from_le_bytes(data[4..12].try_into().unwrap()) as usize;
            let body = zstd::bulk::decompress(&data[12..], body_len)
                .map_err(|e| format!("VAULT_TOMBSTONES zstd decompress: {}", e))?;
            return Self::decode_body(&body);
        }
        if data.len() >= 4 && &data[..4] == SECTION_MAGIC_V1 {
            // Legacy VTS1: magic(4) + raw body. Strip magic, decode body.
            return Self::decode_body(&data[4..]);
        }
        Err(format!("VAULT_TOMBSTONES magic mismatch: expected {:?} or {:?} got {:?}",
            SECTION_MAGIC, SECTION_MAGIC_V1, &data[..data.len().min(4)]))
    }

    /// Decode the raw section body (count + entries) into a store.
    fn decode_body(data: &[u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("VAULT_TOMBSTONES body too short for count".into());
        }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut store = Self::new();
        let mut cursor = 4;
        const ENTRY_FIXED_LEN: usize = 32 + 8 + 4; // blake3 + original_size + doc_id_len

        for i in 0..count {
            if cursor + ENTRY_FIXED_LEN > data.len() {
                return Err(format!("VAULT_TOMBSTONES entry {} truncated at fixed header", i));
            }
            let mut blake3 = [0u8; 32];
            blake3.copy_from_slice(&data[cursor..cursor + 32]);
            cursor += 32;
            let original_size = u64::from_le_bytes(data[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let id_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            if cursor + id_len + 4 > data.len() {
                return Err(format!("VAULT_TOMBSTONES entry {} truncated in doc_id", i));
            }
            let doc_id = std::str::from_utf8(&data[cursor..cursor + id_len])
                .map_err(|e| format!("VAULT_TOMBSTONES entry {} doc_id not utf-8: {}", i, e))?
                .to_string();
            cursor += id_len;
            let content_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            if cursor + content_len > data.len() {
                return Err(format!("VAULT_TOMBSTONES entry {} truncated in content", i));
            }
            let content = data[cursor..cursor + content_len].to_vec();
            cursor += content_len;

            store.entries.insert(doc_id.clone(), VaultTombstone {
                doc_id,
                blake3,
                compressed_bytes: content,
                original_size,
            });
        }
        store.dirty = false;
        Ok(store)
    }
}

/// Current section magic — VTS2 = zstd block-compressed body.
pub const SECTION_MAGIC: &[u8; 4] = b"VTS2";
/// Legacy section magic — VTS1 = raw inline body (read-only, for old files).
pub const SECTION_MAGIC_V1: &[u8; 4] = b"VTS1";
/// zstd level for the tombstone section. 15 matches the frame-table compactor;
/// the body is mostly already-zipped DOCX/PDF so the win comes from cross-doc
/// redundancy (shared fonts/templates), not from re-compressing each blob.
const COMPRESSION_LEVEL: i32 = 15;

//! State serialization/deserialization for ScaEngine.
//!
//! Formats:
//!   SCRM     — breadcrumbs (fingerprints + corpus mean + doc IDs, 24B/doc)
//!   SAID v4  — portable (breadcrumbs + flat text blob + CRC32)
//!   SCA\0    — legacy full (CrystallineCore blob + texts + IDF)
//!   SCS\0    — legacy slim (CrystallineCore blob only)
//!
//! WAL: write_safe() writes to .said.tmp then atomic renames to .said.
//!      If .said.tmp exists on load, the previous write crashed — discard it.

use std::collections::HashMap;
use crate::engine::ScaEngine;
use crate::CrystallineCore;

/// WAL-safe write: serialize to temp file, then atomic rename.
/// If the process crashes mid-write, the original .said is untouched.
pub fn write_safe(path: &str, data: &[u8]) -> Result<(), String> {
    let tmp_path = format!("{}.tmp", path);

    // Write to temp file
    std::fs::write(&tmp_path, data)
        .map_err(|e| format!("Failed to write {}: {}", tmp_path, e))?;

    // Atomic rename (on same filesystem, this is instant + atomic)
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed to rename {} -> {}: {}", tmp_path, path, e))?;

    Ok(())
}

/// WAL-safe read: if .said.tmp exists, a previous write crashed — warn and use .said.
pub fn read_safe(path: &str) -> Result<Vec<u8>, String> {
    let tmp_path = format!("{}.tmp", path);

    // Check for crashed write
    if std::path::Path::new(&tmp_path).exists() {
        eprintln!("[WAL] Found {}, previous write crashed. Using last good {}.", tmp_path, path);
        // Clean up the partial write
        let _ = std::fs::remove_file(&tmp_path);
    }

    std::fs::read(path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))
}

/// Serialize full state (SCA segment + texts). For standalone .said files.
pub fn serialize_state(engine: &ScaEngine) -> Result<Vec<u8>, String> {
    let core_bytes = engine.core.serialize_index(true);
    if core_bytes.is_empty() {
        return Err("serialize_index returned empty bytes".to_string());
    }

    let mut buf = Vec::new();
    buf.extend_from_slice(b"SCA\0");
    buf.extend_from_slice(&(core_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(&core_bytes);

    // Normalized texts
    buf.extend_from_slice(&(engine.doc_texts_normalized.len() as u32).to_le_bytes());
    for text in &engine.doc_texts_normalized {
        let tb = text.as_bytes();
        buf.extend_from_slice(&(tb.len() as u32).to_le_bytes());
        buf.extend_from_slice(tb);
    }

    // Word IDF
    buf.extend_from_slice(&(engine.word_idf.len() as u32).to_le_bytes());
    for (word, idf) in &engine.word_idf {
        let wb = word.as_bytes();
        buf.extend_from_slice(&(wb.len() as u16).to_le_bytes());
        buf.extend_from_slice(wb);
        buf.extend_from_slice(&idf.to_le_bytes());
    }

    // Original texts (preserves exact tokenization for word index rebuild)
    buf.extend_from_slice(&(engine.doc_texts_original.len() as u32).to_le_bytes());
    for text in &engine.doc_texts_original {
        let tb = text.as_bytes();
        buf.extend_from_slice(&(tb.len() as u32).to_le_bytes());
        buf.extend_from_slice(tb);
    }

    Ok(buf)
}

/// Serialize enterprise .said file (breadcrumbs + brain, NO frames).
/// Documents stay at original location. WAL-safe with CRC32.
///
/// Format: [SCRM breadcrumbs] [BRAN brain state] [CRC32 4B]
///
/// Identical to portable format EXCEPT no frames section.
/// Both include: fingerprints, corpus_mean/std, brain, CRC32, WAL.
pub fn serialize_state_slim(engine: &ScaEngine) -> Result<Vec<u8>, String> {
    let mut buf = engine.core.serialize_breadcrumbs();
    if buf.is_empty() {
        return Err("serialize_breadcrumbs returned empty".to_string());
    }

    // BRAIN section (query log + reconsolidation + dream accumulator)
    buf.extend_from_slice(&engine.brain.serialize());

    // CRC32 integrity (same as portable — WAL crash detection)
    let crc = crc32_simple(&buf);
    buf.extend_from_slice(&crc.to_le_bytes());

    Ok(buf)
}

pub fn deserialize_state(bytes: &[u8]) -> Result<ScaEngine, String> {
    if bytes.len() >= 8 && &bytes[0..4] == b"SAID" {
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version == 5 {
            return deserialize_portable(bytes);
        }
        // version 2 = old full CrystallineCore format (handled by deserialize_index)
    }
    if bytes.len() >= 4 && &bytes[0..4] == b"SCA\0" {
        return deserialize_sca(bytes);
    }
    if bytes.len() >= 4 && &bytes[0..4] == b"SCS\0" {
        return deserialize_slim(bytes);
    }
    if bytes.len() >= 4 && &bytes[0..4] == b"SCRM" {
        return deserialize_breadcrumbs(bytes);
    }
    deserialize_legacy(bytes)
}

/// Deserialize enterprise .said (SCRM format) — fingerprints + brain + CRC32.
/// Word structures MUST be rebuilt from original text via rebuild_entity_data().
fn deserialize_breadcrumbs(bytes: &[u8]) -> Result<ScaEngine, String> {
    // Verify CRC32 if present (last 4 bytes)
    if bytes.len() >= 8 {
        let stored_crc = u32::from_le_bytes(bytes[bytes.len()-4..].try_into().unwrap());
        let computed_crc = crc32_simple(&bytes[..bytes.len()-4]);
        if stored_crc == computed_crc {
            // CRC matches — file is intact (strip CRC for parsing)
        } else if stored_crc != 0 {
            // CRC mismatch and non-zero — might be corrupted, or old format without CRC
            // Try parsing anyway (backward compat with pre-CRC .said files)
            eprintln!("[WAL] CRC32 mismatch — .said file may be corrupted or pre-CRC format");
        }
    }

    let mut core = CrystallineCore::new();
    core.deserialize_breadcrumbs(bytes)?;

    // Look for BRAIN section after breadcrumbs
    let brain = if let Some(brain_pos) = find_magic(bytes, b"BRAN") {
        crate::brain::Brain::deserialize(&bytes[brain_pos..]).unwrap_or_else(|_| crate::brain::Brain::new())
    } else {
        crate::brain::Brain::new()
    };

    Ok(ScaEngine {
        core,
        doc_texts_normalized: Vec::new(),
        doc_texts_original: Vec::new(),
        word_idf: HashMap::new(),
        brain,
        #[cfg(feature = "static-embed")]
        static_encoder: None,
        #[cfg(feature = "gpu")]
        gpu_search: None,
    })
}

fn deserialize_sca(bytes: &[u8]) -> Result<ScaEngine, String> {
    let mut pos = 4;

    if pos + 4 > bytes.len() { return Err("Truncated".to_string()); }
    let core_len = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;

    if pos + core_len > bytes.len() { return Err("Truncated core".to_string()); }
    let mut core = CrystallineCore::new();
    core.deserialize_index(&bytes[pos..pos+core_len])?;
    pos += core_len;

    // Normalized texts
    let mut doc_texts_normalized = Vec::new();
    if pos + 4 <= bytes.len() {
        let n = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        for _ in 0..n {
            if pos + 4 > bytes.len() { break; }
            let tlen = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + tlen > bytes.len() { break; }
            doc_texts_normalized.push(String::from_utf8_lossy(&bytes[pos..pos+tlen]).to_string());
            pos += tlen;
        }
    }

    // Word IDF
    let mut word_idf = HashMap::new();
    if pos + 4 <= bytes.len() {
        let n = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        for _ in 0..n {
            if pos + 2 > bytes.len() { break; }
            let wlen = u16::from_le_bytes(bytes[pos..pos+2].try_into().unwrap()) as usize;
            pos += 2;
            if pos + wlen + 4 > bytes.len() { break; }
            let word = String::from_utf8_lossy(&bytes[pos..pos+wlen]).to_string();
            pos += wlen;
            let idf = f32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap());
            pos += 4;
            word_idf.insert(word, idf);
        }
    }

    // Original texts
    let mut doc_texts_original = Vec::new();
    if pos + 4 <= bytes.len() {
        let n = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        for _ in 0..n {
            if pos + 4 > bytes.len() { break; }
            let tlen = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + tlen > bytes.len() { break; }
            doc_texts_original.push(String::from_utf8_lossy(&bytes[pos..pos+tlen]).to_string());
            pos += tlen;
        }
    }

    Ok(ScaEngine {
        core,
        doc_texts_normalized,
        doc_texts_original,
        word_idf,
        brain: crate::brain::Brain::new(),
        #[cfg(feature = "static-embed")]
        static_encoder: None,
        #[cfg(feature = "gpu")]
        gpu_search: None,
    })
}

/// Deserialize slim SCA segment (no texts — caller provides texts from Frames).
fn deserialize_slim(bytes: &[u8]) -> Result<ScaEngine, String> {
    let mut pos = 4; // skip "SCS\0"

    if pos + 4 > bytes.len() { return Err("Truncated".to_string()); }
    let core_len = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;

    if pos + core_len > bytes.len() { return Err("Truncated core".to_string()); }
    let mut core = CrystallineCore::new();
    core.deserialize_index(&bytes[pos..pos+core_len])?;

    // No texts, no IDF — caller must call rebuild_from_texts()
    Ok(ScaEngine {
        core,
        doc_texts_normalized: Vec::new(),
        doc_texts_original: Vec::new(),
        word_idf: HashMap::new(),
        brain: crate::brain::Brain::new(),
        #[cfg(feature = "static-embed")]
        static_encoder: None,
        #[cfg(feature = "gpu")]
        gpu_search: None,
    })
}

/// Serialize portable .said file — breadcrumbs + brain + zstd-compressed text.
/// Self-contained: can be shared, moved, backed up.
///
/// Format v5: [SAID magic 4B] [version 2B=5] [flags 2B]
///            [breadcrumbs_len 4B] [breadcrumbs...]
///            [BRAN brain section...]
///            [num_texts 4B] [offset_table: num_texts × 8B (offset u32 + len u32)]
///            [uncompressed_len 4B] [compressed_len 4B]
///            [zstd_compressed_text_blob...]
///            [CRC32 4B]
///
/// Offsets are into the UNCOMPRESSED blob. On load: decompress first, then slice.
/// zstd L19 gives ~4x compression on English text, decompresses at 1500 MB/s.
pub fn serialize_portable(engine: &ScaEngine) -> Result<Vec<u8>, String> {
    let breadcrumbs = engine.core.serialize_breadcrumbs();
    if breadcrumbs.is_empty() {
        return Err("serialize_breadcrumbs returned empty".to_string());
    }

    let mut buf: Vec<u8> = Vec::new();

    // Header
    buf.extend_from_slice(b"SAID");
    buf.extend_from_slice(&5u16.to_le_bytes()); // version 5 = portable + zstd
    let flags: u16 = 0x10; // portable flag
    buf.extend_from_slice(&flags.to_le_bytes());

    // Breadcrumbs section
    buf.extend_from_slice(&(breadcrumbs.len() as u32).to_le_bytes());
    buf.extend_from_slice(&breadcrumbs);

    // Brain section
    let brain_bytes = engine.brain.serialize();
    buf.extend_from_slice(&brain_bytes);

    // Build flat text blob (uncompressed)
    let texts = &engine.doc_texts_original;
    let mut flat_blob: Vec<u8> = Vec::new();
    let mut offsets: Vec<(u32, u32)> = Vec::with_capacity(texts.len());
    let mut offset: u32 = 0;
    for text in texts {
        let len = text.len() as u32;
        offsets.push((offset, len));
        flat_blob.extend_from_slice(text.as_bytes());
        offset += len;
    }
    let uncompressed_len = flat_blob.len() as u32;

    // Compress with zstd level 19 (best ratio, decompress is still 1500 MB/s)
    let compressed = zstd::bulk::compress(&flat_blob, 19)
        .map_err(|e| format!("zstd compress failed: {}", e))?;
    let compressed_len = compressed.len() as u32;

    // Write offset table (into UNCOMPRESSED blob)
    buf.extend_from_slice(&(texts.len() as u32).to_le_bytes());
    for (off, len) in &offsets {
        buf.extend_from_slice(&off.to_le_bytes());
        buf.extend_from_slice(&len.to_le_bytes());
    }

    // Write compressed blob with sizes
    buf.extend_from_slice(&uncompressed_len.to_le_bytes());
    buf.extend_from_slice(&compressed_len.to_le_bytes());
    buf.extend_from_slice(&compressed);

    // CRC32
    let crc = crc32_simple(&buf);
    buf.extend_from_slice(&crc.to_le_bytes());

    Ok(buf)
}

/// Deserialize portable .said file (v4 uncompressed or v5 zstd-compressed).
fn deserialize_portable(bytes: &[u8]) -> Result<ScaEngine, String> {
    if bytes.len() < 12 { return Err("Portable file too small".to_string()); }

    // Verify CRC32 (last 4 bytes)
    if bytes.len() >= 4 {
        let stored_crc = u32::from_le_bytes(bytes[bytes.len()-4..].try_into().unwrap());
        let computed_crc = crc32_simple(&bytes[..bytes.len()-4]);
        if stored_crc != computed_crc {
            return Err("Portable .said file corrupted (CRC mismatch)".to_string());
        }
    }

    let _version = u16::from_le_bytes([bytes[4], bytes[5]]); // v5
    let _flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    let mut pos = 8;

    // Breadcrumbs
    let bc_len = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;
    if pos + bc_len > bytes.len() { return Err("Truncated breadcrumbs".to_string()); }
    let mut core = CrystallineCore::new();
    core.deserialize_breadcrumbs(&bytes[pos..pos+bc_len])?;
    pos += bc_len;

    // Brain section (BRAN magic)
    let brain = if let Some(brain_pos) = bytes[pos..].windows(4).position(|w| w == b"BRAN") {
        let abs_pos = pos + brain_pos;
        crate::brain::Brain::deserialize(&bytes[abs_pos..])
            .unwrap_or_else(|_| crate::brain::Brain::new())
    } else {
        crate::brain::Brain::new()
    };

    // Skip past BRAN section to text section
    if let Some(brain_pos) = bytes[pos..].windows(4).position(|w| w == b"BRAN") {
        pos += brain_pos + 4; // skip magic
        // Skip log entries
        if pos + 4 <= bytes.len() {
            let n_log = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;
            for _ in 0..n_log {
                pos += 8; // query_hash
                if pos + 2 > bytes.len() { break; }
                let dlen = u16::from_le_bytes([bytes[pos], bytes[pos+1]]) as usize;
                pos += 2 + dlen + 4 + 8; // doc_id + score + timestamp
            }
        }
        // Skip doc recall entries
        if pos + 4 <= bytes.len() {
            let n_docs = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;
            for _ in 0..n_docs {
                if pos + 2 > bytes.len() { break; }
                let dlen = u16::from_le_bytes([bytes[pos], bytes[pos+1]]) as usize;
                pos += 2 + dlen + 4 + 4 + 8; // doc_id + count + weight + last_recalled
            }
        }
        if pos + 4 <= bytes.len() { pos += 4; } // consolidation_cycles
        // Skip cross-timescale accumulator
        if pos + 12 <= bytes.len() {
            pos += 8; // query_emb_count
            if pos + 4 <= bytes.len() {
                let emb_dim = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
                pos += 4 + emb_dim * 8;
            }
        }
    }

    // Text section: offset table + blob
    if pos + 4 > bytes.len() { return Err("Truncated text header".to_string()); }
    let num_texts = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;

    // Read offset table
    let table_size = num_texts * 8;
    if pos + table_size > bytes.len() { return Err("Truncated offset table".to_string()); }
    let mut offsets: Vec<(u32, u32)> = Vec::with_capacity(num_texts);
    for _ in 0..num_texts {
        let off = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap());
        pos += 4;
        let len = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap());
        pos += 4;
        offsets.push((off, len));
    }

    // Decompress zstd-compressed text blob
    if pos + 8 > bytes.len() { return Err("Truncated compressed header".to_string()); }
    let uncompressed_len = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;
    let compressed_len = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;
    if pos + compressed_len > bytes.len() { return Err("Truncated compressed blob".to_string()); }
    let decompressed_blob = zstd::bulk::decompress(&bytes[pos..pos+compressed_len], uncompressed_len)
        .map_err(|e| format!("zstd decompress failed: {}", e))?;

    // Slice texts from decompressed blob
    let mut doc_texts_original: Vec<String> = Vec::with_capacity(num_texts);
    for &(off, len) in &offsets {
        let start = off as usize;
        let end = start + len as usize;
        if end > decompressed_blob.len() {
            return Err("Text offset exceeds blob".to_string());
        }
        doc_texts_original.push(String::from_utf8_lossy(&decompressed_blob[start..end]).to_string());
    }

    let mut engine = ScaEngine {
        core,
        doc_texts_normalized: Vec::new(),
        doc_texts_original: doc_texts_original.clone(),
        word_idf: HashMap::new(),
        brain,
        #[cfg(feature = "static-embed")]
        static_encoder: None,
        #[cfg(feature = "gpu")]
        gpu_search: None,
    };

    engine.rebuild_entity_data(&doc_texts_original);

    Ok(engine)
}

/// Simple CRC32 (IEEE polynomial) — no external dependency needed.
pub fn crc32_simple(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Find a 4-byte magic marker in a byte slice.
fn find_magic(data: &[u8], magic: &[u8; 4]) -> Option<usize> {
    data.windows(4).position(|w| w == magic)
}

fn deserialize_legacy(bytes: &[u8]) -> Result<ScaEngine, String> {
    let mut core = CrystallineCore::new();
    core.deserialize_index(bytes)?;

    let mut doc_texts_normalized = Vec::new();
    let mut word_doc_freq: HashMap<String, f32> = HashMap::new();

    for i in 0..core.get_doc_ids().len() {
        if let Some(text) = core.get_doc_text_by_index(i) {
            let normalized = text
                .replace('\u{2013}', "-").replace('\u{2014}', "-")
                .replace('\u{2018}', "'").replace('\u{2019}', "'")
                .to_lowercase();
            doc_texts_normalized.push(normalized);
            let words: std::collections::HashSet<String> = text
                .split_whitespace().map(|w| w.to_lowercase())
                .filter(|w| w.len() >= 3).collect();
            for w in words { *word_doc_freq.entry(w).or_insert(0.0) += 1.0; }
        } else {
            doc_texts_normalized.push(String::new());
        }
    }

    let n = doc_texts_normalized.len() as f32;
    let word_idf = word_doc_freq.into_iter()
        .map(|(w, df)| (w, ((n + 1.0) / (df + 1.0)).ln() + 1.0))
        .collect();

    Ok(ScaEngine {
        core,
        doc_texts_normalized,
        doc_texts_original: Vec::new(),
        word_idf,
        brain: crate::brain::Brain::new(),
        #[cfg(feature = "static-embed")]
        static_encoder: None,
        #[cfg(feature = "gpu")]
        gpu_search: None,
    })
}

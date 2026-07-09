//! Trigram inverted index for fast substring search — classic Russ Cox /
//! Google Code Search / Sourcegraph Zoekt design.
//!
//! ## How it works
//!
//! For each frame, extract all unique 3-character substrings ("trigrams")
//! from its lowercased content. Build an inverted map:
//!
//! ```text
//! trigram -> sorted list of frame_ids that contain it
//! ```
//!
//! At query time, decompose the query into trigrams, look up each posting
//! list, intersect them (sorted merge) — the result is a tiny candidate set
//! that can possibly match the query. Verify candidates with a real substring
//! check.
//!
//! ## Query example
//!
//! ```text
//! query = "compact_block"
//! trigrams = ["com","omp","mpa","pac","act","ct_","t_b","_bl","blo","loc","ock"]
//!
//! postings["com"] = [42, 187, 503, 2100, ...]
//! postings["omp"] = [42, 187, 503, 2101, ...]
//! postings["mpa"] = [42, 503, 2100, ...]
//! ...
//! intersection = [42, 503]   (tiny candidate set)
//! verify: does frame 42 or 503 actually contain "compact_block"? -> yes/no
//! ```
//!
//! ## Encoding
//!
//! - Posting lists are sorted frame_ids, delta-encoded, then varint-packed.
//! - Trigram table is a sorted array of (trigram_bytes, offset, len) entries.
//! - Everything is zstd-compressed on disk.
//!
//! ## Storage cost
//!
//! For 27K frames of ~5KB average content, ~81M postings after dedup,
//! varint+delta compresses to roughly 30-60MB. This is a one-time cost per
//! compact() — query time drops from O(n × avg_frame_size) to O(k × log n)
//! where k is the number of query trigrams (~10-20 for typical queries).

use std::collections::{BTreeMap, HashSet};

/// One query trigram is 3 bytes of lowercased content.
pub type Trigram = [u8; 3];

// ─────────────────────────────────────────────────────────────────────────
// Varint (LEB128-style) — inline implementation to avoid a new dependency
// ─────────────────────────────────────────────────────────────────────────

fn varint_write(buf: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        buf.push((v as u8) | 0x80);
        v >>= 7;
    }
    buf.push(v as u8);
}

fn varint_read(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    while *pos < buf.len() {
        let b = buf[*pos];
        *pos += 1;
        result |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
        if shift >= 64 { return None; }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────
// TrigramIndex
// ─────────────────────────────────────────────────────────────────────────

/// In-memory trigram inverted index.
///
/// Build order: `TrigramIndex::new()` → `add_frame(frame_id, content)` for
/// each active frame → `finalize()` to sort posting lists → `serialize()`.
pub struct TrigramIndex {
    /// trigram → set of frame_ids (use HashSet during build for dedup,
    /// convert to sorted Vec at finalize time).
    postings: BTreeMap<Trigram, HashSet<u32>>,
    finalized_postings: BTreeMap<Trigram, Vec<u32>>,
    is_finalized: bool,
}

impl TrigramIndex {
    pub fn new() -> Self {
        Self {
            postings: BTreeMap::new(),
            finalized_postings: BTreeMap::new(),
            is_finalized: false,
        }
    }

    /// Extract all unique lowercased trigrams from `content` and add them to
    /// `frame_id`'s posting list entries.
    pub fn add_frame(&mut self, frame_id: u32, content: &str) {
        // Lowercase once. Ignore trigrams that span non-ASCII — we keep the
        // scheme simple and byte-oriented. Non-ASCII chars split into UTF-8
        // bytes which still get trigrammed, just less semantically.
        let lower = content.to_lowercase();
        let bytes = lower.as_bytes();
        if bytes.len() < 3 { return; }

        // Per-frame dedup via HashSet — avoid inserting the same (trigram,
        // frame_id) pair twice when a trigram repeats inside the frame.
        let mut seen_in_frame: HashSet<Trigram> = HashSet::new();
        for window in bytes.windows(3) {
            let tg: Trigram = [window[0], window[1], window[2]];
            if seen_in_frame.insert(tg) {
                self.postings.entry(tg).or_insert_with(HashSet::new).insert(frame_id);
            }
        }
    }

    /// Convert posting HashSets into sorted Vecs. Called once after all
    /// frames are added, before serialization or querying.
    pub fn finalize(&mut self) {
        if self.is_finalized { return; }
        let postings = std::mem::take(&mut self.postings);
        for (tg, set) in postings {
            let mut v: Vec<u32> = set.into_iter().collect();
            v.sort_unstable();
            self.finalized_postings.insert(tg, v);
        }
        self.is_finalized = true;
    }

    /// Number of distinct trigrams in the index.
    pub fn num_trigrams(&self) -> usize {
        if self.is_finalized {
            self.finalized_postings.len()
        } else {
            self.postings.len()
        }
    }

    /// Total postings across all trigrams (for stats).
    pub fn total_postings(&self) -> usize {
        if self.is_finalized {
            self.finalized_postings.values().map(|v| v.len()).sum()
        } else {
            self.postings.values().map(|s| s.len()).sum()
        }
    }

    /// Approximate heap bytes held (postings × 4 bytes/u32 + per-trigram overhead).
    /// Diagnostic for the #4 memory picture.
    pub fn approx_bytes(&self) -> usize {
        let n_keys = if self.is_finalized { self.finalized_postings.len() } else { self.postings.len() };
        self.total_postings() * 4 + n_keys * 48
    }

    /// Look up a single trigram's posting list. Returns empty if unknown.
    pub fn lookup(&self, tg: &Trigram) -> &[u32] {
        debug_assert!(self.is_finalized, "lookup called before finalize");
        self.finalized_postings
            .get(tg)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Extract query trigrams from a pattern string (lowercased).
    pub fn query_trigrams(pattern: &str) -> Vec<Trigram> {
        let lower = pattern.to_lowercase();
        let bytes = lower.as_bytes();
        if bytes.len() < 3 { return Vec::new(); }
        let mut out: Vec<Trigram> = bytes.windows(3)
            .map(|w| [w[0], w[1], w[2]])
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Given a lowercased query pattern, return the sorted frame_ids that
    /// could possibly contain the pattern (AND-intersection of all query
    /// trigram posting lists).
    ///
    /// If the query is too short (< 3 chars), returns None to signal the
    /// caller should fall back to full scan.
    pub fn candidates(&self, pattern: &str) -> Option<Vec<u32>> {
        debug_assert!(self.is_finalized, "candidates called before finalize");
        let trigrams = Self::query_trigrams(pattern);
        if trigrams.is_empty() {
            return None; // caller should fall back
        }
        // Look up all posting lists, fail fast if any is empty (AND = none)
        let mut lists: Vec<&[u32]> = Vec::with_capacity(trigrams.len());
        for tg in &trigrams {
            let list = self.lookup(tg);
            if list.is_empty() {
                return Some(Vec::new()); // definite no match
            }
            lists.push(list);
        }
        // Sort by length ascending — intersection with shortest list first is faster
        lists.sort_by_key(|l| l.len());
        let mut result: Vec<u32> = lists[0].to_vec();
        for list in &lists[1..] {
            result = intersect_sorted(&result, list);
            if result.is_empty() { break; }
        }
        Some(result)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Serialization
    // ─────────────────────────────────────────────────────────────────────

    /// Serialize to a byte buffer. Layout:
    ///
    /// ```text
    /// u8        version (= 1)
    /// u8        reserved (0)
    /// u16       reserved (0)
    /// u32       num_trigrams
    /// u32       postings_raw_len
    /// table:    num_trigrams × (3 bytes trigram + u32 offset + u32 len)
    /// postings: packed delta-varint posting lists, concatenated
    /// ```
    ///
    /// The whole thing gets zstd-compressed by the caller before writing.
    pub fn serialize_raw(&self) -> Vec<u8> {
        debug_assert!(self.is_finalized, "serialize called before finalize");
        let num_trigrams = self.finalized_postings.len();

        // First pass: encode postings, collect (trigram, offset, len) table
        let mut postings_data: Vec<u8> = Vec::new();
        let mut table: Vec<(Trigram, u32, u32)> = Vec::with_capacity(num_trigrams);
        for (tg, list) in &self.finalized_postings {
            let start = postings_data.len() as u32;
            // Delta-encode: first value, then successive gaps
            let mut prev: u32 = 0;
            for &fid in list {
                let delta = fid - prev;
                varint_write(&mut postings_data, delta as u64);
                prev = fid;
            }
            let len = postings_data.len() as u32 - start;
            table.push((*tg, start, len));
        }

        // Header + table + postings
        let mut out: Vec<u8> = Vec::new();
        out.push(1u8); // version
        out.push(0u8); // reserved
        out.extend_from_slice(&0u16.to_le_bytes()); // reserved
        out.extend_from_slice(&(num_trigrams as u32).to_le_bytes());
        out.extend_from_slice(&(postings_data.len() as u32).to_le_bytes());
        for (tg, offset, len) in &table {
            out.extend_from_slice(tg);
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
        }
        out.extend_from_slice(&postings_data);
        out
    }

    /// Deserialize from a raw buffer produced by `serialize_raw`.
    pub fn deserialize_raw(data: &[u8]) -> Result<Self, String> {
        if data.len() < 12 {
            return Err("TRGM: buffer too short".into());
        }
        let version = data[0];
        if version != 1 {
            return Err(format!("TRGM: unsupported version {}", version));
        }
        let num_trigrams = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let postings_len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;

        let table_size = num_trigrams * 11; // 3 bytes trigram + 4 offset + 4 len
        let table_start = 12;
        let table_end = table_start + table_size;
        let postings_start = table_end;
        let postings_end = postings_start + postings_len;
        if postings_end > data.len() {
            return Err("TRGM: truncated postings region".into());
        }

        let mut finalized: BTreeMap<Trigram, Vec<u32>> = BTreeMap::new();
        let table = &data[table_start..table_end];
        let postings = &data[postings_start..postings_end];

        for i in 0..num_trigrams {
            let entry = &table[i * 11..(i + 1) * 11];
            let tg: Trigram = [entry[0], entry[1], entry[2]];
            let offset = u32::from_le_bytes(entry[3..7].try_into().unwrap()) as usize;
            let len = u32::from_le_bytes(entry[7..11].try_into().unwrap()) as usize;
            let slice = &postings[offset..offset + len];

            // Decode delta-varint
            let mut list: Vec<u32> = Vec::new();
            let mut pos = 0;
            let mut prev: u32 = 0;
            while pos < slice.len() {
                let delta = varint_read(slice, &mut pos)
                    .ok_or_else(|| "TRGM: varint decode failed".to_string())?
                    as u32;
                prev += delta;
                list.push(prev);
            }
            finalized.insert(tg, list);
        }

        Ok(Self {
            postings: BTreeMap::new(),
            finalized_postings: finalized,
            is_finalized: true,
        })
    }
}

/// Sorted-list intersection: returns elements present in BOTH `a` and `b`.
/// Both inputs must be sorted ascending, no duplicates.
fn intersect_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_trigrams_basic() {
        let tgs = TrigramIndex::query_trigrams("compact");
        // "compact" -> com, omp, mpa, pac, act (sorted + deduped)
        let expected = ["act", "com", "mpa", "omp", "pac"];
        let expected_bytes: Vec<Trigram> = expected.iter()
            .map(|s| { let b = s.as_bytes(); [b[0], b[1], b[2]] })
            .collect();
        assert_eq!(tgs, expected_bytes);
    }

    #[test]
    fn query_short_is_empty() {
        assert!(TrigramIndex::query_trigrams("co").is_empty());
        assert!(TrigramIndex::query_trigrams("a").is_empty());
    }

    #[test]
    fn build_and_query_roundtrip() {
        let mut idx = TrigramIndex::new();
        idx.add_frame(0, "fn compact_block_dict(&mut self) { }");
        idx.add_frame(1, "struct CompressedBlock { id: u32 }");
        idx.add_frame(2, "fn unrelated_function() -> i32 { 42 }");
        idx.finalize();

        // Query "compact" -> frame 0 only
        let cands = idx.candidates("compact").unwrap();
        assert_eq!(cands, vec![0]);

        // Query "block" -> frames 0 and 1
        let cands = idx.candidates("block").unwrap();
        assert_eq!(cands, vec![0, 1]);

        // Query "function" -> frame 2 only (lowercase form)
        let cands = idx.candidates("function").unwrap();
        assert_eq!(cands, vec![2]);

        // Query "xyz123" (not in any frame) -> empty
        let cands = idx.candidates("xyz123").unwrap();
        assert!(cands.is_empty());
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let mut idx = TrigramIndex::new();
        idx.add_frame(0, "the quick brown fox");
        idx.add_frame(1, "jumps over the lazy dog");
        idx.add_frame(2, "the quick red fox");
        idx.finalize();

        let raw = idx.serialize_raw();
        let idx2 = TrigramIndex::deserialize_raw(&raw).unwrap();

        // Same number of trigrams
        assert_eq!(idx.num_trigrams(), idx2.num_trigrams());

        // Same query results
        let c1 = idx.candidates("quick").unwrap();
        let c2 = idx2.candidates("quick").unwrap();
        assert_eq!(c1, c2);
        assert_eq!(c1, vec![0, 2]);

        let c3 = idx2.candidates("lazy").unwrap();
        assert_eq!(c3, vec![1]);
    }

    #[test]
    fn case_insensitive() {
        let mut idx = TrigramIndex::new();
        idx.add_frame(0, "CompactBlockDict");
        idx.finalize();

        // All case variants should find frame 0
        assert_eq!(idx.candidates("compact").unwrap(), vec![0]);
        assert_eq!(idx.candidates("COMPACT").unwrap(), vec![0]);
        assert_eq!(idx.candidates("Compact").unwrap(), vec![0]);
    }
}

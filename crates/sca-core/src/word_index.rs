//! WIDX — the on-disk / mmap-backed word index (SPIMI pattern).
//!
//! The resident BM25 word index in `CrystallineCore` (word_inverted_fast, doc_word_sets_fast,
//! doc_word_tf_fast, phonetic_index_fast + the word_vocab/word_to_id interning) holds ~70 KB/doc in
//! RAM — ~2.6 GB at 37k docs, which blows the 580 MB constant-memory ceiling and OOM-crashes on a
//! real bank corpus. This module serializes those structures into ONE compact section (varint-delta
//! encoded postings, following the SYMS/TRGM serialized-section pattern) that is written into the
//! `.said` file and READ IN PLACE from the mmap'd bytes at query time — so RAM stays bounded no
//! matter the corpus size.
//!
//! This file is built in TDD increments. Step 1 (here): the serialize/deserialize round-trip for the
//! in-RAM form — the byte format the mmap accessor (step 2) will later read in place. Every posting
//! list is stored SORTED so it can be delta-encoded and later binary-searched / intersected on disk.

/// Varint (LEB128, unsigned) — append `v` to `out`.
fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// Read a uvarint from `data` at `*pos`, advancing `*pos`. Returns None on truncation/overflow.
fn get_uvarint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if *pos >= data.len() || shift >= 64 {
            return None;
        }
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
    }
}

/// Encode a sorted slice of u64 as [count][delta-varints]. Non-sorted input is sorted first by the
/// caller (we assert sortedness cheaply in debug).
fn put_sorted_deltas(out: &mut Vec<u8>, sorted: &[u64]) {
    put_uvarint(out, sorted.len() as u64);
    let mut prev = 0u64;
    for &v in sorted {
        debug_assert!(v >= prev, "put_sorted_deltas requires sorted input");
        put_uvarint(out, v - prev);
        prev = v;
    }
}

/// Decode a [count][delta-varints] list written by `put_sorted_deltas`.
fn get_sorted_deltas(data: &[u8], pos: &mut usize) -> Option<Vec<u64>> {
    let n = get_uvarint(data, pos)? as usize;
    let mut out = Vec::with_capacity(n);
    let mut prev = 0u64;
    for _ in 0..n {
        let d = get_uvarint(data, pos)?;
        prev += d;
        out.push(prev);
    }
    Some(out)
}

/// The word index in a serialization-friendly form. Postings are sorted `Vec`s (not `HashSet`s) so
/// the byte layout is deterministic + delta-encodable and the mmap reader (step 2) can binary-search.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WordIndex {
    /// word-id → word string (word_vocab). Index in the Vec IS the word-id.
    pub vocab: Vec<String>,
    /// per-doc sorted set of word-ids (doc_word_sets_fast).
    pub doc_word_sets: Vec<Vec<u32>>,
    /// per-doc sorted (word-id, tf) pairs (doc_word_tf_fast).
    pub doc_word_tf: Vec<Vec<(u32, u32)>>,
    /// word-id → sorted doc indices (word_inverted_fast).
    pub word_inverted: Vec<(u32, Vec<u32>)>,
    /// soundex → sorted word-ids (phonetic_index_fast).
    pub phonetic: Vec<(String, Vec<u32>)>,
    /// word → IDF weight (word_idf), stored VERBATIM so a cold `said ask` loads the exact IDF the
    /// ingest computed — no re-tokenising 37k docs at query time (that rebuild was the ~3.7 s cold-CLI
    /// cost). Empty on v1 files → the reader falls back to rebuild. Version-gated (v2).
    pub word_idf: Vec<(String, f32)>,
}

/// v1: vocab + postings. v2: also carries `word_idf` (so cold query skips the rebuild). A v2 reader
/// reads v1 files fine (word_idf stays empty → rebuild fallback); a v1 reader stops after phonetic.
const WIDX_VERSION: u8 = 2;

impl WordIndex {
    /// Serialize to a raw byte buffer (pre-zstd). Layout is length-prefixed throughout so the mmap
    /// reader can locate any sub-table by walking offsets. All posting lists are sorted+delta-varint.
    pub fn serialize_raw(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(WIDX_VERSION);
        out.push(0); // reserved
        out.extend_from_slice(&0u16.to_le_bytes()); // reserved

        // vocab
        put_uvarint(&mut out, self.vocab.len() as u64);
        for w in &self.vocab {
            let b = w.as_bytes();
            put_uvarint(&mut out, b.len() as u64);
            out.extend_from_slice(b);
        }

        // doc_word_sets
        put_uvarint(&mut out, self.doc_word_sets.len() as u64);
        for set in &self.doc_word_sets {
            let sorted: Vec<u64> = set.iter().map(|&x| x as u64).collect();
            put_sorted_deltas(&mut out, &sorted);
        }

        // doc_word_tf — store (word-id delta, tf); word-ids sorted so delta works.
        put_uvarint(&mut out, self.doc_word_tf.len() as u64);
        for tf in &self.doc_word_tf {
            put_uvarint(&mut out, tf.len() as u64);
            let mut prev = 0u32;
            for &(wid, count) in tf {
                debug_assert!(wid >= prev, "doc_word_tf must be sorted by word-id");
                put_uvarint(&mut out, (wid - prev) as u64);
                put_uvarint(&mut out, count as u64);
                prev = wid;
            }
        }

        // word_inverted — (word-id, sorted doc indices). word-ids sorted so we delta them too.
        put_uvarint(&mut out, self.word_inverted.len() as u64);
        let mut prev_wid = 0u32;
        for (wid, docs) in &self.word_inverted {
            debug_assert!(*wid >= prev_wid, "word_inverted must be sorted by word-id");
            put_uvarint(&mut out, (*wid - prev_wid) as u64);
            let sorted: Vec<u64> = docs.iter().map(|&x| x as u64).collect();
            put_sorted_deltas(&mut out, &sorted);
            prev_wid = *wid;
        }

        // phonetic — (soundex string, sorted word-ids)
        put_uvarint(&mut out, self.phonetic.len() as u64);
        for (sx, wids) in &self.phonetic {
            let b = sx.as_bytes();
            put_uvarint(&mut out, b.len() as u64);
            out.extend_from_slice(b);
            let sorted: Vec<u64> = wids.iter().map(|&x| x as u64).collect();
            put_sorted_deltas(&mut out, &sorted);
        }

        // word_idf (v2) — (word string, f32 IDF), stored verbatim (LE bytes). Empty is fine.
        put_uvarint(&mut out, self.word_idf.len() as u64);
        for (w, idf) in &self.word_idf {
            let b = w.as_bytes();
            put_uvarint(&mut out, b.len() as u64);
            out.extend_from_slice(b);
            out.extend_from_slice(&idf.to_le_bytes());
        }

        out
    }

    /// Deserialize a buffer produced by `serialize_raw`.
    pub fn deserialize_raw(data: &[u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("WIDX: buffer too short".into());
        }
        let version = data[0];
        if version != 1 && version != 2 {
            return Err(format!("WIDX: unsupported version {}", version));
        }
        let mut pos = 4usize;

        let vocab_n = get_uvarint(data, &mut pos).ok_or("WIDX: vocab count")? as usize;
        let mut vocab = Vec::with_capacity(vocab_n);
        for _ in 0..vocab_n {
            let len = get_uvarint(data, &mut pos).ok_or("WIDX: vocab word len")? as usize;
            if pos + len > data.len() {
                return Err("WIDX: truncated vocab word".into());
            }
            vocab.push(String::from_utf8_lossy(&data[pos..pos + len]).into_owned());
            pos += len;
        }

        let dws_n = get_uvarint(data, &mut pos).ok_or("WIDX: doc_word_sets count")? as usize;
        let mut doc_word_sets = Vec::with_capacity(dws_n);
        for _ in 0..dws_n {
            let v = get_sorted_deltas(data, &mut pos).ok_or("WIDX: doc_word_set")?;
            doc_word_sets.push(v.into_iter().map(|x| x as u32).collect());
        }

        let dtf_n = get_uvarint(data, &mut pos).ok_or("WIDX: doc_word_tf count")? as usize;
        let mut doc_word_tf = Vec::with_capacity(dtf_n);
        for _ in 0..dtf_n {
            let entries = get_uvarint(data, &mut pos).ok_or("WIDX: tf entries")? as usize;
            let mut v = Vec::with_capacity(entries);
            let mut prev = 0u32;
            for _ in 0..entries {
                let d = get_uvarint(data, &mut pos).ok_or("WIDX: tf wid delta")? as u32;
                let count = get_uvarint(data, &mut pos).ok_or("WIDX: tf count")? as u32;
                prev += d;
                v.push((prev, count));
            }
            doc_word_tf.push(v);
        }

        let wi_n = get_uvarint(data, &mut pos).ok_or("WIDX: word_inverted count")? as usize;
        let mut word_inverted = Vec::with_capacity(wi_n);
        let mut prev_wid = 0u32;
        for _ in 0..wi_n {
            let d = get_uvarint(data, &mut pos).ok_or("WIDX: wi wid delta")? as u32;
            prev_wid += d;
            let docs = get_sorted_deltas(data, &mut pos).ok_or("WIDX: wi docs")?;
            word_inverted.push((prev_wid, docs.into_iter().map(|x| x as u32).collect()));
        }

        let ph_n = get_uvarint(data, &mut pos).ok_or("WIDX: phonetic count")? as usize;
        let mut phonetic = Vec::with_capacity(ph_n);
        for _ in 0..ph_n {
            let len = get_uvarint(data, &mut pos).ok_or("WIDX: soundex len")? as usize;
            if pos + len > data.len() {
                return Err("WIDX: truncated soundex".into());
            }
            let sx = String::from_utf8_lossy(&data[pos..pos + len]).into_owned();
            pos += len;
            let wids = get_sorted_deltas(data, &mut pos).ok_or("WIDX: phonetic wids")?;
            phonetic.push((sx, wids.into_iter().map(|x| x as u32).collect()));
        }

        // word_idf (v2 only). v1 files stop here → word_idf empty → rebuild fallback.
        let mut word_idf = Vec::new();
        if version >= 2 {
            let n = get_uvarint(data, &mut pos).ok_or("WIDX: idf count")? as usize;
            word_idf.reserve(n);
            for _ in 0..n {
                let len = get_uvarint(data, &mut pos).ok_or("WIDX: idf word len")? as usize;
                if pos + len > data.len() { return Err("WIDX: truncated idf word".into()); }
                let w = String::from_utf8_lossy(&data[pos..pos + len]).into_owned();
                pos += len;
                if pos + 4 > data.len() { return Err("WIDX: truncated idf value".into()); }
                let idf = f32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                pos += 4;
                word_idf.push((w, idf));
            }
        }

        Ok(WordIndex { vocab, doc_word_sets, doc_word_tf, word_inverted, phonetic, word_idf })
    }
}

/// In-place reader over serialized WIDX bytes (the mmap accessor — SPIMI read side).
///
/// Borrows the serialized blob (`&[u8]`, backed by the mmap in production) and, in ONE scan, builds
/// small offset directories (a byte offset per doc / per word-id / per soundex — kilobytes, not the
/// gigabytes the resident HashMaps cost). Each lookup then decodes exactly one posting list in place
/// from the borrowed bytes: RAM stays bounded regardless of corpus size. Returns the SAME lists as
/// the in-RAM `WordIndex`, so recall is bit-identical.
pub struct WidxReader<'a> {
    data: &'a [u8],
    /// byte offset of each doc's word-set list (index = doc idx).
    doc_word_set_off: Vec<usize>,
    /// byte offset of each doc's tf list (index = doc idx).
    doc_word_tf_off: Vec<usize>,
    /// (word-id, byte offset of its doc-postings) sorted by word-id for binary search.
    word_inverted_off: Vec<(u32, usize)>,
    /// (soundex, byte offset of its word-id list).
    phonetic_off: Vec<(String, usize)>,
    /// vocab strings (small relative to postings; kept resident for word_of/word_id_of).
    vocab: Vec<String>,
}

impl<'a> WidxReader<'a> {
    /// Build the offset directory in one scan over the serialized bytes. Does NOT materialize any
    /// posting list.
    pub fn new(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("WIDX: buffer too short".into());
        }
        if data[0] != WIDX_VERSION {
            return Err(format!("WIDX: unsupported version {}", data[0]));
        }
        let mut pos = 4usize;

        // vocab (kept resident — strings, needed for word<->id)
        let vocab_n = get_uvarint(data, &mut pos).ok_or("WIDX: vocab count")? as usize;
        let mut vocab = Vec::with_capacity(vocab_n);
        for _ in 0..vocab_n {
            let len = get_uvarint(data, &mut pos).ok_or("WIDX: vocab len")? as usize;
            if pos + len > data.len() { return Err("WIDX: truncated vocab".into()); }
            vocab.push(String::from_utf8_lossy(&data[pos..pos + len]).into_owned());
            pos += len;
        }

        // doc_word_sets — record each list's offset, then skip it.
        let dws_n = get_uvarint(data, &mut pos).ok_or("WIDX: dws count")? as usize;
        let mut doc_word_set_off = Vec::with_capacity(dws_n);
        for _ in 0..dws_n {
            doc_word_set_off.push(pos);
            skip_sorted_deltas(data, &mut pos).ok_or("WIDX: skip dws")?;
        }

        // doc_word_tf — record offset, skip (entries × (wid-delta, count)).
        let dtf_n = get_uvarint(data, &mut pos).ok_or("WIDX: dtf count")? as usize;
        let mut doc_word_tf_off = Vec::with_capacity(dtf_n);
        for _ in 0..dtf_n {
            doc_word_tf_off.push(pos);
            let entries = get_uvarint(data, &mut pos).ok_or("WIDX: tf entries")? as usize;
            for _ in 0..entries {
                get_uvarint(data, &mut pos).ok_or("WIDX: tf wid")?;
                get_uvarint(data, &mut pos).ok_or("WIDX: tf count")?;
            }
        }

        // word_inverted — decode the delta'd word-id (needed for the lookup key) + record posting off.
        let wi_n = get_uvarint(data, &mut pos).ok_or("WIDX: wi count")? as usize;
        let mut word_inverted_off = Vec::with_capacity(wi_n);
        let mut prev_wid = 0u32;
        for _ in 0..wi_n {
            let d = get_uvarint(data, &mut pos).ok_or("WIDX: wi wid delta")? as u32;
            prev_wid += d;
            word_inverted_off.push((prev_wid, pos));
            skip_sorted_deltas(data, &mut pos).ok_or("WIDX: skip wi docs")?;
        }

        // phonetic — record (soundex, off), skip the word-id list.
        let ph_n = get_uvarint(data, &mut pos).ok_or("WIDX: ph count")? as usize;
        let mut phonetic_off = Vec::with_capacity(ph_n);
        for _ in 0..ph_n {
            let len = get_uvarint(data, &mut pos).ok_or("WIDX: soundex len")? as usize;
            if pos + len > data.len() { return Err("WIDX: truncated soundex".into()); }
            let sx = String::from_utf8_lossy(&data[pos..pos + len]).into_owned();
            pos += len;
            phonetic_off.push((sx, pos));
            skip_sorted_deltas(data, &mut pos).ok_or("WIDX: skip ph wids")?;
        }

        Ok(WidxReader {
            data,
            doc_word_set_off,
            doc_word_tf_off,
            word_inverted_off,
            phonetic_off,
            vocab,
        })
    }

    pub fn vocab_len(&self) -> usize { self.vocab.len() }
    pub fn num_docs(&self) -> usize { self.doc_word_set_off.len() }

    /// word-id → word string.
    pub fn word_of(&self, wid: u32) -> Option<&str> {
        self.vocab.get(wid as usize).map(|s| s.as_str())
    }
    /// word string → word-id (linear scan of vocab; callers cache when hot).
    pub fn word_id_of(&self, word: &str) -> Option<u32> {
        self.vocab.iter().position(|w| w == word).map(|i| i as u32)
    }

    /// doc idx → sorted word-ids (decoded in place).
    pub fn doc_word_set(&self, doc_idx: usize) -> Option<Vec<u32>> {
        let mut pos = *self.doc_word_set_off.get(doc_idx)?;
        get_sorted_deltas(self.data, &mut pos).map(|v| v.into_iter().map(|x| x as u32).collect())
    }

    /// doc idx → sorted (word-id, tf).
    pub fn doc_word_tf(&self, doc_idx: usize) -> Option<Vec<(u32, u32)>> {
        let mut pos = *self.doc_word_tf_off.get(doc_idx)?;
        let entries = get_uvarint(self.data, &mut pos)? as usize;
        let mut out = Vec::with_capacity(entries);
        let mut prev = 0u32;
        for _ in 0..entries {
            let d = get_uvarint(self.data, &mut pos)? as u32;
            let count = get_uvarint(self.data, &mut pos)? as u32;
            prev += d;
            out.push((prev, count));
        }
        Some(out)
    }

    /// word-id → sorted doc indices (binary search the directory, decode in place).
    pub fn word_inverted(&self, wid: u32) -> Option<Vec<u32>> {
        let i = self.word_inverted_off.binary_search_by_key(&wid, |&(w, _)| w).ok()?;
        let mut pos = self.word_inverted_off[i].1;
        get_sorted_deltas(self.data, &mut pos).map(|v| v.into_iter().map(|x| x as u32).collect())
    }

    /// soundex → sorted word-ids.
    pub fn phonetic(&self, soundex: &str) -> Option<Vec<u32>> {
        let (_, off) = self.phonetic_off.iter().find(|(s, _)| s == soundex)?;
        let mut pos = *off;
        get_sorted_deltas(self.data, &mut pos).map(|v| v.into_iter().map(|x| x as u32).collect())
    }
}

/// Skip a [count][delta-varints] list without decoding it (advance `*pos`).
fn skip_sorted_deltas(data: &[u8], pos: &mut usize) -> Option<()> {
    let n = get_uvarint(data, pos)? as usize;
    for _ in 0..n {
        get_uvarint(data, pos)?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WordIndex {
        WordIndex {
            vocab: vec!["loan".into(), "account".into(), "ledger".into()],
            doc_word_sets: vec![vec![0, 2], vec![1], vec![0, 1, 2]],
            doc_word_tf: vec![vec![(0, 3), (2, 1)], vec![(1, 5)], vec![(0, 1), (1, 1), (2, 4)]],
            word_inverted: vec![(0, vec![0, 2]), (1, vec![1, 2]), (2, vec![0, 2])],
            phonetic: vec![("L500".into(), vec![0]), ("A253".into(), vec![1])],
            word_idf: vec![("account".into(), 1.5), ("ledger".into(), 2.0), ("loan".into(), 1.25)],
        }
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let wi = sample();
        let bytes = wi.serialize_raw();
        let back = WordIndex::deserialize_raw(&bytes).expect("deserialize");
        assert_eq!(wi, back, "WIDX round-trip must be bit-identical");
    }

    #[test]
    fn empty_roundtrips() {
        let wi = WordIndex::default();
        let bytes = wi.serialize_raw();
        let back = WordIndex::deserialize_raw(&bytes).expect("deserialize empty");
        assert_eq!(wi, back);
    }

    #[test]
    fn rejects_bad_version() {
        let mut bytes = sample().serialize_raw();
        bytes[0] = 99;
        assert!(WordIndex::deserialize_raw(&bytes).is_err());
    }

    #[test]
    fn mmap_reader_matches_in_ram() {
        let wi = sample();
        let bytes = wi.serialize_raw();
        let r = WidxReader::new(&bytes).expect("reader");

        assert_eq!(r.vocab_len(), wi.vocab.len());
        assert_eq!(r.num_docs(), wi.doc_word_sets.len());

        // vocab <-> id
        for (i, w) in wi.vocab.iter().enumerate() {
            assert_eq!(r.word_of(i as u32), Some(w.as_str()));
            assert_eq!(r.word_id_of(w), Some(i as u32));
        }
        assert_eq!(r.word_id_of("nonexistent"), None);

        // per-doc word sets + tf — identical to the in-RAM lists
        for (d, expect) in wi.doc_word_sets.iter().enumerate() {
            assert_eq!(r.doc_word_set(d).as_ref(), Some(expect), "doc_word_set doc {}", d);
        }
        for (d, expect) in wi.doc_word_tf.iter().enumerate() {
            assert_eq!(r.doc_word_tf(d).as_ref(), Some(expect), "doc_word_tf doc {}", d);
        }

        // inverted postings by word-id (binary search)
        for (wid, docs) in &wi.word_inverted {
            assert_eq!(r.word_inverted(*wid).as_ref(), Some(docs), "word_inverted wid {}", wid);
        }
        assert_eq!(r.word_inverted(9999), None);

        // phonetic
        for (sx, wids) in &wi.phonetic {
            assert_eq!(r.phonetic(sx).as_ref(), Some(wids), "phonetic {}", sx);
        }
        assert_eq!(r.phonetic("ZZZZ"), None);
    }
}

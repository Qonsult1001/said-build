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
}

const WIDX_VERSION: u8 = 1;

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

        out
    }

    /// Deserialize a buffer produced by `serialize_raw`.
    pub fn deserialize_raw(data: &[u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("WIDX: buffer too short".into());
        }
        let version = data[0];
        if version != WIDX_VERSION {
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

        Ok(WordIndex { vocab, doc_word_sets, doc_word_tf, word_inverted, phonetic })
    }
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
}

//! Self-contained BERT WordPiece tokenizer (issue #4).
//!
//! Replaces HuggingFace `tokenizers` for INFERENCE. The HF `tokenizers` 0.21
//! `encode_batch_fast` allocates a fixed ~250MB transient on its FIRST call
//! (RSS 46MB→308MB on the first encode of even a 3-word string, then frees) —
//! the dominant `said init` encode-phase memory spike. This module produces
//! BYTE-IDENTICAL token IDs without that allocation.
//!
//! It re-implements, faithfully, the exact pipeline configured in the model's
//! `tokenizer.json` (traced from tokenizers 0.21.4):
//!   BertNormalizer{clean_text, handle_chinese_chars, strip_accents:null,
//!                  lowercase:true}
//!   → BertPreTokenizer
//!   → WordPiece{unk:"[UNK]", prefix:"##", max_input_chars_per_word:100}
//! plus added-token (special-token) extraction, with `add_special_tokens=false`
//! (no [CLS]/[SEP] injected).
//!
//! To guarantee identity we use the SAME unicode crates HF tokenizers uses:
//! `unicode_categories` (is_other / is_mark_nonspacing / is_punctuation) and
//! `unicode-normalization-alignments` (NFD). See the per-step comments for the
//! upstream source each rule mirrors.

use serde_json::Value;
use std::collections::HashMap;
use unicode_categories::UnicodeCategories;
use unicode_normalization_alignments::UnicodeNormalization;

/// A loaded WordPiece tokenizer: vocab + the few config knobs we need.
#[derive(Debug, Clone)]
pub struct WordPieceTokenizer {
    /// token string → id (the model `vocab` from tokenizer.json).
    vocab: HashMap<String, u32>,
    /// Added/special tokens with `normalized:false`, longest-first, matched on
    /// RAW text before normalization (mirrors HF AddedVocabulary extraction).
    added_tokens: Vec<(String, u32)>,
    /// id of "[UNK]" (unk_token), if present.
    unk_token_id: Option<u32>,
    /// "##" continuing-subword prefix.
    continuing_subword_prefix: String,
    /// 100 — words longer than this (in chars) collapse to a single [UNK].
    max_input_chars_per_word: usize,
    /// median of all vocab token BYTE-lengths (model2vec uses `String::len()`,
    /// which is bytes); used for the cheap char pre-truncation in `encode`.
    median_token_length: usize,
}

impl WordPieceTokenizer {
    /// Parse a `tokenizer.json` (the full HF serialized tokenizer).
    pub fn from_tokenizer_json(bytes: &[u8]) -> Result<Self, String> {
        let v: Value =
            serde_json::from_slice(bytes).map_err(|e| format!("parse tokenizer.json: {e}"))?;

        let model = v
            .get("model")
            .ok_or_else(|| "tokenizer.json: missing `model`".to_string())?;

        // --- vocab: token -> id -------------------------------------------------
        let vocab_obj = model
            .get("vocab")
            .and_then(Value::as_object)
            .ok_or_else(|| "tokenizer.json: missing model.vocab".to_string())?;
        let mut vocab: HashMap<String, u32> = HashMap::with_capacity(vocab_obj.len());
        for (k, val) in vocab_obj {
            let id = val
                .as_u64()
                .ok_or_else(|| format!("vocab id for {k:?} not an int"))?;
            vocab.insert(k.clone(), id as u32);
        }

        // --- WordPiece config (with the HF defaults if absent) -----------------
        let unk_token = model
            .get("unk_token")
            .and_then(Value::as_str)
            .unwrap_or("[UNK]")
            .to_string();
        let continuing_subword_prefix = model
            .get("continuing_subword_prefix")
            .and_then(Value::as_str)
            .unwrap_or("##")
            .to_string();
        let max_input_chars_per_word = model
            .get("max_input_chars_per_word")
            .and_then(Value::as_u64)
            .unwrap_or(100) as usize;
        let unk_token_id = vocab.get(unk_token.as_str()).copied();

        // --- added tokens (special tokens) -------------------------------------
        // HF AddedVocabulary extracts these from RAW text before normalization.
        // We only replicate the `normalized:false` ones (all 5 BERT specials are),
        // matched case-sensitively, longest content first (HF sorts by length so
        // longer tokens win over prefixes).
        let mut added_tokens: Vec<(String, u32)> = Vec::new();
        if let Some(arr) = v.get("added_tokens").and_then(Value::as_array) {
            for t in arr {
                let normalized = t.get("normalized").and_then(Value::as_bool).unwrap_or(false);
                if normalized {
                    // A `normalized:true` added token would participate in
                    // normalization; none of our models use them. Skip to avoid
                    // silently diverging — they'd need different handling.
                    continue;
                }
                let (Some(content), Some(id)) = (
                    t.get("content").and_then(Value::as_str),
                    t.get("id").and_then(Value::as_u64),
                ) else {
                    continue;
                };
                if content.is_empty() {
                    continue;
                }
                added_tokens.push((content.to_string(), id as u32));
            }
        }
        // Longest content first so e.g. "[MASK]" wins over any shorter prefix.
        added_tokens.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        // --- median token length (BYTE length, matching model2vec) -------------
        // model2vec computes this over `get_vocab(false)` = the model vocab keys.
        let mut lens: Vec<usize> = vocab.keys().map(|k| k.len()).collect();
        lens.sort_unstable();
        let median_token_length = lens.get(lens.len() / 2).copied().unwrap_or(1);

        Ok(Self {
            vocab,
            added_tokens,
            unk_token_id,
            continuing_subword_prefix,
            max_input_chars_per_word,
            median_token_length,
        })
    }

    /// id of [UNK], if any (model2vec filters these out after tokenizing).
    pub fn unk_token_id(&self) -> Option<u32> {
        self.unk_token_id
    }

    /// median vocab token byte-length (for char pre-truncation in `encode`).
    pub fn median_token_length(&self) -> usize {
        self.median_token_length
    }

    /// Full encode of a single text → token ids (with `add_special_tokens=false`,
    /// i.e. no [CLS]/[SEP]). Mirrors HF `Tokenizer::encode`:
    ///   added-token split → normalize → pre-tokenize → WordPiece per piece.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut ids: Vec<u32> = Vec::new();
        // 1. Split off added/special tokens on RAW text (HF does this first).
        for segment in self.split_added(text) {
            match segment {
                Segment::Added(id) => ids.push(id),
                Segment::Text(s) => {
                    // 2. Normalize (BertNormalizer).
                    let normalized = self.normalize(s);
                    // 3. Pre-tokenize (BertPreTokenizer) → words.
                    for word in pre_tokenize(&normalized) {
                        // 4. WordPiece each word.
                        self.wordpiece_into(word, &mut ids);
                    }
                }
            }
        }
        ids
    }

    // =========================================================================
    // ADDED-TOKEN SPLIT
    // =========================================================================

    /// Split `text` into runs of plain text and exact matches of added tokens.
    fn split_added<'a>(&self, text: &'a str) -> Vec<Segment<'a>> {
        if self.added_tokens.is_empty() {
            return vec![Segment::Text(text)];
        }
        let mut out: Vec<Segment> = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0usize;
        let mut last = 0usize;
        while i < bytes.len() {
            let mut matched = None;
            for (content, id) in &self.added_tokens {
                let cl = content.len();
                // byte-compare so we never slice across a char boundary.
                if i + cl <= bytes.len() && &bytes[i..i + cl] == content.as_bytes() {
                    matched = Some((cl, *id));
                    break; // added_tokens is longest-first → first match is longest
                }
            }
            if let Some((cl, id)) = matched {
                if last < i {
                    out.push(Segment::Text(&text[last..i]));
                }
                out.push(Segment::Added(id));
                i += cl;
                last = i;
            } else {
                // advance one full char
                let ch_len = text[i..].chars().next().map_or(1, |c| c.len_utf8());
                i += ch_len;
            }
        }
        if last < text.len() {
            out.push(Segment::Text(&text[last..]));
        }
        out
    }

    // =========================================================================
    // NORMALIZE — BertNormalizer (tokenizers/src/normalizers/bert.rs)
    // =========================================================================

    fn normalize(&self, text: &str) -> String {
        // clean_text: drop NUL / 0xFFFD / control; map whitespace → ' '.
        let mut s: String = text
            .chars()
            .filter(|&c| !(c as u32 == 0 || c as u32 == 0xfffd || is_control(c)))
            .map(|c| if is_whitespace(c) { ' ' } else { c })
            .collect();

        // handle_chinese_chars: wrap CJK chars with spaces so they split.
        if s.chars().any(is_chinese_char) {
            let mut t = String::with_capacity(s.len());
            for c in s.chars() {
                if is_chinese_char(c) {
                    t.push(' ');
                    t.push(c);
                    t.push(' ');
                } else {
                    t.push(c);
                }
            }
            s = t;
        }

        // strip_accents: strip_accents.unwrap_or(lowercase) = true here →
        // NFD then drop nonspacing marks (Mn). Same crate/order as HF.
        s = s.nfd().map(|(c, _)| c).filter(|c| !c.is_mark_nonspacing()).collect();

        // lowercase: per-char Unicode to_lowercase (may expand 1→N chars).
        let mut low = String::with_capacity(s.len());
        for c in s.chars() {
            for lc in c.to_lowercase() {
                low.push(lc);
            }
        }
        low
    }

    // =========================================================================
    // WORDPIECE — tokenizers/src/models/wordpiece/mod.rs `tokenize`
    // =========================================================================

    /// Greedy longest-match-from-front WordPiece. Appends ids for `word`.
    /// On a "bad" word (any unmatchable piece) the WHOLE word → one [UNK].
    fn wordpiece_into(&self, word: &str, out: &mut Vec<u32>) {
        if word.is_empty() {
            return;
        }
        // > max_input_chars_per_word (in CHARS) → single [UNK].
        if word.chars().count() > self.max_input_chars_per_word {
            if let Some(unk) = self.unk_token_id {
                out.push(unk);
            }
            return;
        }

        let mut sub_ids: Vec<u32> = Vec::new();
        let mut is_bad = false;
        let mut start = 0usize;
        let len = word.len();

        while start < len {
            let mut end = len;
            let mut cur: Option<(u32, usize)> = None;
            while start < end {
                let piece = &word[start..end];
                // continuing_subword_prefix on all but the first piece.
                let candidate: String = if start > 0 {
                    format!("{}{}", self.continuing_subword_prefix, piece)
                } else {
                    piece.to_string()
                };
                if let Some(&id) = self.vocab.get(candidate.as_str()) {
                    cur = Some((id, end));
                    break;
                }
                // shrink by the last char's utf8 length (of the bare piece).
                end -= piece.chars().next_back().map_or(1, |c| c.len_utf8());
            }
            match cur {
                Some((id, new_end)) => {
                    sub_ids.push(id);
                    start = new_end;
                }
                None => {
                    is_bad = true;
                    break;
                }
            }
        }

        if is_bad {
            if let Some(unk) = self.unk_token_id {
                out.push(unk);
            }
        } else {
            out.extend(sub_ids);
        }
    }
}

enum Segment<'a> {
    Text(&'a str),
    Added(u32),
}

// =============================================================================
// PRE-TOKENIZE — BertPreTokenizer (tokenizers/src/pre_tokenizers/bert.rs)
// =============================================================================

/// Split on whitespace (removed), then isolate punctuation as its own token.
/// Returns the non-empty word slices in order.
fn pre_tokenize(text: &str) -> Vec<&str> {
    let mut words: Vec<&str> = Vec::new();
    // Step 1: split on is_whitespace, whitespace REMOVED.
    for chunk in text.split(char::is_whitespace) {
        if chunk.is_empty() {
            continue;
        }
        // Step 2: split each chunk on punctuation, punctuation ISOLATED.
        let mut last = 0usize;
        let bytes_iter = chunk.char_indices();
        for (idx, c) in bytes_iter {
            if is_bert_punc(c) {
                if last < idx {
                    words.push(&chunk[last..idx]);
                }
                let next = idx + c.len_utf8();
                words.push(&chunk[idx..next]);
                last = next;
            }
        }
        if last < chunk.len() {
            words.push(&chunk[last..]);
        }
    }
    words
}

// =============================================================================
// CHARACTER PREDICATES — mirror tokenizers exactly
// =============================================================================

/// tokenizers BertNormalizer::is_whitespace
#[inline]
fn is_whitespace(c: char) -> bool {
    match c {
        '\t' | '\n' | '\r' => true,
        _ => c.is_whitespace(),
    }
}

/// tokenizers BertNormalizer::is_control — \t \n \r are NOT control; otherwise
/// "Other" categories (Cc/Cf/Cn/Co) per `unicode_categories::is_other`.
#[inline]
fn is_control(c: char) -> bool {
    match c {
        '\t' | '\n' | '\r' => false,
        _ => c.is_other(),
    }
}

/// tokenizers BertNormalizer::is_chinese_char (CJK Unicode blocks).
#[inline]
fn is_chinese_char(c: char) -> bool {
    matches!(
        c as usize,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B920..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

/// tokenizers BertPreTokenizer::is_bert_punc — ASCII punctuation OR Unicode P*.
#[inline]
fn is_bert_punc(c: char) -> bool {
    c.is_ascii_punctuation() || c.is_punctuation()
}

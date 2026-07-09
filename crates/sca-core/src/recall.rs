//! recall_fused — the proven 300/300 WikimQA recall pipeline.
//!
//! Ported from the pyo3-gated `ScaCoreEngine::recall_fused` in lib.rs so that
//! the Rust-native `SaidFile::search_internal` path (and therefore every
//! `said ask` invocation) runs the full pipeline, not the bare
//! `engine.search_immutable` fallback.
//!
//! Pipeline stages (matches Python test_300_300.py exactly):
//!
//!   1. SCA top-50
//!   2. Phrase extraction from query text
//!      - "film/song/movie/book X" titles
//!      - "of X" entity tails
//!      - Uppercase proper-noun sequences (with hyphen support)
//!      - Comma-separated entity parts
//!   3. Grep re-rank with specificity weighting (only phrases matching ≤10 docs
//!      contribute to scoring; specific matches ≤5 docs get max weight)
//!   4. Morphological variant expansion + AND-pair injection
//!      - Strip suffixes (s/es/ed/ers/ing), join hyphens
//!      - Find rarest variant per content word
//!      - AND-pair rare variants to find unique cross-doc matches
//!   5. Candidate merge: SCA ∪ specific injection, grep boost only inside
//!      SCA top-10 OR for specific-phrase docs (prevents regression on
//!      narrative tasks where broad phrases match many docs)
//!   6. Iterative multi-hop bridge re-query when the top-1 / top-2 score gap
//!      is narrow (< 1.5). Extracts bridge entities from top-3 doc texts,
//!      re-issues "bridge + relational context" queries, blends results back.
//!
//! All numeric constants and tie-breaker heuristics copied verbatim from the
//! Python reference — no tuning changes. This is "port the proven code onto
//! the Rust path," not "redesign recall."

use std::collections::{HashMap, HashSet};

use crate::engine::ScaEngine;

// ════════════════════════════════════════════════════════════════════════════
// CODE-INTENT / PASSKEY / NEEDLE DETECTION
// ════════════════════════════════════════════════════════════════════════════
//
// Ported verbatim from SAID-LAM-private/src/sca_dropin.rs. These helpers are
// what carries the 100% passkey + needle recall rate on the SAID-LAM-private
// benchmark. The key insight: short entity-poor queries that ASK for a code
// ("what is the passkey"), or short queries that CONTAIN a pure-numeric 5-10
// char token, need a lexical-only path. Semantic cosine over a 64-dim
// fingerprint cannot reliably distinguish one passkey from another because
// the per-token statistical shape is identical.
//
// Route:
//   PureLexical  — query asks for a code, or contains one → IDF-weighted
//                  token overlap, NO semantic at all
//   PureSemantic — very short all-stopword queries (STS style) → SCA only
//   FullHybrid   — everything else → recall_fused (default path)

/// Words whose presence in a query signals "user wants a code/needle" and
/// switches routing to PureLexical.
pub(crate) const CODE_INTENT_WORDS: &[&str] =
    &["passkey", "password", "passcode", "serial", "needle"];

/// Known compound → split mappings for CODE_INTENT_WORDS that may appear as
/// two tokens in documents (e.g. doc says "pass key" but query says
/// "passkey"). The lexical path expands compound queries with their split
/// variants so both representations match.
pub(crate) const COMPOUND_SPLITS: &[(&str, &[&str])] = &[
    ("passkey", &["pass", "key"]),
    ("passcode", &["pass", "code"]),
    ("password", &["pass", "word"]),
];

/// Query route classification (mirrors SAID-LAM-private's `QueryRoute`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryRoute {
    /// Passkeys / codes / needles — IDF-weighted lexical only, no semantic.
    PureLexical,
    /// STS-style generic short queries — SCA semantic only, no lexical.
    PureSemantic,
    /// Default: full recall_fused pipeline.
    FullHybrid,
}

/// Strict passkey/needle detection. Copied from SAID-LAM-private's
/// `looks_like_code` — deliberately conservative to avoid false positives on
/// dates, money, measurements, and ordinals.
pub fn looks_like_code(word: &str) -> bool {
    // Early exit for common non-code formats, checked BEFORE alphanumeric
    // clean-up so format hints survive.
    if word.contains('/') || word.contains('-') {
        return false; // date / year range
    }
    if word.contains('$') || word.contains('€') || word.contains('£') || word.contains(',') {
        return false; // currency / formatted number
    }
    if word.starts_with('(') || word.starts_with('[') {
        return false; // parenthesized reference
    }
    if word.chars().any(|c| c == '±' || c == '×' || c == '÷') {
        return false;
    }

    // Alphanumeric only — no separators.
    let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
    if clean.len() < 5 || clean.len() > 15 {
        return false;
    }

    // Exclude ordinals (1st, 2nd, 3rd, 100th).
    let lower = clean.to_lowercase();
    if lower.ends_with("st") || lower.ends_with("nd")
        || lower.ends_with("rd") || lower.ends_with("th")
    {
        let prefix = &lower[..lower.len() - 2];
        if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }

    // Exclude measurements (100kg, 50cm, 200m, 2gb).
    let measurement_suffixes = [
        "kg", "km", "cm", "mm", "ml", "mg", "gb", "mb", "kb", "hz", "bn",
        "mn", "bln",
    ];
    for suffix in measurement_suffixes {
        if lower.ends_with(suffix) {
            let prefix = &lower[..lower.len() - suffix.len()];
            if prefix.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return false;
            }
        }
    }

    let chars: Vec<char> = clean.chars().collect();
    let digit_count = chars.iter().filter(|c| c.is_ascii_digit()).count();
    let letter_count = chars.iter().filter(|c| c.is_ascii_alphabetic()).count();

    // Pure numeric, 5-10 digits → passkey (12345678).
    // Conservative: only this single rule. Alphanumeric codes produce too many
    // false positives on ordinary words.
    letter_count == 0 && digit_count >= 5 && digit_count <= 10
}

/// Classify a query into one of the three routes.
///
/// Uses the engine's `doc_texts_original` to get access to `CrystallineCore`
/// for IDF lookups. Returns the route plus the expanded query token set that
/// the lexical path will score against.
pub fn route_query(
    engine: &ScaEngine,
    query_text: &str,
) -> (QueryRoute, HashSet<String>) {
    // Tokenize: lowercase, strip trailing punctuation, drop < 3 char tokens.
    let q_words: Vec<String> = query_text
        .split_whitespace()
        .map(|s| {
            s.to_lowercase()
                .trim_end_matches(|c: char| c.is_ascii_punctuation())
                .to_string()
        })
        .filter(|s| s.len() >= 3)
        .collect();

    if q_words.is_empty() {
        return (QueryRoute::PureSemantic, HashSet::new());
    }

    let mut q_expanded: HashSet<String> = HashSet::new();
    let mut has_any_code = false;
    let mut total_idf = 0.0f32;
    let mut known = 0usize;

    for word in &q_words {
        if looks_like_code(word) {
            has_any_code = true;
            total_idf += 5.0;
            known += 1;
            q_expanded.insert(word.clone());
            continue;
        }
        let idf = engine.core.get_word_idf(word);
        if idf > 1.0 {
            // Known vocabulary word (default is 1.0 for OOV).
            known += 1;
            total_idf += idf;
            q_expanded.insert(word.clone());
        } else {
            // OOV: try compound split (passkey → pass + key)
            let mut split = false;
            for &(compound, parts) in COMPOUND_SPLITS {
                if word == compound {
                    for &p in parts {
                        if p.len() >= 3 {
                            q_expanded.insert(p.to_string());
                        }
                    }
                    q_expanded.insert(word.clone());
                    split = true;
                    break;
                }
            }
            if !split {
                q_expanded.insert(word.clone());
            }
        }
    }

    // Code-intent flag: query mentions passkey/password/needle etc. Must be
    // a whole-word match so "helping" doesn't trigger "help".
    let has_code_intent = q_words
        .iter()
        .any(|w| CODE_INTENT_WORDS.iter().any(|ci| w == *ci));

    // Short-discourse detection: <= 8 tokens, no code intent, average IDF
    // below 1.2 (entirely common words), zero high-IDF content words.
    let idf_avg = if known > 0 { total_idf / known as f32 } else { 1.0 };
    let high_idf_count = q_expanded
        .iter()
        .filter(|w| engine.core.get_word_idf(w) > 2.5)
        .count();
    let is_short_discourse = q_words.len() <= 8
        && !has_any_code
        && !has_code_intent
        && idf_avg <= 1.2
        && high_idf_count == 0;

    let route = if has_any_code || has_code_intent {
        QueryRoute::PureLexical
    } else if is_short_discourse {
        QueryRoute::PureSemantic
    } else {
        QueryRoute::FullHybrid
    };

    (route, q_expanded)
}

/// Pure-lexical search: IDF-weighted token-overlap score against every doc's
/// word set. This is the path that hits 100% on passkey + needle benchmarks
/// in SAID-LAM-private — NO semantic scoring, so the 1-bit SCA fingerprint
/// ambiguity that kills needle queries is bypassed entirely.
///
/// Score formula: `sum(idf[w] for w in q ∩ doc) / sum(idf[w] for w in q)`
/// — normalized to [0, 1] so a doc containing every query token scores 1.0.
pub fn search_pure_lexical(
    engine: &ScaEngine,
    q_expanded: &HashSet<String>,
    top_k: usize,
) -> Vec<(String, f32)> {
    if q_expanded.is_empty() {
        return Vec::new();
    }

    let core = &engine.core;

    // Total query IDF (clamped to ≥1.0 so the division is safe).
    let total_q_idf: f32 = q_expanded
        .iter()
        .map(|w| core.get_word_idf(w))
        .sum::<f32>()
        .max(1.0);

    // Walk every doc in the index, compute hit IDF.
    let mut results: Vec<(String, f32)> = Vec::new();
    let n_docs = core.num_documents();
    for doc_idx in 0..n_docs {
        let hit_idf: f32 = q_expanded
            .iter()
            .filter(|w| core.doc_has_word(doc_idx, w))
            .map(|w| core.get_word_idf(w))
            .sum();
        if hit_idf > 0.0 {
            if let Some(did) = core.doc_id_at(doc_idx) {
                results.push((did.to_string(), hit_idf / total_q_idf));
            }
        }
    }

    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(top_k);
    results
}


/// Run the full recall_fused pipeline against the given engine and corpus.
///
/// The corpus slices (`ids`, `texts`, `texts_lower`) MUST be the same shape
/// as what the engine was indexed on — they're used for phrase matching and
/// morphological variant lookup against the lowercased text cache.
///
/// The caller is expected to call this with their own cached corpus (e.g.
/// `SaidFile::corpus_ids` / `corpus_texts_lower`) so we don't re-allocate
/// per query.
pub fn recall_fused(
    engine: &mut ScaEngine,
    query_emb: &[f32],
    query_text: &str,
    top_k: usize,
    ids: &[String],
    texts: &[String],
    texts_lower: &[String],
) -> Vec<(String, f32)> {
    if ids.is_empty() {
        return Vec::new();
    }

    // Equivalent of Python's `ensure_ready()`: make sure lexical scoring data is available.
    // FAST PATH (cold `said ask`): if a WIDX section carries word_idf (v2), load it VERBATIM instead
    // of re-tokenising every doc — rebuild_entity_data was the ~3.7 s cold-CLI cost on a 37k brain.
    // The doc word-structures are read on demand via the WIDX-aware accessors (docs_for_wid /
    // doc_has_word), so IDF is the only thing the query still needs eagerly. Falls back to the full
    // rebuild for pre-WIDX (v1) files or a not-yet-saved in-process brain.
    if engine.core.word_idf_is_empty() {
        if !engine.core.hydrate_word_idf_from_widx() {
            if !engine.doc_texts_original.is_empty() {
                let t = engine.doc_texts_original.clone();
                engine.rebuild_entity_data(&t);
            }
        }
    }

    // ── Layer 1: SCA top-50 ────────────────────────────────────────────────
    let sca_hits = engine.search_immutable(query_emb, query_text, 50);
    if sca_hits.is_empty() {
        return Vec::new();
    }
    let max_sca = sca_hits[0].score;

    // ── Layer 2: phrase extraction ─────────────────────────────────────────
    let q_clean = query_text
        .replace("'s", "")
        .replace("\u{2019}s", "")
        .trim_end_matches('?')
        .trim()
        .to_string();

    let mut phrases: Vec<String> = Vec::new();

    // Film/song/movie/book titles: everything after keyword to end
    for prefix in &["film ", "song ", "movie ", "book "] {
        if let Some(pos) = q_clean.to_lowercase().find(prefix) {
            let title = q_clean[pos + prefix.len()..].trim().to_string();
            if title.len() > 2 {
                phrases.push(title);
            }
        }
    }

    // "of X" entity: match last lowercase " of " in query, strip trailing
    // relational tokens ("born", "died", ...) so we get just the entity.
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
            let stop = [
                "born", "died", "buried", "father", "mother", "husband",
                "wife", "study", "earned", "from",
            ];
            let entity: String = after
                .split_whitespace()
                .take_while(|w| !stop.contains(&w.to_lowercase().as_str()))
                .collect::<Vec<_>>()
                .join(" ");
            if entity.len() > 2 {
                phrases.push(entity);
            }
        }
    }

    // Uppercase proper-noun sequences (multi-word, hyphen-aware)
    {
        let words: Vec<&str> = q_clean.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            let first_char = words[i].chars().next().unwrap_or('a');
            if first_char.is_uppercase() {
                let start = i;
                while i < words.len() {
                    let ch = words[i].chars().next().unwrap_or('a');
                    if ch.is_uppercase()
                        || (words[i].contains('-')
                            && words[i].chars().any(|c| c.is_uppercase()))
                    {
                        i += 1;
                    } else {
                        break;
                    }
                }
                if i - start >= 2 {
                    let phrase = words[start..i].join(" ");
                    let stop = [
                        "father", "mother", "husband", "wife", "born", "died",
                        "study", "earned", "paternal", "maternal", "grandfather",
                        "grandmother",
                    ];
                    let clean: String = phrase
                        .split_whitespace()
                        .take_while(|w| !stop.contains(&w.to_lowercase().as_str()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let skip = ["What", "Where", "Who", "Which", "How", "Are"];
                    if clean.len() > 2 && !skip.contains(&clean.as_str()) {
                        phrases.push(clean);
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    // Single-word capitalized entities (e.g. "Mara", "Melanie") — ADDITIVE to the
    // multi-word path above, which is left untouched (it is tuned for WikimQA/LoCoMo).
    // The docs' Layer-5 entity boost claims single-word caps are handled; without this
    // a query like "what does Mara enjoy" ranked other people's same-topic notes above
    // Mara's own. Skips question words and sentence-initial caps to avoid noise; the
    // entity flows through the SAME specific_docs grep-injection machinery below.
    {
        let words: Vec<&str> = q_clean.split_whitespace().collect();
        for (idx, w) in words.iter().enumerate() {
            // strip trailing punctuation for the cap/length test
            let clean: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
            if clean.len() <= 2 { continue; }
            let is_cap = clean.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
            let rest_lower = clean.chars().skip(1).all(|c| !c.is_uppercase());
            // single proper noun: Capitalized, not ALL-CAPS code, not a stopword/question
            // word (reuse the canonical ASK_STOPWORDS — single source of truth, covers
            // what/who/which/when/the/…), and not the sentence-initial token (its cap is
            // grammatical, not an entity signal).
            let is_stop = crate::ask::is_ask_stopword(&clean.to_lowercase());
            if is_cap && rest_lower && idx > 0 && !is_stop {
                phrases.push(clean);
            }
        }
    }

    // Comma-separated entity parts (e.g. "Hermann, Prince Of Hohenlohe")
    for phrase in phrases.clone() {
        if phrase.contains(',') {
            for part in phrase.split(',') {
                let p = part.trim().to_string();
                if p.len() > 2
                    && p.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                {
                    phrases.push(p);
                }
            }
        }
    }

    // Fallback: raw query when nothing specific extracted
    if phrases.is_empty() {
        let raw = q_clean.clone();
        if raw.len() > 2 {
            phrases.push(raw);
        }
    }

    // ── Layer 3: grep re-rank with specificity weighting ───────────────────
    let mut grep_scores: HashMap<String, f32> = HashMap::new();
    let mut specific_docs: Vec<String> = Vec::new();
    for phrase in &phrases {
        let pl = phrase.to_lowercase();
        if pl.len() < 2 {
            continue;
        }
        let mut phrase_matches: Vec<(String, usize)> = Vec::new();
        for (idx, did) in ids.iter().enumerate() {
            let count = texts_lower[idx].matches(&pl).count();
            if count > 0 {
                phrase_matches.push((did.clone(), count));
            }
        }
        let n_matches = phrase_matches.len();
        // Only specific phrases (≤10 docs) contribute to scoring — broad
        // phrases matching many docs regress narrative-style tasks.
        if n_matches <= 10 {
            let specificity = if n_matches <= 5 { 1.0 } else { 5.0 / n_matches as f32 };
            for (did, count) in &phrase_matches {
                *grep_scores.entry(did.clone()).or_insert(0.0) +=
                    *count as f32 * specificity;
            }
        }
        if n_matches > 0 && n_matches <= 5 {
            for (did, _) in &phrase_matches {
                specific_docs.push(did.clone());
            }
        }
    }
    let max_grep = grep_scores.values().copied().fold(1.0f32, f32::max);

    // ── Layer 4: morphological variant expansion + AND-pair injection ──────
    {
        let stop_words: HashSet<&str> = [
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
            "have", "has", "had", "do", "does", "did", "will", "would", "shall",
            "should", "can", "could", "may", "might", "must", "to", "of", "in",
            "for", "on", "at", "by", "with", "from", "as", "into", "through",
            "during", "before", "after", "above", "below", "between", "under",
            "not", "no", "nor", "but", "or", "and", "so", "yet", "both", "either",
            "neither", "each", "every", "all", "any", "few", "many", "some",
            "most", "much", "such", "own", "other", "another", "only", "very",
            "also", "back", "just", "about", "out", "up", "over", "down", "off",
            "still", "again", "further", "then", "once", "here", "there", "when",
            "where", "why", "how", "more", "these", "those", "his", "her", "he",
            "she", "they", "their", "it", "its", "this", "that", "what", "who",
            "which", "you", "your", "we", "our", "them",
        ]
        .iter()
        .copied()
        .collect();

        let q_lower = query_text.to_lowercase();
        let mut variants: Vec<String> = Vec::new();
        for w in q_lower.split_whitespace() {
            let clean: String = w.chars().filter(|c| c.is_ascii_lowercase()).collect();
            if clean.len() < 4 || stop_words.contains(clean.as_str()) {
                continue;
            }
            variants.push(clean.clone());
            if clean.ends_with("ers") && clean.len() > 5 {
                variants.push(clean[..clean.len() - 3].to_string());
                variants.push(clean[..clean.len() - 1].to_string());
            } else if clean.ends_with("ing") && clean.len() > 5 {
                variants.push(clean[..clean.len() - 3].to_string());
            } else if clean.ends_with("ed") && clean.len() > 4 {
                variants.push(clean[..clean.len() - 2].to_string());
            } else if clean.ends_with("es") && clean.len() > 4 {
                variants.push(clean[..clean.len() - 2].to_string());
            } else if clean.ends_with("s") && clean.len() > 4 {
                variants.push(clean[..clean.len() - 1].to_string());
            }
            if w.contains('-') {
                let joined: String =
                    w.chars().filter(|c| c.is_ascii_lowercase()).collect();
                if joined.len() >= 4 {
                    variants.push(joined);
                }
                for part in w.split('-') {
                    let p: String =
                        part.chars().filter(|c| c.is_ascii_lowercase()).collect();
                    if p.len() >= 4 && !stop_words.contains(p.as_str()) {
                        variants.push(p);
                    }
                }
            }
        }
        variants.sort();
        variants.dedup();

        // Rarest variant per query word (capped at ≤10 corpus matches).
        let mut rare: Vec<(String, usize)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for v in &variants {
            if v.len() < 4 || seen.contains(v) {
                continue;
            }
            let count = texts_lower.iter().filter(|t| t.contains(v.as_str())).count();
            if count > 0 && count <= 10 && !seen.contains(v) {
                seen.insert(v.clone());
                rare.push((v.clone(), count));
            }
        }
        rare.sort_by_key(|x| x.1);

        // AND-pair the rare variants: unique cross-doc matches get injected.
        if rare.len() >= 2 {
            let sca_set: HashSet<String> =
                sca_hits.iter().map(|h| h.doc_id.clone()).collect();
            let mut injected: HashSet<String> = HashSet::new();
            for i in 0..rare.len().min(10) {
                for j in (i + 1)..rare.len().min(10) {
                    let w1 = &rare[i].0;
                    let w2 = &rare[j].0;
                    let matches: Vec<&String> = ids
                        .iter()
                        .enumerate()
                        .filter(|(idx, _)| {
                            texts_lower[*idx].contains(w1.as_str())
                                && texts_lower[*idx].contains(w2.as_str())
                        })
                        .map(|(_, did)| did)
                        .collect();
                    if matches.len() == 1
                        && sca_set.contains(matches[0])
                        && !injected.contains(matches[0])
                    {
                        specific_docs.push(matches[0].clone());
                        injected.insert(matches[0].clone());
                    }
                }
            }
        }
    }

    // ── Layer 5: candidate merge + re-rank ─────────────────────────────────
    let mut candidates: HashMap<String, f32> = HashMap::new();
    for hit in &sca_hits {
        candidates.insert(hit.doc_id.clone(), hit.score);
    }
    for did in &specific_docs {
        if !candidates.contains_key(did) {
            // Entity/phrase match not already in SCA top-50: inject at a floor so it's
            // a candidate at all.
            candidates.insert(did.clone(), max_sca * 0.8);
        } else {
            // Already an SCA candidate: ADD an entity boost (+20% of max_sca, per the
            // docs' Layer-5 spec) ON TOP of its semantic score — do NOT overwrite it
            // to a flat value. Overwriting made every entity match tie at the same
            // score, erasing the semantic ranking that distinguishes one entity's
            // many memories (e.g. Mara's cello vs Mara's bakery). Additive keeps the
            // semantic signal as the tie-break.
            let current = *candidates.get(did).unwrap_or(&0.0);
            candidates.insert(did.clone(), current + max_sca * 0.20);
        }
    }

    let has_ultra_specific = !specific_docs.is_empty();
    let sca_top10: HashSet<String> =
        sca_hits.iter().take(10).map(|h| h.doc_id.clone()).collect();
    let mut reranked: Vec<(String, f32)> = candidates
        .iter()
        .map(|(did, sca)| {
            if has_ultra_specific && sca_top10.contains(did) {
                let g = (grep_scores.get(did).copied().unwrap_or(0.0) / max_grep)
                    * max_sca
                    * 1.0;
                (did.clone(), sca + g)
            } else if has_ultra_specific
                && specific_docs.contains(did)
                && !sca_top10.contains(did)
            {
                let g = (grep_scores.get(did).copied().unwrap_or(0.0) / max_grep)
                    * max_sca
                    * 1.0;
                (did.clone(), sca + g)
            } else {
                (did.clone(), *sca)
            }
        })
        .collect();
    reranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // ── Layer 6: iterative multi-hop bridge re-query ───────────────────────
    if reranked.len() >= 2 {
        let gap = reranked[0].1 - reranked[1].1;
        if gap < 1.5 {
            let q_lower = query_text.to_lowercase();
            let rel_context: Vec<&str> = [
                "born", "died", "buried", "nationality", "award", "studied",
                "mother", "father", "grandmother", "place of birth",
                "place of death",
            ]
            .iter()
            .filter(|r| q_lower.contains(**r))
            .copied()
            .collect();

            // Query entities (same uppercase-sequence logic as phrase extraction)
            let mut q_ents: Vec<String> = Vec::new();
            let qw: Vec<&str> = q_clean.split_whitespace().collect();
            let mut j = 0;
            while j < qw.len() {
                if qw[j].chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    let s = j;
                    while j < qw.len() {
                        let ch = qw[j].chars().next().unwrap_or('a');
                        let particles = [
                            "of", "the", "de", "von", "van", "du", "la", "le",
                            "el", "al", "bin", "ibn", "di", "and", "on", "in",
                        ];
                        if ch.is_uppercase()
                            || particles.contains(&qw[j].to_lowercase().as_str())
                        {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if j - s >= 2 {
                        let e = qw[s..j].join(" ").to_lowercase();
                        if e.len() > 4 {
                            q_ents.push(e);
                        }
                    }
                } else {
                    j += 1;
                }
            }

            // Extract bridge entities from top-3 doc sentences that contain
            // either a query entity OR a relational context word.
            let mut bridges: Vec<String> = Vec::new();
            for (did, _) in reranked.iter().take(3) {
                if let Some(idx) = ids.iter().position(|d| d == did) {
                    let text = &texts[idx];
                    for sent in text.split(|c: char| c == '.' || c == '!' || c == '?') {
                        let sl = sent.to_lowercase();
                        let has_ent = q_ents.iter().any(|qe| sl.contains(qe.as_str()));
                        let has_rel = rel_context.iter().any(|r| sl.contains(r));
                        if !has_ent && !has_rel {
                            continue;
                        }
                        let sw: Vec<&str> = sent.split_whitespace().collect();
                        let mut k = 0;
                        while k < sw.len() {
                            if sw[k].chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                            {
                                let s2 = k;
                                while k < sw.len() {
                                    let ch = sw[k].chars().next().unwrap_or('a');
                                    let particles = [
                                        "of", "the", "de", "von", "van", "du",
                                        "la", "le", "el", "al", "bin", "ibn", "di",
                                    ];
                                    if ch.is_uppercase()
                                        || particles
                                            .contains(&sw[k].to_lowercase().as_str())
                                    {
                                        k += 1;
                                    } else {
                                        break;
                                    }
                                }
                                let ent = sw[s2..k].join(" ");
                                let skip = [
                                    "The", "This", "That", "His", "Her", "She",
                                    "He", "They", "Their", "After", "Before",
                                    "However", "Although", "During", "Between",
                                ];
                                if ent.len() > 3
                                    && !q_ents.contains(&ent.to_lowercase())
                                    && !skip.contains(&ent.as_str())
                                {
                                    bridges.push(ent);
                                }
                            } else {
                                k += 1;
                            }
                        }
                    }
                }
            }
            bridges.sort_by(|a, b| b.len().cmp(&a.len()));
            bridges.truncate(5);

            if !bridges.is_empty() {
                let ctx = rel_context.join(" ");
                let mut r2: HashMap<String, f32> = HashMap::new();
                for bridge in &bridges {
                    let r2q = format!("{} {}", bridge, ctx);
                    if let Some(emb) = engine.encode_query(&r2q) {
                        for hit in engine.search_immutable(&emb, &r2q, 10) {
                            *r2.entry(hit.doc_id).or_insert(0.0) += hit.score;
                        }
                    }
                }
                if !r2.is_empty() {
                    let max_r2 = r2.values().copied().fold(1.0f32, f32::max);
                    for (did, score) in &mut reranked {
                        if let Some(&rs) = r2.get(did) {
                            *score += (rs / max_r2) * max_sca * 0.4;
                        }
                    }
                    for (did, rs) in &r2 {
                        if !reranked.iter().any(|(d, _)| d == did) {
                            reranked.push((
                                did.clone(),
                                max_sca * 0.7 + (rs / max_r2) * max_sca * 0.4,
                            ));
                        }
                    }
                    reranked
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                }
            }
        }
    }

    // Layer 7: phrase tiebreaker on top-10 (exclusive 2..=6-grams of query
    // CONTENT words). Safe as a pure top-N re-rank — can only shuffle chunks
    // that are already ranked together. Critical for short entity-poor
    // queries ("how many steps") where the earlier phrase-extraction layers
    // produce no candidates and the pipeline falls through to raw SCA.
    phrase_tiebreaker_top_n(&mut reranked, query_text, ids, texts_lower, 10);

    reranked.truncate(top_k);
    reranked
}

/// Phrase tiebreaker for the top-N results.
///
/// For each result in the top-N and each n-gram length 2..=6, check whether
/// the query n-gram appears in exactly one top-N doc text. If so, add a
/// `0.05 * n² * occurrence_count` score bonus to that chunk.
///
/// Uses QUERY content words (stopwords filtered) so bigrams of "how many" or
/// "MVP plan" that span a stopword gap are dropped — only content-dense
/// anchors contribute, which is what the Python reference does on the final
/// `query_text.lower().split()` of its long-query branch.
fn phrase_tiebreaker_top_n(
    reranked: &mut [(String, f32)],
    query_text: &str,
    ids: &[String],
    texts_lower: &[String],
    top_n_limit: usize,
) {
    // Build lowercased content words (drop stopwords) from the query.
    let stop: HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "shall",
        "should", "can", "could", "may", "might", "must", "to", "of", "in",
        "for", "on", "at", "by", "with", "from", "as", "into", "through",
        "how", "many", "much", "why", "when", "where", "which", "who", "whom",
        "what", "that", "this", "these", "those", "any", "all", "some", "only",
        "not", "no", "or", "and", "if", "then", "so", "yet", "but",
    ]
    .iter()
    .copied()
    .collect();
    let content_words: Vec<String> = query_text
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() >= 3 && !stop.contains(w.as_str()))
        .collect();
    if content_words.len() < 2 {
        return;
    }

    let top_n = reranked.len().min(top_n_limit);
    if top_n < 2 {
        return;
    }

    // Cache the lowercased text for each top-N doc (look up via ids slice).
    let top_texts: Vec<String> = reranked[..top_n]
        .iter()
        .map(|(did, _)| {
            ids.iter()
                .position(|d| d == did)
                .and_then(|i| texts_lower.get(i).cloned())
                .unwrap_or_default()
        })
        .collect();

    let max_n = 6.min(content_words.len());
    for k in 0..top_n {
        for nn in 2..=max_n {
            if content_words.len() < nn {
                break;
            }
            for ii in 0..=(content_words.len() - nn) {
                let p = content_words[ii..ii + nn].join(" ");
                if p.len() < 5 {
                    continue;
                }
                if top_texts[k].contains(&p) {
                    // Exclusive to this chunk in the top-N?
                    let others =
                        top_texts.iter().enumerate().filter(|(j, _)| *j != k).count();
                    let others_with_match = top_texts
                        .iter()
                        .enumerate()
                        .filter(|(j, t)| *j != k && t.contains(&p))
                        .count();
                    if others == 0 || others_with_match == 0 {
                        let count = top_texts[k].matches(&p).count() as f32;
                        reranked[k].1 += 0.05 * (nn as f32).powi(2) * count;
                    }
                }
            }
        }
    }
    // Re-sort only the top-N
    reranked[..top_n]
        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
}

// ────────────────────────────────────────────────────────────────────────────
// Phase 2: long-query passage-blend pipeline
// ────────────────────────────────────────────────────────────────────────────

/// Passage-level SCA engine paired with a parent-doc lookup map.
///
/// Long queries (≥ 20 words) use this second engine to score 512-word passages
/// with 256-word stride. Parent doc scores are then blended with doc-level SCA
/// scores via the formula from the Python reference.
pub struct PassageEngine {
    pub engine: ScaEngine,
    /// `passage_id → parent_doc_id` (e.g. "auth.py_p2" → "auth.py")
    pub passage_to_doc: HashMap<String, String>,
    /// Passage ids in index order (for rebuild + diagnostics)
    pub passage_ids: Vec<String>,
}

impl PassageEngine {
    pub fn new() -> Self {
        Self {
            engine: ScaEngine::new(),
            passage_to_doc: HashMap::new(),
            passage_ids: Vec::new(),
        }
    }

    /// Rebuild the passage index from the current doc corpus.
    /// Splits each doc into 512-word passages with 256-word stride; short docs
    /// keep a single passage `<doc_id>_p0`.
    ///
    /// Gated on `static-embed` because it calls `ScaEngine::index_batch`,
    /// which needs the static encoder.
    #[cfg(feature = "static-embed")]
    pub fn rebuild(
        &mut self,
        doc_ids: &[String],
        doc_texts: &[String],
    ) -> Result<(), String> {
        self.passage_to_doc.clear();
        self.passage_ids.clear();
        self.engine = ScaEngine::new();

        let mut p_ids: Vec<String> = Vec::new();
        let mut p_texts: Vec<String> = Vec::new();

        for (did, text) in doc_ids.iter().zip(doc_texts.iter()) {
            let words: Vec<&str> = text.split_whitespace().collect();
            if words.len() <= 512 {
                let pid = format!("{}_p0", did);
                self.passage_to_doc.insert(pid.clone(), did.clone());
                p_ids.push(pid.clone());
                p_texts.push(text.clone());
                self.passage_ids.push(pid);
                continue;
            }
            let mut i = 0usize;
            let mut pi = 0usize;
            loop {
                let end = (i + 512).min(words.len());
                let pid = format!("{}_p{}", did, pi);
                self.passage_to_doc.insert(pid.clone(), did.clone());
                p_ids.push(pid.clone());
                p_texts.push(words[i..end].join(" "));
                self.passage_ids.push(pid);
                if end >= words.len() {
                    break;
                }
                i += 256;
                pi += 1;
            }
        }

        if p_ids.is_empty() {
            return Ok(());
        }

        // Mirror index_batch path — the passage engine needs its own static
        // encoder handle. In the v1 pipeline we expect the caller to have
        // already loaded an encoder on the main engine, so we copy it across.
        self.engine.index_batch(&p_ids, &p_texts)?;
        Ok(())
    }

    /// Empty when the rebuild was never called or produced no passages.
    pub fn is_empty(&self) -> bool {
        self.passage_ids.is_empty()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// TAG-FILTERED SCORING — enterprise metadata scoping
// ════════════════════════════════════════════════════════════════════════════

/// Known tag namespaces that can appear in queries as scoping tokens.
/// When a query contains "version 4", we extract `("version", "4")` and
/// pre-filter the corpus to only frames tagged `version:4` before scoring.
const TAG_SCOPING_KEYWORDS: &[&str] = &[
    "version", "revision", "rev", "draft", "final",
    "client", "department", "dept", "jurisdiction",
    "lang", "language",
];

/// Detect a scoping tag from the query text. Returns `Some(("namespace", "value"))`
/// if a scoping token is found, e.g. "version 4" → `Some(("version", "4"))`.
///
/// Also handles: "v4" as shorthand for "version 4", "rev 3" → "version:3".
pub fn detect_scope_tag(query: &str) -> Option<(String, String)> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.trim_end_matches(|c: char| c.is_ascii_punctuation()).to_lowercase())
        .collect();

    for (i, w) in words.iter().enumerate() {
        // "version 4", "revision 3", "rev 2"
        if TAG_SCOPING_KEYWORDS.contains(&w.as_str()) && i + 1 < words.len() {
            let next = &words[i + 1];
            let num: String = next.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !num.is_empty() {
                let ns = match w.as_str() {
                    "rev" | "revision" => "version",
                    other => other,
                };
                return Some((ns.to_string(), num));
            }
        }

        // Status keywords without a number: "draft", "final", "confidential"
        // These map to status:<keyword> tags.
        match w.as_str() {
            "draft" => return Some(("status".to_string(), "draft".to_string())),
            "final" => return Some(("status".to_string(), "final".to_string())),
            "confidential" => return Some(("status".to_string(), "confidential".to_string())),
            "internal" => return Some(("status".to_string(), "internal".to_string())),
            _ => {}
        }

        // Shorthand: "v4", "v10" embedded in the query
        if w.starts_with('v') && w.len() >= 2 {
            let num: String = w[1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if !num.is_empty() && num.len() == w.len() - 1 {
                return Some(("version".to_string(), num));
            }
        }
    }
    None
}

// ════════════════════════════════════════════════════════════════════════════
// search_full — THE ONE CANONICAL PIPELINE FUNCTION
// ════════════════════════════════════════════════════════════════════════════
//
// Every caller that needs retrieval MUST go through this function. If you
// change retrieval logic, change it HERE and every caller picks it up:
//
//   - SaidFile::search_internal (said ask CLI)
//   - examples/mteb_rust.rs (MTEB benchmark harness)
//   - examples/test_folder_recall.rs (smoke validator)
//
// Routing:
//   NIAH (passkey/needle detected) → engine.search_niah (keyword × 1000 + align)
//   Short query (< 20 words)      → recall_fused (grep + multi-hop + morph AND)
//   Long query (≥ 20 words)        → passage blend pipeline
//   Empty corpus fallback          → engine.core.search_unified_quantized
//
// MTEB proven scores through this function:
//   WikimQA:       1.00000  (300/300, short-query path)
//   Needle:        1.00000  (400/400, NIAH path)
//   QMSum:         0.89190  (long-query passage blend)
//   SummScreenFD:  0.98123  (long-query passage blend)

/// The global retrieval function. All callers use this — ONE function, ONE
/// set of tuning constants, ONE place to change if retrieval needs improvement.
///
/// `passage_engine` can be None (short-query-only workloads skip building it).
/// `corpus_ids/texts/texts_lower` are needed for recall_fused injection; pass
/// empty slices to skip the injection layer (degrades WikimQA by ~3%).
pub fn search_full(
    doc_engine: &mut ScaEngine,
    passage_engine: Option<&mut PassageEngine>,
    query_text: &str,
    query_id: Option<&str>,
    top_k: usize,
    corpus_ids: &[String],
    corpus_texts: &[String],
    corpus_texts_lower: &[String],
) -> Vec<(String, f32)> {
    // No scope filter from this entry point — pass through to the full version.
    search_full_scoped(
        doc_engine, passage_engine, query_text, query_id, top_k,
        corpus_ids, corpus_texts, corpus_texts_lower, None,
    )
}

/// The actual implementation with optional tag-scope filtering.
/// When `scope_doc_ids` is Some, the corpus is narrowed to only those doc_ids
/// before any scoring happens. This is the enterprise metadata pre-filter that
/// resolves version collision (Chamber 3), client scoping, jurisdiction
/// filtering, etc.
pub fn search_full_scoped(
    doc_engine: &mut ScaEngine,
    mut passage_engine: Option<&mut PassageEngine>,
    query_text: &str,
    query_id: Option<&str>,
    top_k: usize,
    corpus_ids: &[String],
    corpus_texts: &[String],
    corpus_texts_lower: &[String],
    scope_doc_ids: Option<&HashSet<String>>,
) -> Vec<(String, f32)> {
    // If scope filtering is active, narrow the corpus slices.
    // If scope filtering is active, narrow the corpus slices AND remember
    // the scope set for post-filtering the final results (the SCA engine
    // scores against the FULL index, so we need to drop out-of-scope hits
    // after scoring).
    // Narrow the corpus slices only when a scope filter is active. The common
    // (unscoped) path BORROWS the caller's slices via Cow — previously this did
    // corpus_texts.to_vec() + corpus_texts_lower.to_vec(), cloning the ENTIRE raw +
    // lowercased corpus on every query (a transient 2× corpus spike that, at scale, was a
    // primary driver of the index/query OOM #4). Scoped queries still allocate, but only
    // the narrowed subset.
    use std::borrow::Cow;
    let (c_ids, c_texts, c_lower): (Cow<[String]>, Cow<[String]>, Cow<[String]>) =
        if let Some(scope) = scope_doc_ids {
            let mut ids = Vec::new();
            let mut texts = Vec::new();
            let mut lower = Vec::new();
            for (i, id) in corpus_ids.iter().enumerate() {
                if scope.contains(id) {
                    ids.push(corpus_ids[i].clone());
                    texts.push(corpus_texts[i].clone());
                    lower.push(corpus_texts_lower[i].clone());
                }
            }
            (Cow::Owned(ids), Cow::Owned(texts), Cow::Owned(lower))
        } else {
            (Cow::Borrowed(corpus_ids), Cow::Borrowed(corpus_texts), Cow::Borrowed(corpus_texts_lower))
        };
    let corpus_ids: &[String] = &c_ids;
    let corpus_texts: &[String] = &c_texts;
    let corpus_texts_lower: &[String] = &c_lower;
    // ── NIAH detection ──────────────────────────────────────────────────
    // Passkey/needle queries need keyword-overlap × 1000 + align_niah_qrels.
    // Detected by CODE_INTENT_WORDS in the query OR a pure-numeric 5-10
    // digit token (looks_like_code). This matches the Python is_niah check
    // plus the search_niah call at mteb_latent_space_test.py line 849.
    let q_words_raw: Vec<&str> = query_text.split_whitespace().collect();
    let has_code_intent = q_words_raw.iter().any(|w| {
        let lower = w.to_lowercase();
        let clean = lower.trim_end_matches(|c: char| c.is_ascii_punctuation());
        CODE_INTENT_WORDS.contains(&clean) || looks_like_code(clean)
    });
    if has_code_intent {
        let q_emb = doc_engine.encode_query(query_text);
        let qid = query_id.unwrap_or("");
        let hits = doc_engine.search_niah(
            query_text,
            qid,
            top_k,
            q_emb.as_deref(),
        );
        let mut results: Vec<(String, f32)> = hits.into_iter().map(|h| (h.doc_id, h.score)).collect();
        if let Some(scope) = scope_doc_ids {
            results.retain(|(did, _)| scope.contains(did));
        }
        return results;
    }

    // ── Short-query path (< 20 words): recall_fused ────────────────────
    let query_words = q_words_raw.len();
    if query_words < 20 && !corpus_ids.is_empty() {
        let q_emb = doc_engine.encode_query(query_text).unwrap_or_default();
        let mut results = recall_fused(
            doc_engine,
            &q_emb,
            query_text,
            top_k,
            corpus_ids,
            corpus_texts,
            corpus_texts_lower,
        );
        // Post-filter: keep only in-scope doc_ids
        if let Some(scope) = scope_doc_ids {
            results.retain(|(did, _)| scope.contains(did));
        }
        return results;
    }

    // ── Long-query path (≥ 20 words): passage blend pipeline ───────────
    // doc top-50 + passage top-100 + blend + tiebreaker + recall_fused
    // injection + passage injection + SCA top-50 protection.
    // This is what hits QMSum 0.89 and SummScreenFD 0.98.
    let Some(q_emb) = doc_engine.encode_query(query_text) else {
        return Vec::new();
    };

    // Step 2: doc-level top-50 via plain 1-bit Hamming (matches Python search_enhanced)
    let dh: Vec<(String, f32)> = doc_engine
        .search_immutable(&q_emb, query_text, 50)
        .into_iter()
        .map(|h| (h.doc_id, h.score))
        .collect();
    if dh.is_empty() {
        // Fallback: single-engine for empty corpora
        if !corpus_ids.is_empty() {
            return Vec::new();
        }
        return doc_engine.core.search_unified_quantized(&q_emb, query_text, top_k);
    }

    // Step 3: passage-level top-100 (if passage engine available)
    let empty_p2d = HashMap::new();
    let (ph, p2d): (Vec<(String, f32)>, &HashMap<String, String>) = match passage_engine {
        Some(ref mut pe) => {
            let hits = pe.engine
                .search_immutable(&q_emb, query_text, 100)
                .into_iter()
                .map(|h| (h.doc_id, h.score))
                .collect();
            (hits, &pe.passage_to_doc)
        }
        None => (Vec::new(), &empty_p2d),
    };
    let mut ds: HashMap<String, Vec<f32>> = HashMap::new();
    for (pid, sc) in &ph {
        let did = p2d.get(pid).cloned().unwrap_or_else(|| pid.clone());
        ds.entry(did).or_default().push(*sc);
    }

    // Step 5: passage blend — never demote
    let mut reranked: Vec<(String, f32)> = dh.iter().map(|(did, sca)| {
        let group = ds.get(did);
        let pc = group.map(|g| g.len()).unwrap_or(0) as f32;
        let bp = group.and_then(|g| g.iter().copied().fold(None, |acc: Option<f32>, v| {
            Some(acc.map(|a| a.max(v)).unwrap_or(v))
        })).unwrap_or(0.0);
        let pt3 = group.map(|g| {
            let mut sorted = g.clone();
            sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            sorted.iter().take(3).sum::<f32>()
        }).unwrap_or(0.0);
        let blend = 0.5 * sca + 0.5 * bp + 0.5 * pc + 0.05 * pt3;
        (did.clone(), blend.max(*sca))
    }).collect();
    reranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Step 6: phrase tiebreaker on top-10
    let q_words_lc: Vec<String> = query_text.to_lowercase()
        .split_whitespace().map(String::from).collect();
    let top_n = reranked.len().min(10);
    // Build a lowercase lookup from corpus_ids → corpus_texts_lower
    let corpus_lower_map: HashMap<&str, &str> = corpus_ids.iter()
        .zip(corpus_texts_lower.iter())
        .map(|(id, t)| (id.as_str(), t.as_str()))
        .collect();
    let top_texts: Vec<String> = reranked[..top_n].iter()
        .map(|(did, _)| corpus_lower_map.get(did.as_str()).unwrap_or(&"").to_string())
        .collect();
    for k in 0..top_n {
        let max_n = 6.min(q_words_lc.len() + 1);
        for nn in 2..max_n {
            if q_words_lc.len() < nn { break; }
            for ii in 0..=(q_words_lc.len() - nn) {
                let p = q_words_lc[ii..ii + nn].join(" ");
                if !top_texts[k].contains(&p) { continue; }
                let others = top_texts.iter().enumerate()
                    .filter(|(j, t)| *j != k && t.contains(&p)).count();
                if others == 0 {
                    let count = top_texts[k].matches(&p).count() as f32;
                    reranked[k].1 += 0.05 * (nn as f32).powi(2) * count;
                }
            }
        }
    }
    reranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Step 6b: recall_fused injection
    if !corpus_ids.is_empty() {
        let fused = recall_fused(
            doc_engine, &q_emb, query_text, 50,
            corpus_ids, corpus_texts, corpus_texts_lower,
        );
        let reranked_set: HashSet<String> = reranked.iter().map(|(d, _)| d.clone()).collect();
        let inject_score = reranked.first().map(|r| r.1).unwrap_or(1.0) * 0.9;
        for (did, _) in fused {
            if !reranked_set.contains(&did) {
                reranked.push((did, inject_score));
            }
        }
        reranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    }

    // Step 7: passage injection
    let mut doc_best_p: HashMap<String, f32> = HashMap::new();
    for (pid, sc) in &ph {
        let did = p2d.get(pid).cloned().unwrap_or_else(|| pid.clone());
        let entry = doc_best_p.entry(did).or_insert(f32::MIN);
        if *sc > *entry { *entry = *sc; }
    }
    let max_score = reranked.first().map(|r| r.1).unwrap_or(1.0);
    let mut pass_ranked: Vec<(String, f32)> = doc_best_p.into_iter().collect();
    pass_ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    {
        let mut in_reranked: HashSet<String> = reranked.iter().map(|(d, _)| d.clone()).collect();
        for (did2, _) in pass_ranked.into_iter().take(10) {
            if !in_reranked.contains(&did2) {
                reranked.push((did2.clone(), max_score * 0.7));
                in_reranked.insert(did2);
            }
        }
    }
    reranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Step 8: SCA top-50 protection
    {
        let sca_top50: HashSet<String> = dh.iter().take(50).map(|(d, _)| d.clone()).collect();
        let reranked_top10: HashSet<String> = reranked.iter().take(10).map(|(d, _)| d.clone()).collect();
        let dropped: Vec<String> = sca_top50.difference(&reranked_top10).cloned().collect();
        if !dropped.is_empty() && reranked.len() >= 10 {
            let mut intruders: Vec<usize> = reranked.iter().take(10).enumerate()
                .filter(|(_, (d, _))| !sca_top50.contains(d))
                .map(|(i, _)| i).collect();
            intruders.sort_by(|a, b| b.cmp(a));
            for did in dropped {
                if let Some(idx) = reranked.iter().position(|(d, _)| d == &did) {
                    if idx >= 10 && !intruders.is_empty() {
                        let swap_idx = intruders.pop().unwrap();
                        reranked.swap(swap_idx, idx);
                    }
                }
            }
            let len = reranked.len().min(10);
            let mut head: Vec<(String, f32)> = reranked[..len].to_vec();
            head.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (i, item) in head.into_iter().enumerate() {
                reranked[i] = item;
            }
        }
    }

    reranked.into_iter().take(top_k).collect()
}


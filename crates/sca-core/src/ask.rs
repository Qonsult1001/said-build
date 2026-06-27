//! Shared `ask` fusion — the 3-engine train-of-thought retrieval verb.
//!
//! Used by BOTH `said ask` (CLI) and the MCP `ask` tool. Mirrors the proven
//! CLI pipeline:
//!
//!   Engine A — Sym (exact symbol lookup)         confidence 1.00
//!   Engine B — Grep (literal keyword match)      confidence 0.40 – 0.95
//!   Engine C — SCA semantic (`recall_fused`)     confidence 0.30 – 0.80
//!                                                (multi-hop bridge, BM25,
//!                                                 entity boost, graph fan-out
//!                                                 all fire INSIDE recall_fused)
//!
//! Merge by highest confidence per doc_id, apply a RELATIVE cutoff (top_score
//! × factor) + a guaranteed top-K from the SCA engine, then truncate.
//!
//! Caller stays in control of side-effects (auto-dream, save_brain_only) —
//! this module is pure retrieval.

use std::collections::{HashMap, HashSet};

use crate::said_file::SaidFile;

/// One fused candidate from any engine.
#[derive(Debug, Clone)]
pub struct AskCandidate {
    pub doc_id: String,
    pub confidence: f32,
    /// "symbol" | "text" | "semantic"
    pub kind: &'static str,
    pub content: String,
    /// "<kind>:<start_line>-<end_line>" for symbol hits, None otherwise.
    pub location: Option<String>,
}

/// Tunable routing + cutoff constants. Shared defaults so CLI and MCP behave
/// identically when callers don't override anything.
pub const ASK_RELATIVE_CUTOFF: f32 = 0.30;
pub const ASK_SCA_GUARANTEED: usize = 3;
/// How many top SCA semantic hits are kept REGARDLESS of literal keyword overlap.
/// Pure-paraphrase queries (the case SCA exists for) share no keywords with the
/// stored fact, so the old `rank >= 3 && terms_present == 0` drop discarded correct
/// semantic hits past rank 3 — collapsing recall@10. SCA's own ranking is trusted
/// to the documented recall depth (10); only the deeper tail needs a keyword gate to
/// keep noise out. See docs/said-structure/10-benchmarks (MTEB MEAN NDCG@10 0.9655).
pub const ASK_SCA_TRUST_DEPTH: usize = 10;

/// Pending-queries threshold before dream fires, scaled to the active frame
/// count. Small brains adapt fast; large brains stay stable.
///
///   < 500 frames    → every  50 queries  (fast adaptation on personal use)
///   500 – 50k       → every  corpus_size / 10 queries
///   > 50k           → every 500 queries  (stable on enterprise scale)
///
/// Call with the active frame count; returns the pending-queries count at
/// which `brain.dream(threshold)` should fire.
pub fn dynamic_dream_threshold(active_frames: usize) -> u64 {
    let scaled = (active_frames / 10).max(50).min(500);
    scaled as u64
}


/// Stop words for keyword extraction. Conservative — filter common English
/// function words, nothing domain-specific. MATCHES the CLI list verbatim.
const ASK_STOPWORDS: &[&str] = &[
    "the","a","an","is","are","was","were","be","been","being","have","has","had",
    "do","does","did","will","would","shall","should","can","could","may","might",
    "must","to","of","in","for","on","at","by","with","from","as","into","through",
    "during","before","after","above","below","between","under","not","no","nor",
    "but","or","and","so","yet","both","either","neither","each","every","all","any",
    "few","many","some","most","much","such","own","other","another","only","very",
    "also","back","just","about","out","up","over","down","off","still","again",
    "further","then","once","here","there","when","where","why","how","more","these",
    "those","his","her","he","she","they","their","it","its","this","that","what",
    "who","which","you","your","we","our","them","i","me","my","us","if","im",
    "dont","does","doesnt","didnt","isnt",
];

/// True if `w` (lowercased) is an ask stopword. The single source of truth for
/// "this token is not an entity/keyword" — reused by the recall pipeline's
/// single-word entity extractor so the question-word filter lives in ONE place.
pub fn is_ask_stopword(w: &str) -> bool {
    ASK_STOPWORDS.contains(&w)
}

/// Parse `[[concept]]` wikilinks from text into lowercased concept strings — the
/// documented build-graph edge source (3.9). An OKF/Obsidian note carries concept
/// cross-links; these become `link:<concept>` tags at ingest and explicit graph edges
/// the recall path can traverse (so a query reaches a linked note even when the bridge
/// word isn't in the body). Returns each distinct concept once, lowercased + trimmed.
pub fn parse_wikilinks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut rest = text;
    let mut base = 0usize; // absolute offset of `rest` within `text`
    while let Some(rel_start) = rest.find("[[") {
        let start = base + rel_start;
        // Array-indexing guard: `[[` glued to a preceding alphanumeric (matrix[[i]],
        // vec[[k]]) is code subscripting, NOT a wikilink. A real wikilink stands alone
        // (preceded by start-of-text or whitespace/punctuation). Skip glued ones.
        let preceded_by_alnum = start > 0
            && (bytes[start - 1] as char).is_ascii_alphanumeric();
        let after = &text[start + 2..];
        if preceded_by_alnum {
            base = start + 2;
            rest = after;
            continue;
        }
        if let Some(end) = after.find("]]") {
            let inner = after[..end].trim();
            // Obsidian alias/section forms: [[concept|alias]] / [[concept#heading]] →
            // keep the concept part before | or #.
            let concept = inner.split(|c| c == '|' || c == '#').next().unwrap_or(inner).trim();
            // STRICT concept shape — a real wikilink concept is WORDS: ≥2 chars, contains a
            // letter, and only letter / space / - / _ / ' chars. This deliberately REJECTS
            // code/math bracket noise ([[1,2],[3,4]], a[[0]]) and single-letter code
            // indices (matrix[[i]] → 'i') so importing PDFs, code, or any text with literal
            // `[[` never coins junk concepts.
            let valid = concept.chars().count() >= 2
                && concept.len() <= 64
                && concept.chars().any(|c| c.is_alphabetic())
                && concept.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '_' || c == '\'');
            if valid {
                let c = concept.to_lowercase();
                if !out.contains(&c) { out.push(c); }
            }
            // advance past the closing ]]
            base = start + 2 + end + 2;
            rest = &text[base..];
        } else {
            break;
        }
    }
    out
}

/// Whole-token containment: true if `needle` appears in `haystack` bounded by
/// non-alphanumeric edges. Substring `.contains()` makes the discriminator "7" match
/// "office 27"/"office 17" too, so a numeric needle can't beat its near-duplicates.
/// Word-boundary matching makes "7" match only "office 7". Both args lowercased.
fn contains_token(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() { return false; }
    let nb = needle.as_bytes();
    let hb = haystack.as_bytes();
    let mut i = 0;
    while let Some(off) = haystack[i..].find(needle) {
        let s = i + off;
        let e = s + nb.len();
        let left_ok = s == 0 || !(hb[s - 1] as char).is_ascii_alphanumeric();
        let right_ok = e == hb.len() || !(hb[e] as char).is_ascii_alphanumeric();
        if left_ok && right_ok { return true; }
        i = s + 1;
        if i >= haystack.len() { break; }
    }
    false
}

/// Extract searchable keywords from a natural-language query.
/// Returns `(lowercased_keywords, original_case_keywords)`.
///
/// Lowercased keywords are used by grep and SCA (both case-insensitive).
/// Original-case keywords are used by the symbol candidate generator so
/// queries like "what is FrameStore" correctly hit the PascalCase symbol.
/// A CJK / spaceless-script character (Han, Hiragana, Katakana, Hangul). These scripts don't separate
/// words with spaces, so the ASCII word-splitter sees the whole run as ONE non-ASCII blob and drops
/// it — leaving a Chinese/Japanese/Korean query with ZERO keywords (measured: `ask` returned nothing
/// for a Chinese query even though the content was indexed). We emit overlapping CHARACTER BIGRAMS for
/// such runs, which the trigram index already matches — the documented approach for spaceless scripts.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF |  // CJK Unified Ideographs
        0x3400..=0x4DBF |  // CJK Extension A
        0x3040..=0x309F |  // Hiragana
        0x30A0..=0x30FF |  // Katakana
        0xAC00..=0xD7AF)   // Hangul syllables
}

pub fn ask_extract_keywords(query: &str) -> (Vec<String>, Vec<String>) {
    let stop: HashSet<&str> = ASK_STOPWORDS.iter().copied().collect();
    let mut seen_lower: HashSet<String> = HashSet::new();
    let mut lower: Vec<String> = Vec::new();
    let mut original: Vec<String> = Vec::new();
    // CJK character-bigram keywords (spaceless scripts have no word boundaries to split on).
    let cjk_chars: Vec<char> = query.chars().filter(|c| is_cjk(*c)).collect();
    for win in cjk_chars.windows(2) {
        let bigram: String = win.iter().collect();
        if seen_lower.insert(bigram.clone()) { lower.push(bigram.clone()); original.push(bigram); }
    }
    if cjk_chars.len() == 1 { // single CJK char query — keep the char itself
        let s: String = cjk_chars.iter().collect();
        if seen_lower.insert(s.clone()) { lower.push(s.clone()); original.push(s); }
    }
    // Split keeps UNICODE alphanumerics together; CJK is handled above, so exclude it from runs here.
    for word in query.split(|c: char| !(c.is_alphanumeric() || c == '_') || is_cjk(c)) {
        // Drop short words EXCEPT discriminators. A short token with a digit ("7","v2","B3")
        // is a high-IDF needle the docs guarantee. ALSO keep a short token that is CAPITALIZED
        // in the original ("Building C", "Plan A", "Type B") — a single capital letter/label is
        // a discriminator exactly like a number ("Building C" vs "Building D"), and dropping it
        // made `ask` unable to tell the twins apart (the exact note got buried under its tied
        // boilerplate cousins and dropped from top-K). Lowercase short alpha noise ("is","at",
        // "of") is still skipped — stopwords cover the function words.
        let has_digit = word.chars().any(|c| c.is_ascii_digit());
        let is_short_label = word.len() < 3
            && word.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
        if word.len() < 3 && !has_digit && !is_short_label { continue; }
        let w_lower = word.to_lowercase();
        if stop.contains(w_lower.as_str()) { continue; }
        if seen_lower.insert(w_lower.clone()) {
            lower.push(w_lower);
            original.push(word.to_string());
        }
    }
    (lower, original)
}

/// Split a coding-problem description into its ACTION/intent residue by removing
/// TARGET-like tokens (code identifiers, paths, CamelCase/ALLCAPS nouns, tokens
/// with digits). The residue is verb/intent-dominated.
///
/// This is the proven intent-separation breakthrough: whole-text 1-bit
/// fingerprints can't tell "add an endpoint" from "document an endpoint" (the
/// nouns drown the verb), but fingerprinting the action residue separately DOES
/// separate intent — cleanly, within the 1-bit substrate, no full floats.
/// Measured in `tests/test_intent_separation.rs`. The caller passes one plain
/// string; `.said` derives the action field with this — zero user effort.
pub fn action_residue(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split_whitespace() {
        let tok = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '/');
        if tok.is_empty() { continue; }
        let first_upper = tok.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        let is_target =
            tok.contains('/') ||                                       // path / route
            tok.contains('_') ||                                       // snake_case
            tok.chars().any(|c| c.is_ascii_digit()) ||                 // has digits
            (first_upper && tok.chars().skip(1).any(|c| c.is_uppercase())) || // CamelCase/ALLCAPS
            tok.chars().filter(|c| c.is_uppercase()).count() >= 2;     // mixed caps
        if !is_target {
            out.push(tok.to_lowercase());
        }
    }
    out.join(" ")
}

/// Generate candidate symbol-name spellings from keyword lists.
/// See the CLI docstring for the full enumeration strategy — matches it
/// verbatim so CLI and MCP produce identical symbol candidates.
pub fn ask_symbol_candidates(lower: &[String], original: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let add = |s: String, seen: &mut HashSet<String>, out: &mut Vec<String>| {
        if !s.is_empty() && seen.insert(s.clone()) { out.push(s); }
    };

    // 1. Raw keywords (lowercased AND original-case)
    for k in lower { add(k.clone(), &mut seen, &mut out); }
    for k in original { add(k.clone(), &mut seen, &mut out); }

    // 2. Adjacent pair joins: snake, flat, camel, pascal
    for i in 0..lower.len().saturating_sub(1) {
        let a = &lower[i];
        let b = &lower[i + 1];
        add(format!("{}_{}", a, b), &mut seen, &mut out);
        add(format!("{}{}", a, b), &mut seen, &mut out);
        add(format!("{}{}", a, capitalize(b)), &mut seen, &mut out);
        add(format!("{}{}", capitalize(a), capitalize(b)), &mut seen, &mut out);
    }

    // 3. Adjacent triple joins
    for i in 0..lower.len().saturating_sub(2) {
        let (a, b, c) = (&lower[i], &lower[i + 1], &lower[i + 2]);
        add(format!("{}_{}_{}", a, b, c), &mut seen, &mut out);
        add(format!("{}{}{}", a, b, c), &mut seen, &mut out);
        add(format!("{}{}{}", a, capitalize(b), capitalize(c)), &mut seen, &mut out);
        add(format!("{}{}{}", capitalize(a), capitalize(b), capitalize(c)), &mut seen, &mut out);
    }

    // 4. Full-chain join (if ≥ 4 keywords)
    if lower.len() >= 4 {
        add(lower.join("_"), &mut seen, &mut out);
        add(lower.join(""), &mut seen, &mut out);
    }

    out
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// How DISTINCTIVE is a symbol-name match — i.e. how confident should Engine A be that hitting this
/// symbol answers the query, vs the name being a common English word that *coincidentally* names a
/// symbol? This is the symbol-engine analogue of Engine B's IDF/rarity guard (see the long comment at
/// ask.rs ~393: "do NOT treat rare common-English words as discriminators — a rare word can
/// coincidentally land in the WRONG doc and out-rank the correct semantic match").
///
/// Measured failure this fixes: a descriptive query "which function builds the wikilink concept graph
/// from frames" generated "frames" as a symbol candidate, hit the trivial symbol `frames` at flat 1.00,
/// and BURIED the real `build_concept_links`. Likewise "the abstention gate" → symbol `threshold`,
/// "the steering hook" → symbol `steering`. All coincidental single-word matches outranking the answer.
///
/// Signal is purely structural + corpus-derived (NO hard-coded stopword list):
///   * a COMPOUND identifier (snake_case, camelCase humps, or a long name) is an intentional, specific
///     symbol the user almost certainly means → distinctiveness 1.0 (full confidence, unchanged).
///   * a single short all-lowercase token that is ALSO a plain dictionary-shaped word is most likely
///     coincidental → discounted toward the grep band so a strong semantic match can win.
/// `query_len` = number of query keywords; a 1-word query that literally IS the symbol name keeps full
/// confidence (the user typed the identifier), but the SAME word buried in a long descriptive query does not.
fn symbol_distinctiveness(name: &str, query_len: usize) -> f32 {
    let has_underscore = name.contains('_');
    // camelCase / PascalCase hump = a lower→upper transition anywhere in the name.
    let has_hump = name.chars().zip(name.chars().skip(1))
        .any(|(a, b)| a.is_ascii_lowercase() && b.is_ascii_uppercase());
    let has_digit = name.chars().any(|c| c.is_ascii_digit());
    let is_compound = has_underscore || has_hump || has_digit || name.len() >= 12;
    if is_compound {
        return 1.0; // intentional identifier — full confidence (the common case, unchanged behaviour)
    }
    // A short single-word lowercase name (frames, threshold, steering, ask). If the user's WHOLE query
    // is essentially this one word, they typed the identifier on purpose → keep it strong. If it's one
    // word inside a longer descriptive question, it's probably coincidental → discount so Engine C wins.
    if query_len <= 1 {
        1.0
    } else {
        // Discount grows with query length: the more descriptive the question, the less a lone
        // common-word symbol match should dominate. Floor keeps it a real (grep-band) candidate, not
        // dropped — if it IS the answer, the float rerank can still surface it.
        (0.92 - 0.06 * (query_len.saturating_sub(1) as f32)).clamp(0.55, 0.92)
    }
}

/// Run the full 3-engine `ask` fusion against a `SaidFile` brain.
///
/// * `brain` — the opened `.said` file
/// * `query` — natural-language question
/// * `top` — max results returned in non-deep mode
/// * `deep` — when true, widens SCA fetch and returns all candidates above cutoff
/// * `scope_doc_ids` — optional tag-scope filter (CLI passes this, MCP can too)
///
/// Returns `(candidates, keywords_lower)`. Caller handles dream/persist.
pub fn ask(
    brain: &mut SaidFile,
    query: &str,
    top: usize,
    deep: bool,
    scope_doc_ids: Option<&HashSet<String>>,
) -> (Vec<AskCandidate>, Vec<String>) {
    let (keywords, keywords_orig) = ask_extract_keywords(query);
    if keywords.is_empty() {
        return (Vec::new(), keywords);
    }

    let mut candidates: HashMap<String, AskCandidate> = HashMap::new();
    let upsert = |cands: &mut HashMap<String, AskCandidate>, c: AskCandidate| {
        match cands.get(&c.doc_id) {
            Some(existing) if existing.confidence >= c.confidence => {}
            _ => { cands.insert(c.doc_id.clone(), c); }
        }
    };

    // ── Engine A — Sym (exact symbol lookup, confidence 1.00) ─────────────
    // `ask` is the RETRIEVAL layer: it returns the exact frame the client asked for, so the LLM can
    // hand it to a language server (rust-analyzer/tsserver/pyright) for the type-precise work — "find
    // all references", "what breaks if I change this signature". `.said` deliberately does NOT walk
    // the call-graph here: that would be a SHALLOW, name-matched (untyped) traversal duplicating the
    // LSP's job, and it would inject possibly-wrong neighbours into normal recall. The call-graph is
    // available as the explicit `code_calls` / `code_callers` verbs the caller invokes WHEN it wants
    // the neighbourhood (see docs/said-structure/06-ingestion-plugins/lsp.md — .said returns stored
    // facts, the LSP resolves types, the LLM orchestrates between them).
    let query_len = keywords.len();
    for cand_name in ask_symbol_candidates(&keywords, &keywords_orig) {
        for sym_hit in brain.sym(&cand_name, 5) {
            if sym_hit.name != cand_name { continue; }
            if let Some(scope) = scope_doc_ids {
                if !scope.contains(&sym_hit.doc_id) { continue; }
            }
            let content = brain.get(&sym_hit.doc_id).unwrap_or_default();
            // Weight by distinctiveness: a compound identifier (build_concept_links) stays at 1.00; a
            // coincidental common-word match (frames/threshold in a long descriptive query) is discounted
            // into the grep band so a stronger semantic answer can win. See symbol_distinctiveness.
            let sym_conf = symbol_distinctiveness(&sym_hit.name, query_len);
            upsert(&mut candidates, AskCandidate {
                doc_id: sym_hit.doc_id.clone(),
                confidence: sym_conf,
                kind: "symbol",
                content,
                location: Some(format!(
                    "{}:{}-{}",
                    sym_hit.kind, sym_hit.start_line, sym_hit.end_line
                )),
            });
        }
    }

    // ── Engine B — Grep (literal keyword match, confidence 0.40 – 0.95) ──
    // Corpus size for IDF: use the indexed doc count (what's actually searchable), which
    // is the right denominator and is reliable regardless of frame-stat bookkeeping.
    let corpus_docs = (brain.engine.core.get_doc_ids().len() as f32).max(1.0);
    for (kw_i, kw) in keywords.iter().enumerate() {
        // Same rule as extraction (ask_extract_keywords): grep short tokens too when they're a
        // discriminator — a digit-bearer ("7") OR a short capitalized label ("C" in "Building C").
        // Without mirroring extraction here, "C" was extracted but never grep'd, so the exact
        // "Building C" note got no discriminator signal and was dropped under its tied cousins.
        let kw_has_digit = kw.chars().any(|c| c.is_ascii_digit());
        let kw_is_label = kw.len() < 3 && keywords_orig.get(kw_i)
            .and_then(|o| o.chars().next())
            .map(|c| c.is_ascii_uppercase()).unwrap_or(false);
        if kw.len() < 3 && !kw_has_digit && !kw_is_label { continue; }
        let hits = brain.grep(kw, 30);
        // Rarity (IDF-ish) of THIS keyword: a token that appears in very few docs is a
        // strong discriminator (a unique id like "vorlex97", a code, a proper noun); one
        // in many docs is common structure. Count WORD-BOUNDARY occurrences, not raw grep
        // hits — grep is substring ("vorlex7" matches "vorlex70"), which would inflate df
        // and wrongly demote a genuinely unique token. df is from the capped hit list,
        // good enough to tell "rare" from "everywhere".
        let df = hits.iter()
            .filter(|h| contains_token(&h.content.to_lowercase(), kw.as_str()))
            .count().max(1) as f32;
        let rarity = (corpus_docs / df).ln().max(0.0) / (corpus_docs.ln().max(1.0)); // 0..~1
        for h in hits {
            if let Some(scope) = scope_doc_ids {
                if !scope.contains(&h.doc_id) { continue; }
            }
            let content_lower = h.content.to_lowercase();
            let terms_present = keywords.iter()
                .filter(|k| contains_token(&content_lower, k.as_str()))
                .count();
            // Normally require ≥2 matched terms (multi-word queries) to suppress
            // single-common-word noise. EXCEPTION: a sufficiently RARE token (high IDF —
            // a unique id, code, or proper noun) is a strong discriminator on its own, so
            // a doc that contains it qualifies even if the query's other (common) words
            // are absent. Without this, "what does Vorlex97 do for fun" rejected the one
            // note containing "vorlex97" because it lacked "fun" — the exact recall@10
            // shortfall on near-template-identical memories.
            let min_terms = if keywords.len() >= 2 { 2 } else { 1 };
            // A discriminator is an IDENTIFIER-like rare token — it carries a digit (a
            // number, code, or alnum id like "7", "office7", "vorlex97"). We deliberately
            // do NOT treat rare *common-English* words as discriminators: in a paraphrase
            // query ("who found the first antibiotic") a rare word can coincidentally land
            // in the WRONG doc and the boost would out-rank the correct semantic match
            // (measured: that regressed recall@10 0.95→0.80). Identifier tokens don't have
            // that failure mode — they only match the doc that literally shares the id.
            // An identifier-class discriminator: carries a digit ("7","REF-0019") OR is a short
            // capitalized LABEL ("C","B" in "Building C"). Both only match the doc that literally
            // shares the token, so boosting them can't mis-fire on paraphrase (a rare *common* word
            // is neither). We pass the ORIGINAL-cased query token so capitalization is visible.
            let kw_orig = keywords_orig.iter().find(|o| o.to_lowercase() == *kw).map(|s| s.as_str()).unwrap_or(kw.as_str());
            let is_identifier = kw.chars().any(|c| c.is_ascii_digit())
                || (kw.len() <= 2 && kw_orig.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false));
            let rare_discriminator = is_identifier && rarity >= 0.85
                && contains_token(&content_lower, kw.as_str());
            if terms_present < min_terms && !rare_discriminator { continue; }
            // Base on how many query terms matched, PLUS a rare-token boost: matching a
            // high-IDF discriminator (rarity→1) is far more informative than matching a
            // common word, so it should out-score a structurally-similar semantic match
            // (~0.6) that lacks the discriminator. Without this, a unique identifier
            // present only via grep (terms_present=1 → 0.40) loses to wrong-but-similar
            // notes (recall@10 shortfall on near-template-identical memories).
            // Only boost when THIS doc actually contains the rare token at a word
            // boundary. grep is substring, so a rare token like "7" also returns
            // "office17"/"office27"; those must NOT get the discriminator boost (it would
            // tie them with the true "office 7" at the 0.95 cap and scramble the order).
            // Shared common words saturate the 0.95 grep ceiling for EVERY near-template twin (100
            // "Invoice reference REF-XXXX covers the March charge" notes all hit 0.95 on the shared
            // words). If the discriminator boost is added UNDER .min(0.95) it's swallowed by the cap,
            // so the exact "REF-0019" note ties its 100 cousins at 0.95 and arbitrary ordering DROPS
            // it from top-K (measured: gold-in-top10 collapsed to 0.19). Fix: a matched rare IDENTIFIER
            // discriminator lifts the doc into a reserved 0.95–0.99 band ABOVE the shared-word ceiling,
            // so the exact-id note leads its boilerplate twins. Identifier-gated (carries a digit) +
            // word-boundary-verified, so it only lifts the doc that LITERALLY shares the id — it can't
            // mis-fire on paraphrase (a rare common word is not an identifier, never enters this band).
            let shared = (0.40 + 0.15 * (terms_present as f32 - 1.0)).min(0.95).max(0.40);
            let confidence = if rare_discriminator {
                (0.95 + 0.04 * rarity).min(0.99)
            } else {
                shared
            };
            upsert(&mut candidates, AskCandidate {
                doc_id: h.doc_id.clone(),
                confidence,
                kind: "text",
                content: h.content,
                location: None,
            });
        }
    }

    // ── Engine C — SCA semantic (recall_fused — BM25 + graph + multi-hop) ─
    let sca_fetch = if deep { 100 } else { 20 };
    let sca_hits = brain.query(query, sca_fetch);
    for (rank, h) in sca_hits.into_iter().enumerate() {
        if let Some(scope) = scope_doc_ids {
            if !scope.contains(&h.doc_id) { continue; }
        }
        let content_lower = h.content.to_lowercase();
        let terms_present = keywords.iter()
            .filter(|k| contains_token(&content_lower, k.as_str()))
            .count();
        // Trust SCA's ranking to the documented recall depth even with no literal
        // keyword overlap (pure paraphrase). Only gate the deeper tail on keywords.
        if rank >= ASK_SCA_TRUST_DEPTH && terms_present == 0 { continue; }
        let base = 0.30 + (h.score * 0.30).clamp(0.0, 0.30);
        let kw_bonus = 0.05 * (terms_present as f32 - 1.0).max(0.0);
        let confidence = (base + kw_bonus).min(0.80);
        // Semantic tie-break (CONDITIONAL): when Engine B (grep) already inserted this
        // doc but its keyword match is NON-DISCRIMINATING — matched on at most one
        // keyword, typically a shared entity like "Mara" that every sibling note also
        // has — add a fraction of the SCA semantic score so the asymmetric ranking
        // (which separates one entity's many memories 5/5) orders the otherwise-tied
        // results. Skipped when terms_present >= 2: a doc with a unique discriminator
        // (e.g. "office" AND "7") has a real lexical lead that must win — this keeps the
        // lexical needle case at 30/30.
        if terms_present <= 1 {
            if let Some(existing) = candidates.get_mut(&h.doc_id) {
                // Non-discriminating keyword match (entity-only): let the asymmetric
                // semantic score be the primary ranking signal among the tied siblings.
                // Blend toward s_sem rather than a tiny nudge — these docs have no
                // lexical signal to lose, so semantic should dominate.
                // Cap at 1.0: a symbol hit (≤1.00) that ALSO greps must not inflate ABOVE the symbol
                // ceiling (measured: coincidental matches reached 1.27, dominating the real answer).
                existing.confidence = (existing.confidence.max(confidence) + h.score * 0.40).min(1.0);
            }
        }
        upsert(&mut candidates, AskCandidate {
            doc_id: h.doc_id.clone(),
            confidence,
            kind: "semantic",
            content: h.content,
            location: None,
        });
    }

    // ── Engine D — wikilink graph traversal (build-graph edges) ──────────
    // For each query keyword, pull notes that carry an explicit `[[keyword]]` concept
    // link (stored as a `link:<kw>` tag at ingest). This is the documented build-graph
    // path (3.9): it reaches a linked note even when the bridge word is absent from its
    // body — the OKF cross-link → .said edge that turns out-of-scope inference
    // ("heart" → a "cardiologist" note linked [[heart]]) into a direct edge hop. Scored
    // as a confident keyword-class hit; additive (never demotes) so it can't regress
    // queries that have no links.
    for kw in &keywords {
        for did in brain.frames_linking_concept(kw) {
            if let Some(scope) = scope_doc_ids {
                if !scope.contains(&did) { continue; }
            }
            let content = brain.get(&did).unwrap_or_default();
            upsert(&mut candidates, AskCandidate {
                doc_id: did,
                confidence: 0.90,   // an explicit concept link is a strong, intentional edge
                kind: "text",
                content,
                location: None,
            });
        }
    }

    // Engine D-2: ENTITY-BRIDGE second hop (the true multi-hop walk). The loop above only follows a
    // concept whose word is in the QUERY. But a 2-hop question — "what is Dr. Lee's team handling?"
    // — matches memory A ("Dr. Lee … [[cardiology]] team") whose ANSWER lives in a SIBLING memory B
    // ("[[cardiology]] team is handling the bypass") that shares A's concept but NOT the query's
    // words. So: take the strongest current seeds, read THEIR OWN concept edges (link: tags, from
    // both [[wikilinks]] AND auto build_concept_links entities), and pull in every sibling that
    // shares one. This makes the bridge DETERMINISTIC — if the edge exists, B is reached; it cannot
    // depend on B happening to rank high by similarity. Additive (never demotes) and scoped-aware;
    // bounded so a hub concept can't flood the result set.
    //
    // NO hard-coded confidence thresholds decide WHETHER to follow: a `link:` edge is binary truth —
    // if a matched candidate carries one, its siblings are reachable, full stop. We pull them in as
    // `semantic`-kind candidates so the LATENT-SPACE float rerank below (line ~552, full 64-dim cosine
    // on the re-encoded query) is what RANKS them — the encoder decides how good the bridge answer is,
    // not a magic number. The only bounds are anti-flood caps (a hub concept linking hundreds of
    // frames must not swamp the result set); those are size limits, not signal thresholds.
    {
        const MAX_SEED_FOLLOW: usize = 4;     // follow the few best current candidates' edges
        const MAX_BRIDGE_PER_CONCEPT: usize = 8; // a single concept can't contribute more than this
        // The candidates that matched the query so far — follow the edges of the strongest few (by
        // current coarse score) purely to bound work; we do NOT threshold on the score value.
        let mut seeds: Vec<(String, f32)> = candidates.values()
            .map(|c| (c.doc_id.clone(), c.confidence)).collect();
        seeds.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        seeds.truncate(MAX_SEED_FOLLOW);
        // Coarse score of the strongest seed — used ONLY to stamp bridged siblings at a value in the
        // same band so they survive the relative cutoff and ENTER the rerank pool; the rerank then
        // reorders everything by true latent similarity. (If rerank doesn't fire — e.g. an all-text
        // result — this keeps the bridge just under the seed so it never displaces a direct answer.)
        let seed_top = seeds.first().map(|(_, c)| *c).unwrap_or(0.0);
        let bridge_stamp = (seed_top - 0.001).max(0.0);
        for (seed_id, _) in seeds {
            // Read the seed's own concept edges (`link:` tags — from [[wikilinks]] AND auto entities).
            // Collected first so the immutable meta borrow is released before get()/linking calls.
            let concepts: Vec<String> = brain.frames.get_meta(&seed_id)
                .map(|m| m.tags.iter()
                    .filter_map(|t| t.strip_prefix("link:").map(|c| c.to_string()))
                    .collect())
                .unwrap_or_default();
            for concept in concepts {
                let mut added = 0usize;
                for sib in brain.frames_linking_concept(&concept) {
                    if sib == seed_id { continue; }
                    if added >= MAX_BRIDGE_PER_CONCEPT { break; }
                    if let Some(scope) = scope_doc_ids {
                        if !scope.contains(&sib) { continue; }
                    }
                    // A followed `link:` edge is a DETERMINISTIC fact. If the sibling is already a
                    // candidate (e.g. a weak semantic hit the rerank/gap would later drop), UPGRADE it
                    // to the deterministic bridge edge rather than skipping — otherwise the edge truth
                    // is lost to the low fuzzy score. If new, add it. Either way it becomes a keyword-
                    // class ("text") hit at bridge_stamp: never floored/gap-dropped, NOT rescored by
                    // query-cosine (a true bridge answer has LOW direct similarity by definition), and
                    // pinned just below the direct hits so it can't displace a real answer.
                    let existing_conf = candidates.get(&sib).map(|c| c.confidence).unwrap_or(0.0);
                    if existing_conf >= bridge_stamp { continue; } // already stronger — leave it
                    let content = brain.get(&sib).unwrap_or_default();
                    candidates.insert(sib.clone(), AskCandidate {
                        doc_id: sib,
                        confidence: bridge_stamp,
                        kind: "text",
                        content,
                        location: None,
                    });
                    added += 1;
                }
            }
        }
    }

    // ── Merge + relative cutoff + truncate ───────────────────────────────
    let mut results: Vec<AskCandidate> = candidates.into_values().collect();
    results.sort_by(|a, b| {
        b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Content-dedup: the same fact stored under several ids would otherwise fill
    // several top-K slots with identical text — wasting the result budget and (when
    // fed to an LLM) the context window. Results are sorted by confidence, so keeping
    // the FIRST occurrence of each content signature keeps the highest-confidence copy.
    // Signature = trimmed, whitespace-collapsed, lowercased content (so trivial
    // formatting differences still collapse). Distinct memories are untouched.
    {
        let mut seen: HashSet<String> = HashSet::new();
        results.retain(|r| {
            let sig: String = r.content.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            seen.insert(sig)
        });
    }

    let top_score = results.first().map(|r| r.confidence).unwrap_or(0.0);
    let cutoff = top_score * ASK_RELATIVE_CUTOFF;

    let max_results = if deep { usize::MAX } else { top };
    let mut kept: Vec<AskCandidate> = results.into_iter()
        .enumerate()
        .filter(|(i, r)| *i < ASK_SCA_GUARANTEED || r.confidence >= cutoff)
        .map(|(_, r)| r)
        .take(max_results)
        .collect();

    // Full-float rerank of the returned set (QJL near-collision recovery, 14.1/14.8).
    // The 1-bit doc fingerprint loses per-dim magnitude, so among near-collision
    // candidates (e.g. one entity's many memories) the coarse score ties or mis-orders
    // — yet full 64-dim float cosine on the SAME embedding separates them cleanly
    // (proven 5/5, test_float_rerank_value). We re-encode the query + each kept
    // candidate's stored text (~80µs each, only the handful we return — no storage
    // change) and reorder by cosine. Sym hits (exact symbol, confidence 1.0) are pinned
    // above the semantic rerank so code/entity lookups keep their precedence.
    // Gate: only rerank when the result set is SEMANTIC-led — i.e. no candidate earned
    // its place via a discriminating multi-keyword lexical match. When a unique lexical
    // discriminator exists (e.g. "office" AND "7" → the exact note), that lexical order
    // is authoritative and float cosine would wrongly tie near-identical texts. The
    // needle/lexical case has multi-keyword `text` hits; the same-entity paraphrase case
    // has only `semantic` hits sharing one generic entity token.
    let lexically_discriminated = kept.iter().any(|c| c.kind == "text" && c.confidence > 0.55);
    let has_semantic = kept.iter().any(|c| c.kind == "semantic");
    if kept.len() > 1 && has_semantic {
        if let Some(q_emb) = brain.engine.encode_query(query) {
            let cos = |a: &[f32], b: &[f32]| -> f32 {
                let (mut d, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
                for i in 0..a.len().min(b.len()) { d += a[i]*b[i]; na += a[i]*a[i]; nb += b[i]*b[i]; }
                d / (na.sqrt().max(1e-12) * nb.sqrt().max(1e-12))
            };
            // Anisotropy correction ("all-but-the-top", Mu & Viswanath 2018). Static
            // mean-pooled embeddings live in a narrow cone: every pair already has cosine
            // ~0.45, so a real match barely out-scores noise NUMERICALLY even when it
            // ranks first. Subtracting the corpus mean removes that shared common
            // component and spreads the band — a true match stays high while off-topic
            // docs drop toward/below zero. It's a fixed global shift, so it preserves
            // within-query ranking (recall@k unchanged) while making the score itself
            // meaningful/thresholdable. Measured: relevant top ~0.55 vs irrelevant top
            // ~0.10 (test_score_separation). Falls back to raw cosine if no corpus mean.
            // Diagonal whitening: (v-μ)/σ. Centering removes the shared common component
            // (the cone); dividing by the per-dimension std additionally down-weights the
            // few high-variance directions that dominate raw dot products and carry generic
            // structure/co-occurrence rather than topical meaning. Measured to widen the
            // relevant-vs-offtopic gap over plain centering (test_whitening_probe: a
            // lexical-coincidence off-topic top drops 0.39→0.29 while genuine matches hold
            // ~0.46-0.59). Full ZCA was measured WORSE here (it inflates low-variance noise
            // dims of the 64-dim Matryoshka), so diagonal is the right level. σ already
            // computed at index time (corpus_std); falls back to centering if absent.
            let mu = brain.engine.core.get_corpus_mean();
            let sd = brain.engine.core.get_corpus_std();
            let whiten = |v: &[f32]| -> Vec<f32> {
                if mu.len() == v.len() {
                    v.iter().enumerate()
                        .map(|(d, x)| (x - mu[d]) / sd.get(d).copied().unwrap_or(1.0).max(1e-6))
                        .collect()
                } else { v.to_vec() }
            };
            let q_c = whiten(&q_emb);
            // Brain recency/salience signal (Layer 9, docs 3.3): newer + more-recalled
            // memories rank higher; cold ones fade — like a brain adding and forgetting.
            // search_internal applies this BEFORE the rerank; without folding it back in,
            // reordering purely by cosine would ERASE it and near-identical memories ("5
            // team-meeting notes", "Devi's electrician vs Alex's electrician") would tie
            // arbitrarily instead of letting recency/salience pick the right one.
            let s_slow = brain.engine.brain.s_slow_read(&q_emb);
            let s_slow_boost = if s_slow > 0.1 { 1.0 + (s_slow * 0.01).min(0.5) } else { 1.0 };
            let mut scored: Vec<(f32, bool, AskCandidate)> = kept.into_iter().map(|c| {
                // Pin ABOVE the semantic rerank ONLY a DISTINCTIVE symbol hit (a compound identifier the
                // user clearly meant — confidence kept at the symbol ceiling). A symbol hit that
                // Engine A already DISCOUNTED (a coincidental common-word match like `frames`/`threshold`
                // in a long descriptive query — see symbol_distinctiveness) must NOT pin; it competes on
                // cosine like any candidate so a stronger semantic answer (the real `build_concept_links`)
                // can lead. Without this gate the pin (sort by is_sym first) re-floats the coincidental
                // symbol to rank-0 even after the confidence discount.
                let is_sym = c.kind == "symbol" && c.confidence >= 0.95;
                // Prefer the STORED doc embedding (the exact indexed 64-dim vector) over
                // re-encoding the displayed content — re-encoding can drift from what was
                // indexed (truncated/modified content) and loses fidelity. Fall back to
                // re-encode only if the doc has no cached embedding.
                let doc_emb = brain.engine.core.get_embedding(&c.doc_id).cloned()
                    .or_else(|| brain.engine.encode_query(&c.content));
                let cosine = doc_emb.map(|e| cos(&q_c, &whiten(&e))).unwrap_or(0.0);
                // Meaning leads (cosine), the brain breaks ties (recall_weight × recency).
                // recall_weight is 1.0–2.0, s_slow_boost 1.0–1.5 — a multiplicative nudge,
                // so the right MEANING still dominates but among near-identical matches the
                // newest/most-salient wins (the documented Layer-9 behavior).
                let recall_w = brain.engine.brain.get_recall_weight(&c.doc_id);
                let s = cosine.max(0.0) * recall_w * s_slow_boost;
                (s, is_sym, c)
            }).collect();
            // Reorder by combined score ONLY when there is no authoritative lexical
            // discriminator. When a unique multi-keyword `text` hit leads (needle case),
            // that order is authoritative and we must not reshuffle it — we still rescore
            // the semantic tail's confidence + gate it below, just without reordering.
            if !lexically_discriminated {
                // Sym first (precise lookups), then by combined cosine×brain score.
                scored.sort_by(|a, b| {
                    b.1.cmp(&a.1).then(b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal))
                });
            }

            // Promote the combined (cosine × brain) score to the candidate's confidence
            // for the SEMANTIC hits we just rescored. The cosine separates meaning out of
            // the anisotropy-collapsed 1-bit band; the brain multiplier keeps recency/
            // salience as the tie-break. Sym and `text` (keyword) hits keep their own
            // confidence — they were never in the collapsed band.
            for (s, is_sym, c) in scored.iter_mut() {
                if !*is_sym && c.kind == "semantic" {
                    c.confidence = *s;
                }
            }

            // Background distribution for z-score abstention (below). The centered cosines
            // of ALL reranked semantic candidates are this query's similarity distribution;
            // a genuine answer sits several σ above it, an off-topic query is a flat cluster
            // near its own mean. Captured here BEFORE floor/gap filtering so the stats
            // reflect the full background, not the survivors. N-independent: it's relative
            // to this query's own spread, so it behaves the same at N=20 and N=400.
            let sem_scores: Vec<f32> = scored.iter()
                .filter(|(_, is_sym, c)| !*is_sym && c.kind == "semantic")
                .map(|(s, _, _)| *s).collect();
            let (bg_mean, bg_std) = if sem_scores.len() >= 4 {
                let m = sem_scores.iter().sum::<f32>() / sem_scores.len() as f32;
                let var = sem_scores.iter().map(|s| (s - m).powi(2)).sum::<f32>()
                    / sem_scores.len() as f32;
                (m, var.sqrt().max(1e-6))
            } else { (0.0, -1.0) }; // std<0 => not enough data, skip z-gate

            kept = scored.into_iter().map(|(_, _, c)| c).collect();

            // Abstention floor on the now-separated semantic score. An off-topic query in
            // any size brain produces only weak centered cosines (~0.10); a real match
            // clears ~0.25 comfortably. Drop semantic hits below the floor so `ask`
            // returns the confident answer(s) — or nothing — instead of the whole brain.
            // Ranking-preserving and N-independent (centering is global), so it does NOT
            // recreate the recall@10 regression a raw-confidence floor caused. Sym/text
            // hits are never floored. Tunable via SAID_ASK_FLOOR.
            // SAID_ASK_ABSTAIN=0 disables the whole floor/gap/abstention block (A/B probe:
            // documented `ask` returns top-K via relative cutoff + ASK_SCA_GUARANTEED only).
            let abstain_on = std::env::var("SAID_ASK_ABSTAIN").map(|v| v != "0").unwrap_or(true);
            if !deep && abstain_on {
                let floor: f32 = std::env::var("SAID_ASK_FLOOR").ok()
                    .and_then(|v| v.parse().ok()).unwrap_or(0.05);
                let any_strong = kept.iter().any(|c| c.kind != "semantic" || c.confidence >= floor);
                if any_strong {
                    // Confident answer(s) exist — keep those, drop the weak semantic tail.
                    kept.retain(|c| c.kind != "semantic" || c.confidence >= floor);
                } else {
                    // Nothing clears the floor: the brain has no confident match. Return
                    // only the single best guess rather than the whole brain, so an
                    // off-topic question yields one closest memory (or, with the relative
                    // cutoff upstream, possibly none) instead of every memory as noise.
                    kept.truncate(1);
                }

                // Relative gap on the now-separated centered scores. With anisotropy
                // removed, a confident query has a clear leader and the rest fall away;
                // an off-topic small-brain query is a flat low cluster. Drop semantic
                // hits that trail the leader by more than `gap` — this trims the "returns
                // the whole brain" tail that the floor alone can't (legit at-scale hard
                // matches score as low as ~0.05, overlapping small-brain noise, so the
                // floor stays low; the GAP catches the flat cluster instead). Only fires
                // when the leader is itself reasonably strong, so it never thins a genuine
                // multi-answer result set where everything is high. Tunable via SAID_ASK_GAP.
                let gap: f32 = std::env::var("SAID_ASK_GAP").ok()
                    .and_then(|v| v.parse().ok()).unwrap_or(0.20);
                if let Some(top) = kept.first().map(|c| c.confidence) {
                    if top >= floor {
                        kept.retain(|c| c.kind != "semantic" || (top - c.confidence) <= gap);
                    }
                }

                // Abstention via per-query z-score ("no confident answer"). A static
                // absolute floor can't work here: a hard paraphrase match and off-topic
                // noise both score ~0.3-0.45 centered (they overlap), so any constant
                // threshold either lets noise through or cuts real matches (measured: 0.45
                // regressed recall@volume to 0.40). The calibrated signal is RELATIVE — how
                // many σ the top hit stands above THIS query's own similarity distribution
                // (Mu&Viswanath geometry + QPP-style calibration). A genuine answer is a
                // clear outlier (high z); an off-topic query is a flat cluster where even
                // the top sits near the mean (low z). N-independent and ranking-preserving.
                // Only abstains when the leader is a semantic hit with no strong lexical
                // support, so a keyword/symbol answer is never suppressed. Tunable via
                // SAID_ASK_ZMIN; std<0 means too few candidates to calibrate → skip.
                let z_min: f32 = std::env::var("SAID_ASK_ZMIN").ok()
                    .and_then(|v| v.parse().ok()).unwrap_or(1.0);
                let has_strong_lexical = kept.iter()
                    .any(|c| c.kind != "semantic" && c.confidence >= 0.55);
                if bg_std > 0.0 && !has_strong_lexical {
                    if let Some(top) = kept.first() {
                        if top.kind == "semantic" {
                            let z = (top.confidence - bg_mean) / bg_std;
                            if z < z_min { kept.clear(); }
                        }
                    }
                }

                // Existence abstention ("do I have any memory about X?") — THRESHOLD-FREE.
                // The z-score above is RELATIVE and, by design (ZMUV forgives a lone outlier), it
                // PASSES the exact failure we must reject: in a small/flat brain an off-topic query
                // can have one frame stand >1σ above the mean at a LOW absolute score. A constant
                // cosine floor (the old SAID_ASK_MINCONF=0.6x) catches it but is a hard-coded magic
                // number that won't transfer across corpora/encoders. The literature's fix (QPP: NQC,
                // Shtok&Kurland TOIS'12; Lowe's ratio test, IJCV'04) is two SCALE-FREE shape signals,
                // each a ratio over THIS query's own score range so nothing absolute is baked in:
                //   gap        = (top1 − top2) / (top1 − min)   — leadership (Lowe ratio): a real
                //                answer has a clear leader; a no-answer query is a flat tie (gap→0).
                //   commitment = σ / (top1 − min)               — NQC: a committed list is peaked;
                //                an off-topic blob is flat (commitment→0).
                // Abstain only when BOTH are weak (OOD work — KNN-OOD ICML'22, NNGuide ICCV'23 — shows
                // neither leadership nor spread alone suffices). Same guard as z (semantic leader, no
                // strong lexical), so keyword/symbol answers and high-confidence multi-answer sets are
                // never touched. The two shape parameters are unit-free "how clear must the leader be"
                // knobs (≈ Lowe's 1−0.8), NOT cosine thresholds; off unless SAID_ASK_ABSTAIN_SHAPE=1
                // so existing callers stay byte-identical, while the gap/commitment MATH is corpus-
                // independent — no per-corpus retuning. Tunable via SAID_ASK_GAPMIN / SAID_ASK_COMMITMIN.
                let shape_on = std::env::var("SAID_ASK_ABSTAIN_SHAPE").map(|v| v == "1").unwrap_or(false);
                if shape_on && !has_strong_lexical {
                    // semantic scores in rank order (kept is already sorted; confidences are the
                    // reranked cosines for semantic hits).
                    let sem: Vec<f32> = kept.iter()
                        .filter(|c| c.kind == "semantic").map(|c| c.confidence).collect();
                    if sem.len() >= 3 && kept.first().map(|c| c.kind == "semantic").unwrap_or(false) {
                        let top1 = sem[0];
                        let top2 = sem[1];
                        let smin = sem.iter().cloned().fold(f32::INFINITY, f32::min);
                        let range = (top1 - smin).max(1e-6);
                        let gap = (top1 - top2) / range;             // Lowe leadership
                        let commitment = bg_std / range;             // NQC commitment (σ over range)
                        let gap_min: f32 = std::env::var("SAID_ASK_GAPMIN").ok()
                            .and_then(|v| v.parse().ok()).unwrap_or(0.30);
                        let commit_min: f32 = std::env::var("SAID_ASK_COMMITMIN").ok()
                            .and_then(|v| v.parse().ok()).unwrap_or(0.30);
                        let shape_flat = gap < gap_min && commitment < commit_min;
                        // LEXICAL GROUNDING veto (the orthogonal signal score-shape is blind to). In a
                        // NOISY mixed corpus an off-topic query ("wifi password at the lodge", no answer)
                        // can still have ONE weak semantic hit standing slightly proud of the blob, so
                        // the shape isn't flat enough and the gate misses — and the brain returns a
                        // proximity ARTIFACT (e.g. a Penicillin note) that shares NONE of the query's
                        // words. The fix (COIL/Clarity; "exact lexical match carries relevance dense
                        // similarity discards") is a BINARY, corpus-derived grounding test: does the top
                        // hit share ≥1 query content-term? It's a set-intersection — NO magnitude
                        // threshold, transfers across corpora/encoders. Abstain only when shape is flat
                        // AND the top hit is UNGROUNDED (the proximity artifact). A real weak answer
                        // shares a term (grounded → kept); a peaked paraphrase passes the shape test.
                        let top_grounded = kept.first().map(|c| {
                            let lc = c.content.to_lowercase();
                            keywords.iter().any(|k| contains_token(&lc, k.as_str()))
                        }).unwrap_or(false);
                        if shape_flat && !top_grounded {
                            kept.clear(); // flat blob AND no lexical anchor → embedding-proximity artifact
                        }
                    }
                }
            }
        }
    }

    // Final LEXICAL-GROUNDING veto (existence abstention, threshold-free). The per-query shape gate
    // above only governs the SEMANTIC engine's tail; in a noisy mixed corpus a no-answer query can
    // still surface a weak hit from ANY engine (e.g. a near-duplicate that spiked). After the full
    // result is assembled, if NOTHING in the kept set shares a query content-term (zero lexical
    // grounding across the whole answer) AND nothing is a strong exact lexical/symbol hit, the result
    // is an embedding-proximity artifact — there is no real answer, so abstain. Binary set-overlap,
    // no magnitude threshold (COIL/Clarity). Opt-in via SAID_ASK_ABSTAIN_SHAPE so default callers are
    // byte-identical; a genuine paraphrase answer that shares no surface term is preserved by the
    // "strong lexical/symbol hit" carve-out and by the fact that this only fires when the shape gate
    // is requested (the same callers that already accept shape-based abstention).
    if std::env::var("SAID_ASK_ABSTAIN_SHAPE").map(|v| v == "1").unwrap_or(false) && !kept.is_empty() {
        let any_grounded = kept.iter().any(|c| {
            let lc = c.content.to_lowercase();
            keywords.iter().any(|k| contains_token(&lc, k.as_str()))
        });
        let any_strong_lexical = kept.iter().any(|c| c.kind != "semantic" && c.confidence >= 0.55);
        if !any_grounded && !any_strong_lexical {
            kept.clear();
        }
    }

    // Auto-dream — intrinsic to recall, fired HERE in core so EVERY caller (CLI, MCP,
    // Rust API, orchestrator) gets identical brain-state evolution. Previously each
    // caller duplicated this trigger; SaidFile::maybe_dream is the single source of
    // truth now. Pure math, no LLM, no caller action.
    brain.maybe_dream();

    (kept, keywords)
}

// ── Coding-fix recall: ONE scorer, shared by said-cli and said-orchestration ──
// Previously the CLI (`best_fix_for`) and the orchestrator (`best_iteration`) used
// DIFFERENT scorers that disagreed (the same match scored 0.53 in one, 0.95 in the
// other). This is the single source of truth: the ask chain for the neighborhood +
// `.said`'s 1-bit hierarchical SEMANTIC fingerprint of the problem to pick the right
// one within it (separates near-twins by meaning, not shared words).

/// Marker strings for the stored coding-fix frames. ONE source of truth so the CLI
/// (`learn-fix`/`recall-fix`), the MCP tools, and said-orchestration::learn all write
/// and read byte-compatible frames into the SAME learning store.
pub const FIX_KIND_TAG: &str = "coding-fix";
pub const FIX_ACTION_TAG: &str = "coding-fix-action";
pub const FIX_PILLAR_TAG: &str = "pillar:procedural";
pub const FIX_SUCCESS_TAG: &str = "procedural:outcome=success";
pub const FIX_ACTION_ID_PREFIX: &str = "fixaction::";
const FIX_EDITS_SEP: &str = "\n<<<SAID-FIX-EDITS>>>\n";
const FIX_ACTION_SEP: &str = "\n<<<SAID-FIX-ACTION>>>\n";

/// A recalled verified coding-fix: the full human-readable note (the story an LLM
/// reloads), the verified change-set JSON, and the match score.
pub struct RecalledFix {
    pub doc_id: String,
    pub score: f32,
    /// Everything before the machine payload — TASK + FILES/STEPS/ERRORS/LEARNINGS.
    pub note: String,
    /// The stored verified change-set JSON (the edits that built+passed).
    pub edits_json: String,
    /// The fix's language from its `lang:` meta tag, if any (None = untagged/agnostic).
    pub lang: Option<String>,
}

/// 16-hex-char BLAKE3 of the body for the frame doc_id — the canonical `.said`
/// content hash (same as ingest/dedup/frame-checksums use). NOT FNV: the whole
/// protocol is blake3, and a divergent hash here would be a latent footgun.
fn fix_hash(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().as_str()[..16].to_string()
}

/// Stable identity for a coding task — the canonical dedup key. Two learnings for the
/// SAME task resolve to the SAME identity (so the new one supersedes the old), while a
/// genuinely different task resolves to a different one.
///
/// - If `label` is a clean stable task-id (no spaces, e.g. "lru_cache", "javascript/lru"),
///   it IS the identity — the factory/CLI can pin one frame per task explicitly.
/// - Otherwise, derive from the problem: lowercase, collapse whitespace, strip trailing
///   punctuation. This is intentionally NOT the full body (note/edits vary per solve) and
///   NOT the loose action-residue (too coarse — would merge distinct tasks). It's the
///   normalized problem statement, which is stable across re-solves of the same task.
pub fn task_identity(problem: &str, label: Option<&str>) -> String {
    if let Some(l) = label {
        let l = l.trim();
        if !l.is_empty() && !l.contains(char::is_whitespace) {
            return format!("task-id:{}", l.to_lowercase());
        }
    }
    let norm: String = problem
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .to_string();
    format!("problem:{}", norm)
}

/// Assemble the coding-fix frame body: a TASK line (recall key) + the human note
/// (verbatim) + the machine payload (edits + intent residue), joined by the markers.
/// `note` is the full human-readable story (may already start with sections); we
/// prepend `TASK:` so `problem` always round-trips.
fn fix_body(problem: &str, note: &str, edits_json: &str, action: &str) -> String {
    let mut body = format!("TASK: {}\n\n", problem.trim());
    body.push_str(note.trim());
    body.push_str(FIX_EDITS_SEP);
    body.push_str(edits_json.trim());
    body.push_str(FIX_ACTION_SEP);
    body.push_str(action.trim());
    body
}

/// The full human note (everything before the machine payload markers).
pub fn fix_note(body: &str) -> String {
    body.split(FIX_EDITS_SEP).next().unwrap_or(body).trim().to_string()
}

/// The stored change-set JSON (between the edits and action markers).
pub fn fix_edits(body: &str) -> String {
    let after = match body.split_once(FIX_EDITS_SEP) {
        Some((_, b)) => b,
        None => return String::new(),
    };
    after.split(FIX_ACTION_SEP).next().unwrap_or(after).trim().to_string()
}

/// LEARN — store a verified coding iteration into the shared learning store.
/// `note` is the human-readable story (sections like FILES/STEPS/ERRORS/LEARNINGS,
/// or a full 10-section iteration note); `edits_json` is the verified change-set.
/// Writes the coding-fix frame + its action-residue companion (for intent matching),
/// rebuilds the index, and returns the doc_id. ONLY call after a green gate — the
/// stored outcome is always success. Shared by CLI, MCP, and orchestration so every
/// caller contributes to ONE store the others recall from.
pub fn learn_coding_fix(
    brain: &mut SaidFile,
    problem: &str,
    note: &str,
    edits_json: &str,
    label: Option<&str>,
) -> String {
    let action = action_residue(problem);
    let body = fix_body(problem, note, edits_json, &action);
    // STABLE TASK IDENTITY for canonical dedup. The doc_id keys on the TASK identity, not
    // the (LLM-authored, varying) note/edits — so re-learning the same task SUPERSEDES the
    // existing frame (put() with the same doc_id tombstones the old one) instead of piling
    // up near-duplicate frames that pollute recall and can outrank the original. Identity =
    // an explicit `label` when it's a stable task-id (e.g. "lru_cache"), else the
    // normalized problem text. A genuinely different problem → different id → distinct
    // frame. Override OFF (legacy body-hash, allows duplicates) via SAID_LEARN_BODY_ID=1.
    let id16 = if std::env::var("SAID_LEARN_BODY_ID").is_ok() {
        fix_hash(&body)
    } else {
        fix_hash(&task_identity(problem, label))
    };
    let doc_id = format!("fix::{}", id16);
    // Native PROCEDURAL pillar (not just the tag): a coding-fix is an action
    // sequence with an outcome, so it must live in the Procedural pillar so
    // recall_by_pillar(Procedural) finds it — `remember_as` would leave it in the
    // default pillar with only a tag. Tags carried alongside for filtering.
    let mut tags = vec![
        FIX_PILLAR_TAG.to_string(),
        FIX_SUCCESS_TAG.to_string(),
        FIX_KIND_TAG.to_string(),
    ];
    if let Some(l) = label {
        if !l.trim().is_empty() {
            tags.push(format!("pr:{}", l.trim()));
        }
    }
    // LANGUAGE TAG (per-language guarantee): derive the fix's language from the file
    // extensions in its change-set and store it as a first-class meta tag, so recall can
    // HARD-FILTER by language (a Python task never receives a C#/JS fix). Stored at learn
    // time so every future fix is tagged regardless of caller (CLI/MCP/orchestrator).
    // No detectable extension => no tag => the fix stays language-agnostic (recalls for any
    // language), which is correct for genuinely language-neutral fixes.
    if let Some(lang) = lang_from_edits(edits_json) {
        tags.push(format!("lang:{}", lang));
    }
    brain.remember_with_pillar(
        Some(&doc_id), &body, Some(FIX_KIND_TAG),
        crate::frames::Pillar::Procedural, tags,
    );
    // Action-residue companion: its content is ONLY the intent residue, so its
    // fingerprint reflects WHAT IS BEING DONE, not the target nouns. Also Procedural.
    if !action.is_empty() {
        let action_id = format!("{}{}", FIX_ACTION_ID_PREFIX, id16);
        brain.remember_with_pillar(
            Some(&action_id), &action, Some(FIX_ACTION_TAG),
            crate::frames::Pillar::Procedural, vec![FIX_ACTION_TAG.to_string()],
        );
    }
    let _ = brain.build_index();
    doc_id
}

/// RECALL — the best verified fix for `problem`, or None below `min_score`. Uses the
/// shared semantic scorer ([`best_coding_fix`]). Caller falls through to its LLM on
/// None. Shared by CLI `recall-fix`, MCP, and orchestration so all see the same store.
pub fn recall_coding_fix(brain: &mut SaidFile, problem: &str, min_score: f32) -> Option<RecalledFix> {
    recall_coding_fixes(brain, problem, 1, min_score).into_iter().next()
}

/// RECALL TOP-K — the K best verified fixes clearing `min_score`, highest first. The
/// semantic top-k contract (measured recall@5 = 100% at 1000 records): a caller can
/// inject several candidates and let the model pick/adapt, rescuing cases where the
/// right learning isn't rank-1. `recall_coding_fix` is the k=1 wrapper.
pub fn recall_coding_fixes(brain: &mut SaidFile, problem: &str, k: usize, min_score: f32) -> Vec<RecalledFix> {
    // LANGUAGE GUARANTEE for per-language packs. Recall scores on the PROBLEM TEXT only,
    // so a near-identically worded task in another language ("Email value object with
    // value equality") can pull a C# frame for a Python task (measured: 0.77). For a
    // per-language product that bleed is unacceptable, so when SAID_RECALL_LANG is set we
    // HARD-FILTER to frames whose stored `lang:<x>` token matches — a C# fix becomes
    // literally unreachable for a Python task, by construction, not by wording luck.
    // Frames with NO `lang:` token (legacy/general) are kept (they're language-agnostic).
    // Unset => no constraint (back-compat). Over-fetch k*4 so the post-filter still fills k.
    let lang_want = std::env::var("SAID_RECALL_LANG").ok()
        .map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty());
    let fetch_k = if lang_want.is_some() { k.saturating_mul(4).max(k) } else { k };
    best_coding_fixes(brain, problem, fetch_k).into_iter()
        .filter(|(_, score)| *score >= min_score)
        .map(|(doc_id, score)| {
            let body = brain.get(&doc_id).unwrap_or_default();
            // Language signal, in priority order: first-class `lang:` META TAG (written at
            // learn time on every new fix), else a `lang:` token in the body (hand-built
            // packs), else infer from the change-set's file extensions (covers the 794
            // legacy frames stored before learn-time tagging existed — no backfill needed).
            let meta_lang = brain.frames.get_meta(&doc_id).and_then(|m| {
                m.tags.iter().find_map(|t| t.strip_prefix("lang:"))
                    .map(|l| l.to_ascii_lowercase())
            });
            RecalledFix { note: fix_note(&body), edits_json: fix_edits(&body), doc_id, score, lang: meta_lang }
        })
        .filter(|fix| match &lang_want {
            None => true,
            Some(want) => {
                let have = fix.lang.clone()
                    .or_else(|| frame_lang(&fix.note))
                    .or_else(|| lang_from_edits(&fix.edits_json));
                match have {
                    Some(have) => &have == want, // known language: must match the active one
                    None => true,                // truly language-agnostic: keep
                }
            }
        })
        .take(k.max(1))
        .collect()
}

/// Parse the stored `lang:<x>` token from a coding-fix frame body (e.g. a curated FILES
/// line `src:context7 lang:csharp area:architecture arch:ddd`). Lower-cased; None when the
/// frame body carries no language token. This is the body-text language signal; recall ALSO
/// checks the first-class `lang:` meta tag (see recall_coding_fixes) so both old hand-tagged
/// packs and new learn-time-tagged fixes are covered.
fn frame_lang(body: &str) -> Option<String> {
    body.split_whitespace()
        .find_map(|tok| tok.strip_prefix("lang:"))
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty())
}

/// Map a source-file extension to the canonical `lang:` token. ONE source of truth shared
/// by learn (tagging) and any caller; keep in lock-step with the orchestrator's detector.
pub fn ext_to_lang(ext: &str) -> Option<&'static str> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "cs" | "csx" | "csproj" => "csharp",
        "py" | "pyi" => "python",
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "go" => "go",
        "java" => "java",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "cpp",
        "c" | "h" => "c",
        _ => return None,
    })
}

/// Derive the language of a coding-fix from the file paths in its change-set JSON. Returns
/// the first recognized language across the edits' `"file":"..."` fields (fixes are
/// single-language in practice). None when no edit has a known extension.
fn lang_from_edits(edits_json: &str) -> Option<String> {
    // Try structured parse first (array of edits, or a single edit object). If that fails
    // (older/looser stored forms), fall back to scanning the raw text for any "file":"..."
    // (and "path") values — robust to schema drift across the frames written over time.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(edits_json) {
        let edits: Vec<&serde_json::Value> = match &v {
            serde_json::Value::Array(a) => a.iter().collect(),
            serde_json::Value::Object(_) => vec![&v],
            _ => vec![],
        };
        for e in edits {
            for key in ["file", "path", "filename", "target"] {
                if let Some(p) = e.get(key).and_then(|f| f.as_str()) {
                    if let Some(lang) = ext_to_lang(p.rsplit('.').next().unwrap_or("")) {
                        return Some(lang.to_string());
                    }
                }
            }
        }
    }
    // Raw-text fallback: pull every `"file":"...ext"` / `"path":"...ext"` token.
    for marker in ["\"file\":\"", "\"path\":\""] {
        let mut rest = edits_json;
        while let Some(i) = rest.find(marker) {
            let after = &rest[i + marker.len()..];
            let end = after.find('"').unwrap_or(after.len());
            let path = &after[..end];
            if let Some(lang) = ext_to_lang(path.rsplit('.').next().unwrap_or("")) {
                return Some(lang.to_string());
            }
            rest = &after[end.min(after.len())..];
        }
    }
    None
}

/// The single coding-fix scorer. Returns the best-matching coding-fix `doc_id` and
/// its score in [0,1], or None if there are no coding-fix candidates. Caller applies
/// its own confidence floor. Used by BOTH the CLI `recall-fix` and the orchestrator.
///
/// Design: RIDE the world-class `ask` chain (the documented retrieval pipeline —
/// SCA + routed BM25/IDF + entity boost + graph fan-out, MTEB 0.9655) to get the
/// right NEIGHBORHOOD of coding-fix candidates. Then pick the right one WITHIN it by
/// `.said`'s own SEMANTIC signal: the 1-bit hierarchical fingerprint of the full
/// PROBLEM text, scored pure-semantic (`rank_by_fingerprint` forces the PureSemantic
/// route — alpha 0.0, Hamming distance only, the paraphrase route from 3.1). This is
/// what separates near-twins lexical overlap cannot: "LRU cache" vs an "LFU cache
/// (tie-break by least-recently-used)" embed DIFFERENTLY (measured: 0.97 vs 0.70 for
/// an LRU query), because the encoder captures meaning, not shared words. A small
/// action-fingerprint bonus adds intent agreement.
/// score = rel_ask_conf × (0.4 + 0.5·semantic + 0.1·intent).
pub fn best_coding_fix(brain: &mut SaidFile, problem: &str) -> Option<(String, f32)> {
    best_coding_fixes(brain, problem, 1).into_iter().next()
}

/// Top-K coding-fix candidates for `problem`, highest score first. The semantic
/// top-k contract: callers don't need precision@1 — they read the top 5/10 and the
/// right learning is among them (the orchestrator injects the best; an agent can
/// review several). At scale the neighborhood is wide and near-duplicates abound, so
/// returning a ranked list is the honest interface. Empty when no coding-fix frames.
///
/// Neighborhood and fingerprint widths scale with the corpus so a crowded store
/// doesn't truncate the true match out of the candidate pool before scoring.
pub fn best_coding_fixes(brain: &mut SaidFile, problem: &str, k: usize) -> Vec<(String, f32)> {
    // Widen the ask neighborhood + fingerprint pools with corpus size: at 10 records
    // 25 is plenty; at 1000s the right fix can sit past rank 25, so scale the fetch.
    let n = brain.frames.active_count();
    let fetch = (n / 2).clamp(50, 1000);

    // deep=true is REQUIRED here. The non-deep abstention/cutoff path is tuned for end-user `ask`: when a
    // coincidental high-confidence SYMBOL hit matches the query (e.g. the word "save" → a `save` symbol),
    // it treats "a confident answer exists" as true and DROPS the semantic tail — which is exactly the
    // fix frame we're after (a Procedural memory found semantically). Measured: on a paraphrased fix
    // query, brain.query surfaced the fix at 0.71 but non-deep ask returned ONLY the symbol (fix
    // starved), so recall_fix wrongly returned "No known fix"; deep ask returns the fix. We re-score the
    // fix candidates ourselves below (rel_conf + semantic + intent fingerprints), so we want the FULL
    // semantic-led pool here, not the abstention-trimmed top-K.
    let (fusion_cands, _kw) = ask(brain, problem, fetch, true, None);
    let ranked: Vec<(String, f32)> = fusion_cands.iter()
        .filter(|c| brain.frames.get_meta(&c.doc_id)
            .map(|m| m.tags.iter().any(|t| t == FIX_KIND_TAG)).unwrap_or(false))
        .map(|c| (c.doc_id.clone(), c.confidence))
        .collect();
    if ranked.is_empty() {
        return Vec::new();
    }
    let top_conf = ranked.iter().map(|(_, c)| *c).fold(0.0f32, f32::max).max(1e-6);

    // SEMANTIC discriminator: pure-semantic 1-bit fingerprint of the FULL problem
    // against every frame — separates near-twins (LRU vs LFU) where words tie.
    let sem_fp: HashMap<String, f32> = brain.rank_by_fingerprint(problem, fetch)
        .into_iter().collect();
    // INTENT bonus: action-isolated fingerprint (separates "add" from "document").
    let q_action = action_residue(problem);
    let action_fp: HashMap<String, f32> = if q_action.is_empty() {
        HashMap::new()
    } else {
        brain.rank_by_fingerprint(&q_action, fetch).into_iter()
            .filter_map(|(d, s)| d.strip_prefix(FIX_ACTION_ID_PREFIX).map(|id| (id.to_string(), s)))
            .collect()
    };

    let dbg = std::env::var("SAID_FIX_SCORE_DEBUG").is_ok();
    let mut scored: Vec<(String, f32)> = ranked.iter().map(|(doc_id, conf)| {
        let id16 = doc_id.strip_prefix("fix::").unwrap_or(doc_id);
        let intent = action_fp.get(id16).copied().unwrap_or(0.0);
        // ask confidence is the "right-neighborhood" SPINE signal — but in deep mode the float rerank
        // can zero a fix frame's ask confidence (its stored text differs from the query, so the centered
        // cosine ≈ 0) even though the fix is the correct answer. Used as a raw MULTIPLIER, rel_conf=0
        // then nullified strong semantic+intent fingerprints (measured: a paraphrased fix with
        // semantic=0.49 intent=0.61 scored 0.000 and was wrongly dropped). The frame is ALREADY in the
        // fix-filtered candidate set, so its membership is the spine signal; floor rel_conf so it
        // contributes without being able to veto the fingerprint discriminators that actually pick the
        // right fix. Floor 0.5 = "it's in the neighborhood"; a strong spine (→1.0) still ranks higher.
        let rel_conf = (conf / top_conf).max(0.5);
        let semantic = sem_fp.get(doc_id).copied()
            .or_else(|| sem_fp.get(&format!("{}{}", FIX_ACTION_ID_PREFIX, id16)).copied())
            .unwrap_or(0.0);
        // ask confidence = right neighborhood (spine); the semantic fingerprint of the
        // PROBLEM (meaning) + the action fingerprint (isolated INTENT) pick the right
        // one within it. Weighted comparably so an adversarial twin (an LFU fix that
        // mentions "least-recently-used") is out-voted by intent.
        let score = rel_conf * (0.3 + 0.4 * semantic + 0.3 * intent);
        if dbg {
            eprintln!("[fix-score] {} ask={:.3} rel={:.3} semantic={:.3} intent={:.3} -> {:.3}",
                doc_id, conf, rel_conf, semantic, intent, score);
        }
        (doc_id.clone(), score)
    }).collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k.max(1));
    scored
}

// best_coding_fix(es) are validated end-to-end by the decoy + scale harnesses
// (hard-eval/recall-measure.sh, recall-scale.sh) — they need a brain with the static
// encoder loaded (the semantic fingerprint signal), which a pure unit test cannot give.

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

/// Extract searchable keywords from a natural-language query.
/// Returns `(lowercased_keywords, original_case_keywords)`.
///
/// Lowercased keywords are used by grep and SCA (both case-insensitive).
/// Original-case keywords are used by the symbol candidate generator so
/// queries like "what is FrameStore" correctly hit the PascalCase symbol.
pub fn ask_extract_keywords(query: &str) -> (Vec<String>, Vec<String>) {
    let stop: HashSet<&str> = ASK_STOPWORDS.iter().copied().collect();
    let mut seen_lower: HashSet<String> = HashSet::new();
    let mut lower: Vec<String> = Vec::new();
    let mut original: Vec<String> = Vec::new();
    for word in query.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if word.len() < 3 { continue; }
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
    for cand_name in ask_symbol_candidates(&keywords, &keywords_orig) {
        for sym_hit in brain.sym(&cand_name, 5) {
            if sym_hit.name != cand_name { continue; }
            if let Some(scope) = scope_doc_ids {
                if !scope.contains(&sym_hit.doc_id) { continue; }
            }
            let content = brain.get(&sym_hit.doc_id).unwrap_or_default();
            upsert(&mut candidates, AskCandidate {
                doc_id: sym_hit.doc_id.clone(),
                confidence: 1.00,
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
    for kw in &keywords {
        if kw.len() < 3 { continue; }
        let hits = brain.grep(kw, 30);
        for h in hits {
            if let Some(scope) = scope_doc_ids {
                if !scope.contains(&h.doc_id) { continue; }
            }
            let content_lower = h.content.to_lowercase();
            let terms_present = keywords.iter()
                .filter(|k| content_lower.contains(k.as_str()))
                .count();
            let min_terms = if keywords.len() >= 2 { 2 } else { 1 };
            if terms_present < min_terms { continue; }
            let confidence = (0.40 + 0.15 * (terms_present as f32 - 1.0))
                .min(0.95).max(0.40);
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
            .filter(|k| content_lower.contains(k.as_str()))
            .count();
        if rank >= ASK_SCA_GUARANTEED && terms_present == 0 { continue; }
        let base = 0.30 + (h.score * 0.30).clamp(0.0, 0.30);
        let kw_bonus = 0.05 * (terms_present as f32 - 1.0).max(0.0);
        let confidence = (base + kw_bonus).min(0.80);
        upsert(&mut candidates, AskCandidate {
            doc_id: h.doc_id.clone(),
            confidence,
            kind: "semantic",
            content: h.content,
            location: None,
        });
    }

    // ── Merge + relative cutoff + truncate ───────────────────────────────
    let mut results: Vec<AskCandidate> = candidates.into_values().collect();
    results.sort_by(|a, b| {
        b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
    });

    let top_score = results.first().map(|r| r.confidence).unwrap_or(0.0);
    let cutoff = top_score * ASK_RELATIVE_CUTOFF;

    let max_results = if deep { usize::MAX } else { top };
    let kept: Vec<AskCandidate> = results.into_iter()
        .enumerate()
        .filter(|(i, r)| *i < ASK_SCA_GUARANTEED || r.confidence >= cutoff)
        .map(|(_, r)| r)
        .take(max_results)
        .collect();

    (kept, keywords)
}

// ── Coding-fix recall: ONE scorer, shared by said-cli and said-orchestration ──
// Previously the CLI (`best_fix_for`) and the orchestrator (`best_iteration`) used
// DIFFERENT scorers — the CLI's intent fingerprint vs. raw `ask` fusion text — and
// they disagreed (the same match scored 0.53 in one, 0.95 in the other). Fusion
// text is bag-of-words and collides at scale. This is the single source of truth:
// intent fingerprint (the gate, separates "add X" from "document X") + symmetric
// (Jaccard) overlap of the DISTINCTIVE target tokens (picks the right X), weighted
// so a strong conceptual match scores HIGH enough to clear a confidence floor.

/// Marker strings for the stored coding-fix body (must match what `learn-fix` writes
/// and what said-orchestration::learn uses).
pub const FIX_KIND_TAG: &str = "coding-fix";
pub const FIX_ACTION_ID_PREFIX: &str = "fixaction::";
const FIX_EDITS_SEP: &str = "\n<<<SAID-FIX-EDITS>>>\n";
const FIX_ACTION_SEP: &str = "\n<<<SAID-FIX-ACTION>>>\n";

/// Split a stored fix body into (problem/TASK line, edits JSON, action residue).
fn split_fix_body(body: &str) -> (String, String, String) {
    let (note_plus, action) = match body.split_once(FIX_ACTION_SEP) {
        Some((a, b)) => (a, b.trim().to_string()),
        None => (body, String::new()),
    };
    let (note, edits) = match note_plus.split_once(FIX_EDITS_SEP) {
        Some((a, b)) => (a, b.trim().to_string()),
        None => (note_plus, String::new()),
    };
    let problem = note.lines().next()
        .and_then(|l| l.strip_prefix("TASK: "))
        .unwrap_or("").trim().to_string();
    (problem, edits, action)
}

/// Common English/boilerplate words that carry no discriminating signal for a
/// coding problem ("implement an X cache" — the X is what matters, not the rest).
/// Kept tiny and obvious; this is a stop-list, not NLP.
const FIX_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "over", "per",
    "implement", "add", "fix", "make", "build", "create", "use", "using", "when",
    "where", "which", "must", "should", "its", "are", "not", "but", "all", "any",
    "get", "put", "set", "has", "size", "count", "return", "returns", "value",
    "key", "keys", "entry", "operation", "operations", "average",
];

/// Distinctive concept tokens of a problem string, lowercased: alphanumeric tokens
/// of length >= 3 that are NOT stopwords. Does NOT strip CamelCase / all-caps tokens
/// — so "LRU", "LFU", "TTL" SURVIVE as the most discriminating words (action_residue
/// strips exactly those; that was the bug). ALSO splits CamelCase so "LRUCache"
/// contributes both "lru" and "cache", matching a stored "LRU cache" written as two
/// words (the tokenization gap that left `LRUCache get put` scoring low).
fn concept_tokens(problem: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for raw in problem.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '/')) {
        if raw.is_empty() { continue; }
        for piece in split_camel(raw) {
            let t = piece.to_lowercase();
            if t.len() >= 3 && !FIX_STOPWORDS.contains(&t.as_str()) {
                out.insert(t);
            }
        }
    }
    out
}

/// Split a token on CamelCase / acronym boundaries, keeping the whole token too.
/// "LRUCache" -> ["LRUCache", "LRU", "Cache"]; "getOrCreate" -> [whole, get, Or,
/// Create]. snake_case is already split by the caller's delimiter pass.
fn split_camel(tok: &str) -> Vec<String> {
    let mut parts = vec![tok.to_string()];
    let chars: Vec<char> = tok.chars().collect();
    let mut start = 0;
    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let cur = chars[i];
        let next = chars.get(i + 1).copied();
        // Boundary: lower->Upper (getOr|Create), or Upper-run -> Upper+lower
        // (LRU|Cache: split before the C that starts a new word).
        let boundary = (prev.is_lowercase() && cur.is_uppercase())
            || (prev.is_uppercase() && cur.is_uppercase()
                && next.map(|n| n.is_lowercase()).unwrap_or(false));
        if boundary {
            parts.push(chars[start..i].iter().collect());
            start = i;
        }
    }
    if start > 0 {
        parts.push(chars[start..].iter().collect());
    }
    parts
}

/// The single coding-fix scorer. Returns the best-matching coding-fix `doc_id` and
/// its score in [0,1], or None if there are no coding-fix candidates. Caller applies
/// its own confidence floor. Used by BOTH the CLI `recall-fix` and the orchestrator.
///
/// Design: RIDE the world-class `ask` chain (the documented retrieval pipeline —
/// SCA + routed BM25/IDF + entity boost + graph fan-out, MTEB 0.9655). Its ranked
/// `confidence` is the spine — we do NOT re-implement IDF/concept scoring here, that
/// duplicates Layer 2. The ONE thing the chain lacks for procedural coding-fixes is
/// INTENT isolation: two near-twins ("LRU cache" vs an "LFU cache" whose text says
/// "tie-break by least-recently-used") have near-identical text, so the chain ties
/// them. The action/intent fingerprint (the documented "intent breakthrough",
/// action-isolated 1-bit matching) is the discriminator that separates them. So:
/// score = ask_confidence (relative) × intent-agreement factor.
pub fn best_coding_fix(brain: &mut SaidFile, problem: &str) -> Option<(String, f32)> {
    let (fusion_cands, _kw) = ask(brain, problem, 25, false, None);
    // Keep ask's ranking + confidence for coding-fix frames only.
    let ranked: Vec<(String, f32)> = fusion_cands.iter()
        .filter(|c| brain.frames.get_meta(&c.doc_id)
            .map(|m| m.tags.iter().any(|t| t == FIX_KIND_TAG)).unwrap_or(false))
        .map(|c| (c.doc_id.clone(), c.confidence))
        .collect();
    if ranked.is_empty() {
        return None;
    }
    let top_conf = ranked.iter().map(|(_, c)| *c).fold(0.0f32, f32::max).max(1e-6);

    // Intent fingerprint over the action residue — action-isolated, so it scores by
    // WHAT IS BEING DONE (implement an eviction cache) not the shared surface words.
    // This is the tie-breaker the ask chain doesn't carry for fixes.
    let q_action = action_residue(problem);
    let action_fp: HashMap<String, f32> = if q_action.is_empty() {
        HashMap::new()
    } else {
        brain.rank_by_fingerprint(&q_action, 100).into_iter()
            .filter_map(|(d, s)| d.strip_prefix(FIX_ACTION_ID_PREFIX).map(|id| (id.to_string(), s)))
            .collect()
    };
    // IDF-weighted concept overlap — the discriminator the ask chain lacks for
    // near-twins. ask ranks "LRU cache" and an "LFU cache (tie-break by LRU)" equal
    // (near-identical text); the intent fingerprint is brittle to paraphrase and
    // often 0. What reliably separates them is which DISTINCTIVE concept tokens are
    // shared, weighted by rarity across the candidate set (so "lru" >> "cache").
    // Computed over the coding-fix candidates only — small, self-contained.
    let q_concept = concept_tokens(problem);
    let mut df: HashMap<String, usize> = HashMap::new();
    let mut cand_concept: HashMap<String, HashSet<String>> = HashMap::new();
    for (doc_id, _) in &ranked {
        let body = brain.get(doc_id).unwrap_or_default();
        let (c_problem, _e, _a) = split_fix_body(&body);
        let c = concept_tokens(&c_problem);
        for t in &c { *df.entry(t.clone()).or_insert(0) += 1; }
        cand_concept.insert(doc_id.clone(), c);
    }
    let n_docs = ranked.len() as f32;
    let idf = |tok: &str| -> f32 {
        let d = df.get(tok).copied().unwrap_or(0) as f32;
        (1.0 + n_docs / (1.0 + d)).ln()
    };
    let q_idf_total: f32 = q_concept.iter().map(|t| idf(t)).sum();

    let dbg = std::env::var("SAID_FIX_SCORE_DEBUG").is_ok();
    let mut best: Option<(String, f32)> = None;
    for (doc_id, conf) in &ranked {
        let id16 = doc_id.strip_prefix("fix::").unwrap_or(doc_id);
        let intent = action_fp.get(id16).copied().unwrap_or(0.0);
        let rel_conf = conf / top_conf;
        let empty = HashSet::new();
        let c_concept = cand_concept.get(doc_id).unwrap_or(&empty);
        let concept = if q_idf_total <= 0.0 { 0.0 } else {
            let covered: f32 = q_concept.iter()
                .filter(|t| c_concept.contains(*t)).map(|t| idf(t)).sum();
            covered / q_idf_total
        };
        // ask confidence is the recall spine (gets the right neighborhood); the
        // IDF concept overlap picks the right one WITHIN that neighborhood (breaks
        // near-twin ties via distinctive tokens); the intent fingerprint adds a
        // small same-action bonus. Concept-weighted so a distinctive match wins.
        let score = rel_conf * (0.35 + 0.5 * concept + 0.15 * intent);
        if dbg {
            eprintln!("[fix-score] {} ask={:.3} rel={:.3} concept={:.3} intent={:.3} -> {:.3}",
                doc_id, conf, rel_conf, concept, intent, score);
        }
        if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
            best = Some((doc_id.clone(), score));
        }
    }
    best
}

#[cfg(test)]
mod fix_score_tests {
    use super::*;

    #[test]
    fn split_camel_breaks_acronym_words() {
        let p = split_camel("LRUCache");
        assert!(p.iter().any(|s| s == "LRUCache"));
        assert!(p.iter().any(|s| s == "LRU"), "acronym kept: {:?}", p);
        assert!(p.iter().any(|s| s == "Cache"), "trailing word kept: {:?}", p);
        let g = split_camel("getOrCreate");
        assert!(g.iter().any(|s| s == "get"));
        assert!(g.iter().any(|s| s == "Create"));
    }

    #[test]
    fn concept_tokens_keep_acronyms_drop_stopwords() {
        let c = concept_tokens("Implement an LRU cache with get and put");
        assert!(c.contains("lru"), "distinctive acronym survives: {:?}", c);
        assert!(c.contains("cache"));
        assert!(!c.contains("get"), "stopword dropped");
        assert!(!c.contains("the"));
        // LRUCache split lets the one-word form match a two-word stored "lru cache".
        let c2 = concept_tokens("LRUCache get put evict");
        assert!(c2.contains("lru") && c2.contains("cache"), "camel split: {:?}", c2);
    }

    #[test]
    fn lru_and_lfu_concepts_are_distinct() {
        let lru = concept_tokens("Implement an LRU cache evict least recently used");
        let lfu = concept_tokens("Implement an LFU cache evict least frequently used");
        assert!(lru.contains("lru") && !lru.contains("lfu"));
        assert!(lfu.contains("lfu") && !lfu.contains("lru"));
    }
}

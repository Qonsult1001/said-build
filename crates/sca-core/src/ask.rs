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

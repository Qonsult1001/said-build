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
            RecalledFix { note: fix_note(&body), edits_json: fix_edits(&body), doc_id, score }
        })
        .filter(|fix| match &lang_want {
            None => true,
            Some(want) => match frame_lang(&fix.note) {
                Some(have) => &have == want, // tagged frame: must match the active language
                None => true,               // untagged/general frame: language-agnostic, keep
            },
        })
        .take(k.max(1))
        .collect()
}

/// Parse the stored `lang:<x>` token from a coding-fix frame body (it lives in the FILES
/// line, e.g. `src:context7 lang:csharp area:architecture arch:ddd`). Lower-cased; None
/// when the frame carries no language token (a general/legacy frame). This is the recall-
/// time language signal until the factory promotes `lang:` to a first-class meta tag.
fn frame_lang(body: &str) -> Option<String> {
    body.split_whitespace()
        .find_map(|tok| tok.strip_prefix("lang:"))
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty())
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

    let (fusion_cands, _kw) = ask(brain, problem, fetch, false, None);
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
        let rel_conf = conf / top_conf;
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

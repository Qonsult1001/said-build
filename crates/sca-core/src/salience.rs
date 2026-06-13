//! Decision 4 — salience scoring (v0, deterministic heuristic).
//!
//! Scores a single turn/utterance/memory on how "worth remembering" it is.
//! v0 is rule-based — no training data, no model weights, no ML dependency.
//! Future versions will swap `score_turn`'s internals for a Model2Vec + linear
//! head trained on labeled turns (~1K examples) but the return shape stays
//! stable so every downstream consumer (dream function, MCP tag writer,
//! session accumulator) keeps working across the upgrade.
//!
//! Rationale for shipping v0 now:
//! - Dream function (Decision 5) needs SOMETHING to decide which episodic
//!   frames to distil; waiting for a trained model blocks the whole pillar
//!   pipeline.
//! - Heuristic + tags written to real user sessions GENERATE the labeled
//!   training data we need for v1 (every user correction = a negative
//!   label, every /remember = a positive label).
//! - The 8 signals below are what mem0's heuristic pre-filter actually
//!   checks before calling the LLM; we just stay honest about being
//!   heuristic instead of wrapping an LLM call on top.
//!
//! Signals (all bounded, each contributes ≤ 20 points to a 0..=100 score):
//!   1. Explicit markers (`important:`, `always`, `never`, `must`, `/remember`)
//!   2. Correction markers (`actually`, `no, wrong`, `i meant`, `not`)
//!   3. Decision markers (`we decided`, `let's go with`, `chose`)
//!   4. Assertion markers (`X is Y`, `X =`, `the password is`, `my X is`)
//!   5. Length band (very short → low; medium → neutral; very long → mild)
//!   6. Chit-chat penalty (`lol`, `ok`, `thanks`, `cool`, `got it`) → low
//!   7. Question mark (`?` = usually not worth remembering without answer)
//!   8. Caller-supplied pillar bias (Episodic tool_completion → +10, etc.)
//!
//! Score bands:
//!   0..=29   Low    (drop / leave unranked — noise, chit-chat)
//!   30..=59  Medium (keep, normal retrieval weight)
//!   60..=100 High   (always preserve, high reconsolidation weight)
//!
//! Session accumulator: caller keeps a running sum of scores across a session.
//! When it crosses threshold 150 (per Generative Agents), emit a dream cycle.

use crate::frames::Pillar;

/// Bounded score band for a single turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalienceBand {
    /// 0..=29 — noise / chit-chat / unanswered question.
    Low,
    /// 30..=59 — keep, normal retrieval weight.
    Medium,
    /// 60..=100 — preserve, flag for reconsolidation.
    High,
}

impl SalienceBand {
    pub fn from_score(score: u32) -> Self {
        match score {
            0..=29 => Self::Low,
            30..=59 => Self::Medium,
            _ => Self::High,
        }
    }

    /// Lowercase name suitable for a tag (`salience:low`).
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Result of scoring one turn.
#[derive(Debug, Clone)]
pub struct Salience {
    /// Bounded 0..=100 integer score.
    pub score: u32,
    pub band: SalienceBand,
    /// Tags to attach to the resulting frame (e.g. `salience:high`,
    /// `reconsolidation`, `decision`). Callers can pass these straight
    /// through `remember_with_pillar(extra_tags: ...)`.
    pub tags: Vec<String>,
}

impl Salience {
    /// Convenience: the `salience:<band>` tag as a String.
    pub fn band_tag(&self) -> String {
        format!("salience:{}", self.band.tag())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Signal detectors — pure functions on lowercase text. Each returns points.
// ════════════════════════════════════════════════════════════════════════════

const EXPLICIT_MARKERS: &[&str] = &[
    "/remember",
    "important:",
    "note:",
    "always ",
    "never ",
    "must ",
    "critical:",
    "remember that",
    "don't forget",
    "key point",
];

const CORRECTION_MARKERS: &[&str] = &[
    "actually,",
    "actually ",
    "no, wrong",
    "no wrong",
    "i meant",
    "i mean,",
    "not x",
    " not that",
    "correction:",
    "to be clear",
    "let me correct",
];

const DECISION_MARKERS: &[&str] = &[
    "we decided",
    "let's go with",
    "let's use",
    "chose ",
    "going with ",
    "we'll use",
    "decision:",
    "approved:",
    "agreed:",
    "final answer",
];

/// Patterns that look like fact-binding assertions. Cheap substring checks
/// that match common factual shapes without needing a parser.
const ASSERTION_MARKERS: &[&str] = &[
    " is ",
    " are ",
    " = ",
    ":",
    " was ",
    " were ",
];

/// Substrings that indicate low-signal social/conversational chaff.
/// Matched against the WHOLE lowercase turn; a short utterance that is
/// MOSTLY one of these gets the chit-chat penalty.
const CHIT_CHAT_PHRASES: &[&str] = &[
    "lol",
    "haha",
    "ok",
    "okay",
    "thanks",
    "thank you",
    "ty",
    "cool",
    "nice",
    "got it",
    "sure",
    "yep",
    "yeah",
    "no problem",
    "np",
    "welcome",
    "sounds good",
];

fn count_markers(hay: &str, needles: &[&str]) -> u32 {
    let mut hits = 0u32;
    for n in needles {
        if hay.contains(n) {
            hits += 1;
        }
    }
    hits
}

fn explicit_score(lower: &str) -> u32 {
    // Explicit "remember this" markers are the strongest positive signal —
    // the user is literally asking for preservation. One marker contributes
    // 35; cap at 45 (stacked markers rarely add more information after the
    // first hit). A single explicit marker + any of length/assertion/pillar
    // signal should cross the High threshold (60) by itself.
    (count_markers(lower, EXPLICIT_MARKERS) * 35).min(45)
}

fn correction_score(lower: &str) -> u32 {
    (count_markers(lower, CORRECTION_MARKERS) * 12).min(20)
}

fn decision_score(lower: &str) -> u32 {
    (count_markers(lower, DECISION_MARKERS) * 12).min(20)
}

fn assertion_score(lower: &str) -> u32 {
    // Needs at least one assertion marker AND a non-trivial length — rules
    // out tiny utterances that happen to contain "is".
    let word_count = lower.split_whitespace().count();
    if word_count < 4 {
        return 0;
    }
    let hits = count_markers(lower, ASSERTION_MARKERS);
    if hits == 0 {
        0
    } else {
        // Single-marker assertion = 8, multi-marker = 12, cap 15.
        (8 + (hits.saturating_sub(1)) * 2).min(15)
    }
}

fn length_score(word_count: usize) -> u32 {
    // Bell shape: very short = 0, 6..40 words = 10, long tapers.
    // Widened the "normal" band to start at 6 — a 6-8 word assertion like
    // "the API key is key_abc123" is high-signal and shouldn't get a
    // length penalty just for being concise.
    match word_count {
        0..=3 => 0,
        4..=5 => 5,
        6..=40 => 10,
        41..=120 => 7,
        _ => 5,
    }
}

fn chit_chat_penalty(lower: &str, word_count: usize) -> u32 {
    if word_count > 6 {
        return 0;
    }
    // For short utterances, if the whole thing is basically a chat phrase,
    // subtract heavily. "ok thanks" → penalty 15. "ok thanks I'll check
    // later" → only length penalty, no chat penalty.
    let trimmed = lower.trim_end_matches(|c: char| !c.is_alphanumeric());
    for p in CHIT_CHAT_PHRASES {
        if trimmed == *p || (trimmed.starts_with(p) && trimmed.len() <= p.len() + 3) {
            return 15;
        }
    }
    0
}

fn question_penalty(text: &str) -> u32 {
    // A bare question without an answer binding is lower-value.
    if text.trim_end().ends_with('?') {
        5
    } else {
        0
    }
}

fn pillar_bias(pillar: Pillar) -> u32 {
    // Small nudges by pillar — caller's semantic choice is a signal.
    match pillar {
        Pillar::Semantic => 8,    // Distilled facts are worth keeping.
        Pillar::Procedural => 10, // Action recipes are high value.
        Pillar::External => 5,
        Pillar::Code => 5,
        // Vault frames carry no personal-salience signal, same as raw turns and legacy memory.
        Pillar::Episodic | Pillar::Memory | Pillar::Document => 0,
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Public API
// ════════════════════════════════════════════════════════════════════════════

/// Score a single turn for salience. Pure function — same input = same
/// output, deterministic across runs.
///
/// `text`: the raw utterance/turn/memory content (any length).
/// `pillar`: the caller's intent for where this frame will be written.
///           Affects only the pillar bias signal.
pub fn score_turn(text: &str, pillar: Pillar) -> Salience {
    let lower = text.to_lowercase();
    let word_count = lower.split_whitespace().count();

    let explicit = explicit_score(&lower);
    let correction = correction_score(&lower);
    let decision = decision_score(&lower);
    let assertion = assertion_score(&lower);
    let length = length_score(word_count);
    let bias = pillar_bias(pillar);

    let chit_chat = chit_chat_penalty(&lower, word_count);
    let question = question_penalty(text);

    let gross = explicit + correction + decision + assertion + length + bias;
    let penalty = chit_chat + question;
    let score = gross.saturating_sub(penalty).min(100);

    // Build tag set: always the band tag, plus specific markers so the
    // dream function can filter on event types without re-scoring.
    let mut tags: Vec<String> = Vec::new();
    let band = SalienceBand::from_score(score);
    tags.push(format!("salience:{}", band.tag()));
    if correction > 0 {
        tags.push("reconsolidation".to_string());
    }
    if decision > 0 {
        tags.push("decision".to_string());
    }
    if explicit > 0 {
        tags.push("explicit".to_string());
    }

    Salience { score, band, tags }
}

/// Running accumulator — tracks salience across a session / window so we can
/// trigger a dream cycle when cumulative importance crosses threshold.
///
/// Threshold 150 is from Generative Agents (Park et al. 2023). Callers can
/// override via `new_with_threshold`.
#[derive(Debug, Clone)]
pub struct SalienceAccumulator {
    sum: u32,
    count: u32,
    threshold: u32,
}

impl SalienceAccumulator {
    pub fn new() -> Self {
        Self { sum: 0, count: 0, threshold: 150 }
    }

    pub fn new_with_threshold(threshold: u32) -> Self {
        Self { sum: 0, count: 0, threshold }
    }

    /// Add a scored turn's score to the running sum. Returns `true` if the
    /// accumulator just crossed its threshold (caller should trigger a
    /// dream cycle; the accumulator resets automatically on crossing).
    pub fn add(&mut self, score: u32) -> bool {
        self.sum += score;
        self.count += 1;
        if self.sum >= self.threshold {
            self.sum = 0;
            self.count = 0;
            true
        } else {
            false
        }
    }

    pub fn sum(&self) -> u32 { self.sum }
    pub fn count(&self) -> u32 { self.count }
    pub fn threshold(&self) -> u32 { self.threshold }
    pub fn reset(&mut self) { self.sum = 0; self.count = 0; }
}

impl Default for SalienceAccumulator {
    fn default() -> Self { Self::new() }
}

// ════════════════════════════════════════════════════════════════════════════
// Step 8 — Surprise / reconsolidation detector (semantic contradiction)
// ════════════════════════════════════════════════════════════════════════════

/// Outcome of checking a new utterance against the brain's existing memory.
///
/// `Benign` = nothing interesting, same topic as prior memory but consistent,
/// or no close-enough prior memory. No tagging needed.
///
/// `TopicalUpdate` = we found a close prior frame on the same topic; the new
/// content is different but not contradictory (e.g. adding detail to a known
/// fact). Surface it with `reconsolidation:update` tag — dream can merge.
///
/// `Contradiction` = close prior frame, but the new content flips discriminating
/// tokens (the user is correcting themselves or overriding a stored fact).
/// Surface with `reconsolidation:contradicts:<prior_doc_id>` tag — admin tools
/// can show conflict pairs, dream layer can preserve both as disagreements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surprise {
    Benign,
    TopicalUpdate { prior_doc_id: String },
    Contradiction { prior_doc_id: String },
}

impl Surprise {
    /// Tags to attach to the NEW frame when surprise is non-benign.
    /// Returns an empty vec for `Benign`.
    pub fn tags(&self) -> Vec<String> {
        match self {
            Self::Benign => Vec::new(),
            Self::TopicalUpdate { prior_doc_id } => vec![
                "reconsolidation".to_string(),
                "reconsolidation:update".to_string(),
                format!("updates:{}", prior_doc_id),
            ],
            Self::Contradiction { prior_doc_id } => vec![
                "reconsolidation".to_string(),
                "reconsolidation:contradicts".to_string(),
                format!("contradicts:{}", prior_doc_id),
            ],
        }
    }
}

/// A match candidate from the caller's nearest-frame lookup.
/// `doc_id` identifies the prior frame; `similarity` is a normalized [0, 1]
/// score where 1.0 = identical fingerprint and 0.0 = unrelated.
/// `token_overlap` is the fraction of high-IDF query tokens also present
/// in the candidate's body — used to distinguish contradiction (same topic,
/// different facts) from topical update (same topic, expanding facts).
#[derive(Debug, Clone)]
pub struct PriorMatch {
    pub doc_id: String,
    pub similarity: f32,
    pub token_overlap: f32,
}

/// Classify an incoming utterance's relationship to prior memory.
///
/// Pure function of the new content + the caller-provided best-match
/// candidate. No LLM. No training data. Deterministic.
///
/// **Thresholds (tuned on the surprise_probe fixture — see example):**
///   - similarity < 0.25 → `Benign` (different topic, nothing to reconcile)
///   - similarity ≥ 0.25 AND token_overlap ≥ 0.90 → `TopicalUpdate`
///     (near-identical vocabulary → the new content restates / expands; dream
///     layer can merge)
///   - similarity ≥ 0.25 AND token_overlap < 0.90 AND ≥ 0.40 → `Contradiction`
///     (close topic, shares most structural tokens but ≥ 1 key token differs
///     → likely override or correction)
///   - similarity ≥ 0.25 AND overlap < 0.40 → `Benign`
///     (topic match was superficial, not enough shared meaning)
///
/// Similarity comes from `SaidFile::find_prior_match`, which divides the
/// raw SCA+grep fused score by 10 to land on a [0, 1]-ish scale. The 0.40
/// floor corresponds to a raw score of ~4, which on short utterances
/// reliably indicates "same topic".
///
/// Lexical correction markers (`actually`, `no, wrong`, `i meant`) are
/// handled by `score_turn` — this function complements them by catching
/// silent corrections where the user doesn't announce the change.
pub fn classify_surprise(prior: Option<&PriorMatch>) -> Surprise {
    let Some(m) = prior else { return Surprise::Benign };
    if m.similarity < 0.25 {
        return Surprise::Benign;
    }
    if m.token_overlap >= 0.90 {
        return Surprise::TopicalUpdate { prior_doc_id: m.doc_id.clone() };
    }
    if m.token_overlap >= 0.40 {
        return Surprise::Contradiction { prior_doc_id: m.doc_id.clone() };
    }
    Surprise::Benign
}

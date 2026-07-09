//! Write-time TEMPORAL GROUNDING: when a personal memory is saved, relative time phrases
//! ("last quarter", "last year", "yesterday") are resolved to ABSOLUTE dates in the stored text,
//! so plain semantic recall finds the memory later (the answering LLM no longer has to know what
//! "last year" meant at write time). This mirrors the research-proven Mem0 Layer-1 pattern
//! (configs/prompts.py: "Always ground relative references to specific dates") but done
//! DETERMINISTICALLY in pure Rust — the free brain is offline/no-LLM. `today` is passed in
//! (not read from the clock) so the transform is reproducible and unit-testable.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_temporal_grounding -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::time_compat::ground_relative_dates;

// Anchor "today" = 2026-07-06 (a Monday in Q3 2026), matching the session date.
const Y: i32 = 2026;
const M: u32 = 7;
const D: u32 = 6;

fn g(text: &str) -> String {
    ground_relative_dates(text, Y, M, D)
}

#[test]
fn last_year_grounds_to_previous_year() {
    let out = g("Last year I completed my marine biology certification.");
    assert!(out.contains("2025"), "‘last year’ from 2026 must ground to 2025, got: {out}");
    // Original text is preserved (append, don't destroy).
    assert!(out.contains("marine biology"), "original content preserved: {out}");
}

#[test]
fn last_quarter_grounds_to_previous_quarter() {
    // Today is Q3 2026 → last quarter = Q2 2026.
    let out = g("Last quarter I migrated the billing system to SQLite.");
    assert!(out.contains("Q2 2026"), "‘last quarter’ from Q3 2026 must ground to Q2 2026, got: {out}");
}

#[test]
fn last_quarter_wraps_year_boundary() {
    // If today were Q1, last quarter is Q4 of the PRIOR year.
    let out = ground_relative_dates("Last quarter we shipped the release.", 2026, 2, 15); // Q1 2026
    assert!(out.contains("Q4 2025"), "‘last quarter’ from Q1 2026 must ground to Q4 2025, got: {out}");
}

#[test]
fn this_year_grounds_to_current_year() {
    let out = g("Earlier this year I hired Carol.");
    assert!(out.contains("2026"), "‘this year’ must ground to 2026, got: {out}");
}

#[test]
fn last_month_grounds_to_previous_month() {
    // July 2026 → last month = June 2026.
    let out = g("Last month I took a holiday.");
    assert!(out.contains("June 2026") || out.contains("2026-06"),
        "‘last month’ from July 2026 must ground to June 2026, got: {out}");
}

#[test]
fn last_month_wraps_year_boundary() {
    let out = ground_relative_dates("Last month the build broke.", 2026, 1, 10); // January
    assert!(out.contains("December 2025") || out.contains("2025-12"),
        "‘last month’ from January must ground to December of prior year, got: {out}");
}

#[test]
fn no_relative_phrase_is_left_untouched() {
    let text = "The wifi password is sunflower-42.";
    assert_eq!(g(text), text, "text with no relative phrase must be returned byte-identical");
}

#[test]
fn absolute_dates_are_not_double_grounded() {
    // A memory that already says "in 2025" must not get a spurious extra grounding.
    let text = "On 19th September 2025 I signed the office lease.";
    let out = g(text);
    assert_eq!(out, text, "already-absolute text must be untouched: {out}");
}

#[test]
fn grounding_is_idempotent() {
    // Grounding an already-grounded string must not append a second time.
    let once = g("Last year I got certified.");
    let twice = ground_relative_dates(&once, Y, M, D);
    assert_eq!(once, twice, "grounding must be idempotent (no double-append): {twice}");
}

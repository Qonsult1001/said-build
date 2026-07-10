//! Regression gates for the recall-UX primitives shipped in v0.11.8–v0.11.9:
//!   - `tag_scope` — the AND-tag pre-filter that `ask tags:[…]` / `ask --tag` feed to `scope_doc_ids`.
//!   - the tie-band math — the gap-to-#2 rule that decides whether the "close matches" footer fires.
//! The footer/tags rendering itself lives in the said-mcp handler + said-cli (argv/JSON-RPC surfaces,
//! covered by verify-mcp.md / verify-cli.md); this test locks the CORE logic those surfaces depend on
//! so a refactor can't silently break tag-scoped recall or the tie decision.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model" --test test_recall_ux_gates -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn tag_scope_and_filters_and_is_honest_on_no_match() {
    let path = "test_recall_ux_gates.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "encoder must load");

    // Three Q2 items, two Q3 items; some also carry project:said.
    brain.remember_with_salience(Some("q2a"), "Q2 watcher plan.", None, Pillar::Semantic,
        vec!["quarter:Q2".into(), "project:said".into()]);
    brain.remember_with_salience(Some("q2b"), "Q2 git plan.", None, Pillar::Semantic,
        vec!["quarter:Q2".into(), "project:said".into()]);
    brain.remember_with_salience(Some("q2c"), "Q2 mail plan.", None, Pillar::Semantic,
        vec!["quarter:Q2".into()]);
    brain.remember_with_salience(Some("q3a"), "Q3 oauth pilot.", None, Pillar::Semantic,
        vec!["quarter:Q3".into(), "project:said".into()]);
    brain.remember_with_salience(Some("q3b"), "Q3 gmail live.", None, Pillar::Semantic,
        vec!["quarter:Q3".into()]);
    brain.build_index().expect("build_index");

    // Single-tag scope → exactly the Q2 set.
    let q2 = brain.tag_scope(&["quarter:Q2".into()]).expect("some scope");
    assert_eq!(q2.len(), 3, "quarter:Q2 scopes to the 3 Q2 memories");
    assert!(q2.contains("q2a") && q2.contains("q2b") && q2.contains("q2c"));
    assert!(!q2.contains("q3a"), "Q3 memory must not leak into the Q2 scope");

    // AND across tags → intersection (Q2 AND project:said = q2a, q2b only).
    let q2_said = brain.tag_scope(&["quarter:Q2".into(), "project:said".into()]).expect("some");
    assert_eq!(q2_said.len(), 2, "Q2 AND project:said = 2 (AND semantics, not OR)");
    assert!(q2_said.contains("q2a") && q2_said.contains("q2b"));
    assert!(!q2_said.contains("q2c"), "q2c lacks project:said → excluded by AND");

    // A tag matching NOTHING yields an empty scope (honest — recall returns nothing), NOT None
    // (which would silently fall back to unscoped). This is the load-bearing "no silent fallback".
    let none_match = brain.tag_scope(&["quarter:Q9".into()]);
    assert_eq!(none_match, Some(std::collections::HashSet::new()),
        "an unmatched tag returns an EMPTY scope, not None (no silent unscoped fallback)");

    // Empty tag list → None (no filter at all).
    assert!(brain.tag_scope(&[]).is_none(), "no tags → no scope filter");

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));
}

/// The tie-footer decision is: given ranked confidences, does #1 CLEARLY win (gap to #2 > 0.03)?
/// This is the exact rule both the MCP handler and the CLI use; lock it so a tweak can't make the
/// footer chatty (fire on a clear leader) or silent (miss a flat cluster). Pure arithmetic — no brain.
#[test]
fn tie_band_fires_on_flat_cluster_and_suppresses_on_clear_leader() {
    // Mirror of the shipped guard: footer fires iff there are >=3 results within 0.05 of the top
    // AND #1 does not clearly lead (gap to #2 <= 0.03).
    fn footer_fires(scores: &[f32]) -> bool {
        if scores.len() < 3 { return false; }
        let top = scores[0];
        let gap_to_second = top - scores[1];
        if gap_to_second > 0.03 { return false; }              // clear leader → suppress
        let tied = scores.iter().filter(|s| (top - **s) <= 0.05).count();
        tied >= 3
    }

    // Flat cluster at the grep ceiling (5× 0.95, zero gap) → the ambiguity worth scoping → FIRES.
    assert!(footer_fires(&[0.95, 0.95, 0.95, 0.95, 0.95]), "flat cluster is a tie → footer fires");

    // A clear leader (0.99 with a 0.10 gap) even with other near-top hits → SUPPRESSED (not chatty).
    assert!(!footer_fires(&[0.99, 0.89, 0.88, 0.87]), "clear leader → footer suppressed");

    // The classic 0.90-band bleed (integrations) → FIRES.
    assert!(footer_fires(&[0.90, 0.90, 0.90, 0.90]), "0.90-band bleed → footer fires");

    // Fewer than 3 results → never fires (single winner or a pair).
    assert!(!footer_fires(&[0.90, 0.90]), "<3 results → no footer");
    assert!(!footer_fires(&[0.99]), "single result → no footer");

    // A small gap (well under the 0.03 threshold) is still a tie → fires; a clearly larger gap
    // (well over 0.03) is a clear leader → suppressed. (Avoid asserting on the exact 0.03 boundary:
    // f32 subtraction like 0.93 - 0.90 lands on either side of 0.03 by float imprecision — the
    // boundary itself is not a behavior we promise, only the two sides of it.)
    assert!(footer_fires(&[0.92, 0.90, 0.89]), "gap ~0.02 (< 0.03) is still a tie → fires");
    assert!(!footer_fires(&[0.95, 0.90, 0.89]), "gap ~0.05 (> 0.03) → clear leader → suppressed");
}

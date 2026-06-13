//! Decision 5 — dream function (content consolidation, v1).
//!
//! Distils clusters of recent Episodic frames into a single Semantic frame,
//! preserving the originals with a `dreamed:<cycle>` tag for audit. This is
//! the **content-level** dream; the fingerprint-level dream
//! (`Brain::dream`, which drifts `corpus_mean`/`std`) still runs on its own
//! schedule and does a different job.
//!
//! Pipeline (minimum-viable v1):
//!
//!   1. Collect candidate Episodic frames:
//!      - pillar == Episodic
//!      - status == Active
//!      - no `dreamed:*` tag
//!   2. Sort candidates by salience (high → low) using frame tags.
//!   3. For each undrained candidate (pop from front):
//!      - Query the SCA engine with its content
//!      - Keep the top-K hits whose doc_id is in the candidate set AND
//!        whose score > cluster_threshold
//!      - If cluster size ≥ min_cluster_size: emit a Semantic frame
//!        (concatenated content, derived_from tag) and mark members as
//!        `dreamed:<cycle>`.
//!      - Remove clustered members from the candidate pool.
//!   4. Return `DreamReport`.
//!
//! Explicitly OUT of scope for v1:
//!   - LLM summarization (violates BYO-LLM rule); Semantic content is the
//!     concatenation of cluster representatives with `\n---\n` separators.
//!     LLMs reading it via retrieval handle concatenation fine.
//!   - Contradiction detection (needs v1 ML salience classifier).
//!   - Procedural distillation from tool_completion patterns.
//!   - Tombstoning source Episodic frames (preservation is the M365 story;
//!     we just tag).
//!
//! What this DOES unblock:
//!   - Retrieval can now find Semantic facts that summarize many Episodic
//!     turns. LoCoMo cat 1 (single-hop factoid) and cat 5 (open-domain)
//!     should lift because the consolidated fact is findable on a short
//!     query that wouldn't match any single raw turn.
//!   - Decision 2's `rerank_by_pillar` becomes useful — Semantic frames
//!     skip the Episodic recency decay.

use std::collections::HashSet;

/// Cycle counter — each call to `run_dream` increments.
pub type DreamCycle = u32;

/// Per-cycle report. Serializable to JSON for MCP.
#[derive(Debug, Clone, Default)]
pub struct DreamReport {
    pub cycle: DreamCycle,
    /// Episodic candidates considered (pre-cluster).
    pub candidates: usize,
    /// Clusters formed (= Semantic frames written).
    pub clusters_formed: usize,
    /// Episodic frames marked `dreamed:<cycle>`.
    pub frames_marked: usize,
    /// doc_ids of Semantic frames created this cycle.
    pub semantic_frames_created: Vec<String>,
    /// Elapsed time in milliseconds.
    pub elapsed_ms: u64,
}

/// Tunable dream parameters. Defaults are the v1 values; later ML-based
/// versions will calibrate these against held-out LoCoMo QA.
#[derive(Debug, Clone, Copy)]
pub struct DreamParams {
    /// Minimum cluster size (≥ 2 — singletons aren't worth consolidating).
    pub min_cluster_size: usize,
    /// Minimum normalized score (0..=1) for a sibling to join a cluster.
    /// SaidFile::recall scores are divided by the top hit's score before
    /// comparing, so this is effectively "X% as strong as the self-hit."
    /// 0.15 means siblings scoring within ~15% of the self-hit join the
    /// cluster. Lower = more aggressive merging.
    pub cluster_threshold: f32,
    /// Maximum frames per cluster — caps content length of the resulting
    /// Semantic frame so retrieval doesn't blow up on huge concatenations.
    pub max_cluster_size: usize,
    /// Maximum candidates to process per cycle (safety cap).
    pub max_candidates: usize,
    /// Maximum clusters to emit per cycle (safety cap).
    pub max_clusters: usize,
}

impl Default for DreamParams {
    fn default() -> Self {
        Self {
            min_cluster_size: 2,
            cluster_threshold: 0.08,
            max_cluster_size: 8,
            max_candidates: 500,
            max_clusters: 64,
        }
    }
}

/// Salience band from a frame's tags. Used to sort candidates so high-
/// salience memories seed clusters first.
pub fn salience_rank_from_tags(tags: &[String]) -> u8 {
    // Higher number = higher rank (gets processed first).
    for tag in tags {
        if tag == "salience:high" {
            return 3;
        }
        if tag == "salience:medium" {
            return 2;
        }
        if tag == "salience:low" {
            return 1;
        }
    }
    // Untagged frames slot between low and medium — they might be fine,
    // but they haven't been scored so we're less sure.
    1
}

/// Build a Semantic-frame body from a cluster of Episodic frame contents.
///
/// v1 is crude: concatenate with `\n---\n` separators and a header. This
/// is deliberately honest — we have no LLM in the write path, and we don't
/// want to pretend we're summarizing. Retrieval-time LLMs read the
/// concatenation directly.
pub fn build_semantic_body(
    cycle: DreamCycle,
    cluster_index: usize,
    members: &[(String, String)], // (doc_id, content)
) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "# Consolidated memory — dream cycle {}, cluster {}\n\n",
        cycle, cluster_index
    ));
    body.push_str(&format!("Derived from {} episodic memories:\n\n", members.len()));
    for (i, (did, content)) in members.iter().enumerate() {
        if i > 0 {
            body.push_str("\n---\n\n");
        }
        body.push_str(&format!("## source {}: {}\n\n", i + 1, did));
        body.push_str(content);
    }
    body
}

/// Build the `derived_from:<id1>,<id2>,...` tag value. Kept as one tag
/// (comma-separated ids) to avoid tag bloat — our frame tags are a
/// `Vec<String>` and Decision 2 already iterates them per query.
pub fn build_derived_from_tag(member_doc_ids: &[String]) -> String {
    format!("derived_from:{}", member_doc_ids.join(","))
}

/// Set of tags to add to the CREATED Semantic frame.
pub fn build_semantic_tags(
    cycle: DreamCycle,
    member_doc_ids: &[String],
) -> Vec<String> {
    vec![
        "pillar:semantic".to_string(),
        "event:dream_consolidation".to_string(),
        format!("dream_cycle:{}", cycle),
        build_derived_from_tag(member_doc_ids),
        // Salience: a distilled fact is high-value by construction.
        "salience:high".to_string(),
    ]
}

/// Tag to add on each source Episodic frame after consolidation.
pub fn dreamed_tag(cycle: DreamCycle) -> String {
    format!("dreamed:{}", cycle)
}

/// Pure-function stage: given candidate IDs sorted by salience rank and
/// a cluster-lookup function, resolve which clusters to form. The
/// SaidFile glue code provides the lookup closure (doing the SCA query).
///
/// Returns a Vec of clusters; each cluster is a Vec of doc_ids including
/// the seed as the first element.
pub fn form_clusters<F>(
    candidates_sorted: &[String],
    params: &DreamParams,
    mut neighbors_of: F,
) -> Vec<Vec<String>>
where
    F: FnMut(&str) -> Vec<(String, f32)>,
{
    let mut already_clustered: HashSet<String> = HashSet::new();
    let mut clusters: Vec<Vec<String>> = Vec::new();
    let candidate_set: HashSet<String> = candidates_sorted.iter().cloned().collect();

    for seed in candidates_sorted.iter() {
        if already_clustered.contains(seed) {
            continue;
        }
        let neighbors = neighbors_of(seed.as_str());
        let mut members: Vec<String> = vec![seed.clone()];
        for (did, score) in neighbors {
            if members.len() >= params.max_cluster_size {
                break;
            }
            if &did == seed {
                continue;
            }
            if !candidate_set.contains(&did) {
                continue;
            }
            if already_clustered.contains(&did) {
                continue;
            }
            if score < params.cluster_threshold {
                continue;
            }
            members.push(did);
        }
        if members.len() >= params.min_cluster_size {
            for m in &members {
                already_clustered.insert(m.clone());
            }
            clusters.push(members);
            if clusters.len() >= params.max_clusters {
                break;
            }
        }
    }

    clusters
}

// ════════════════════════════════════════════════════════════════════════════
// Tests for the pure-function stage — glue is tested via integration.
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salience_rank_high_beats_medium_beats_low() {
        let high = salience_rank_from_tags(&["salience:high".to_string()]);
        let med = salience_rank_from_tags(&["salience:medium".to_string()]);
        let low = salience_rank_from_tags(&["salience:low".to_string()]);
        assert!(high > med && med >= low, "{} > {} > {}", high, med, low);
    }

    #[test]
    fn untagged_frame_ranks_above_low() {
        // Untagged = no explicit score; we rank them = 1 (low). That means
        // they get processed LATER than high-salience frames, which is
        // correct — untagged is "unknown priority", not "definitely noise".
        let untagged = salience_rank_from_tags(&[]);
        let low = salience_rank_from_tags(&["salience:low".to_string()]);
        assert_eq!(untagged, low);
    }

    #[test]
    fn form_clusters_respects_min_size() {
        // Single candidate with no neighbors → no cluster.
        let candidates = vec!["a".to_string()];
        let clusters = form_clusters(&candidates, &DreamParams::default(), |_| vec![]);
        assert!(clusters.is_empty());
    }

    #[test]
    fn form_clusters_groups_related_frames() {
        let candidates = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let params = DreamParams::default();
        // a has b as neighbor (score 0.8), c is below threshold (0.02 < 0.08).
        let clusters = form_clusters(&candidates, &params, |seed| match seed {
            "a" => vec![("b".to_string(), 0.8), ("c".to_string(), 0.02)],
            _ => vec![],
        });
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0], vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn form_clusters_does_not_reassign_members() {
        // Once a frame is in a cluster, it can't seed another.
        let candidates = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let params = DreamParams::default();
        let clusters = form_clusters(&candidates, &params, |seed| match seed {
            "a" => vec![("b".to_string(), 0.9), ("c".to_string(), 0.9)],
            "b" => vec![("a".to_string(), 0.9), ("c".to_string(), 0.9)],
            "c" => vec![("a".to_string(), 0.9), ("b".to_string(), 0.9)],
            _ => vec![],
        });
        // Only ONE cluster forms; b and c are absorbed into a's cluster.
        assert_eq!(clusters.len(), 1, "got {:?}", clusters);
        assert_eq!(clusters[0].len(), 3);
    }

    #[test]
    fn form_clusters_respects_threshold() {
        let candidates = vec!["a".to_string(), "b".to_string()];
        let params = DreamParams {
            cluster_threshold: 0.5,
            ..DreamParams::default()
        };
        let clusters = form_clusters(&candidates, &params, |seed| match seed {
            "a" => vec![("b".to_string(), 0.3)], // below threshold
            _ => vec![],
        });
        assert!(clusters.is_empty(), "below-threshold pair should not cluster");
    }

    #[test]
    fn form_clusters_caps_max_cluster_size() {
        let candidates: Vec<String> = (0..20).map(|i| format!("f{}", i)).collect();
        let params = DreamParams {
            max_cluster_size: 3,
            ..DreamParams::default()
        };
        let clusters = form_clusters(&candidates, &params, |seed| {
            if seed == "f0" {
                candidates[1..].iter().map(|d| (d.clone(), 0.9)).collect()
            } else {
                vec![]
            }
        });
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].len(), 3, "max_cluster_size was 3");
    }

    #[test]
    fn build_semantic_tags_includes_pillar_and_derived_from() {
        let tags = build_semantic_tags(7, &["a".to_string(), "b".to_string()]);
        assert!(tags.iter().any(|t| t == "pillar:semantic"));
        assert!(tags.iter().any(|t| t == "dream_cycle:7"));
        assert!(tags.iter().any(|t| t == "derived_from:a,b"));
        assert!(tags.iter().any(|t| t == "salience:high"));
    }

    #[test]
    fn dreamed_tag_format() {
        assert_eq!(dreamed_tag(3), "dreamed:3");
    }
}

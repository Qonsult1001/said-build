//! Harvest-at-init: scan an existing repo and auto-learn BLUEPRINTS from REPEATED structures.
//!
//! Grounded in code-clone / idiom-mining research (NiCad, Wrangler, source-code pattern-mining): a code
//! structure is a reusable pattern only at **minimum support = 2** (the universal clone-class default),
//! above a **size floor** (~5-10 lines / a few call-steps), under a **similarity gate** (~70%). One-off
//! functions are NOT patterns and are skipped — high precision, low noise, matching the 80/20 "repeats
//! across entities" definition.
//!
//! Similarity is STRUCTURAL + deterministic (no encoder needed, so harvest works on any build): two
//! functions are the same shape if their ordered CALL-SKELETON (the sequence of things they do) and kind
//! match closely. The blueprint's "sections" = that common ordered skeleton — exactly what a future
//! entity of the shape should reproduce, rendered per language.

use crate::said_file::SaidFile;

/// Minimum occurrences for a structure to count as a reusable pattern. Clone-detection's universal
/// default clone-class size (a pattern must REPEAT). Below this it's a one-off, not a blueprint.
const MIN_SUPPORT: usize = 2;
/// Size floor (in call-skeleton steps): skip trivial functions (getters, one-liners) that have too
/// little structure to be a meaningful blueprint. Mirrors clone tools' min-clone-size (~5-10 lines).
const MIN_SKELETON_STEPS: usize = 2;
/// Quality floor on the COMMON skeleton a cluster produces. The intersection of two functions can
/// collapse to one or two trivial shared calls (noise like ["request","ce"]); a blueprint is only worth
/// storing if the shared structure is substantial. Higher than MIN_SKELETON_STEPS on purpose: an
/// individual function may be small, but a STORED pattern must carry real reusable structure.
const MIN_COMMON_STEPS: usize = 3;
/// Structural similarity gate (Jaccard over the call-skeleton sets). ~0.70 == clone detection's standard
/// "dissimilarity <= 30%". Two functions in the same cluster must share at least this fraction of steps.
const SIM_GATE: f32 = 0.70;

/// Result of a harvest pass.
#[derive(Debug, Default)]
pub struct HarvestReport {
    pub files_scanned: usize,
    pub functions_seen: usize,
    pub clusters_found: usize,
    /// (shape, support, doc_id) for each blueprint learned (or kept-first).
    pub blueprints: Vec<(String, usize, String)>,
}

/// One harvested function's structural signature.
struct FnSig {
    name: String,
    lang: Option<String>,
    /// The ordered call-skeleton — what the function DOES, step by step. The blueprint sections.
    skeleton: Vec<String>,
    /// Where it came from + a sample, so the host agent can read intent when naming phases.
    file: String,
    sample: String,
}

/// A clustered repeated structure handed to the HOST AGENT to name into NL intent phases. `.said` produces
/// this deterministically (the mechanism); the agent supplies the naming (the intelligence) — keeping
/// `.said` LLM-free while producing the 14.15-mandated NL-phase blueprint via `learn_blueprint`.
#[derive(Debug, Clone)]
pub struct HarvestCluster {
    /// A rough verb hint from the member names (e.g. "create") — NOT the final shape; the agent renames.
    pub shape_hint: String,
    /// How many functions share this structure (support; >= MIN_SUPPORT).
    pub support: usize,
    /// The language (from the first member's file extension).
    pub lang: Option<String>,
    /// The common ordered CALL-skeleton (implementation tokens) — the agent maps these to NL intent phases.
    pub calls: Vec<String>,
    /// Member function names (for the agent's context).
    pub members: Vec<String>,
    /// A representative source snippet so the agent can read the INTENT, not just the call names.
    pub sample_code: String,
    /// Source file of the representative member.
    pub sample_file: String,
}

/// The leading VERB of a function name as a clean shape hint. Splits camelCase + snake_case, then takes
/// the first ALPHABETIC token of length >= 3 (so single-letter fragments like "x"/"h"/"1" and prefixes
/// from camelCase splitting don't become garbage shape names like "et<Entity>"). Falls back to the whole
/// name lowercased when nothing qualifies.
fn leading_verb(name: &str) -> String {
    // split camelCase: insert a boundary before each uppercase that follows a lowercase.
    let mut spaced = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c.is_uppercase() && prev_lower { spaced.push(' '); }
        spaced.push(c);
        prev_lower = c.is_lowercase();
    }
    spaced.split(|c: char| c == '_' || c == ' ' || !c.is_alphanumeric())
        .map(|t| t.to_ascii_lowercase())
        .find(|t| t.len() >= 3 && t.chars().all(|c| c.is_ascii_alphabetic()))
        .unwrap_or_else(|| name.to_ascii_lowercase())
}

/// Is this a real call-skeleton step (a meaningful identifier), not a parser fragment? Drops <=2-char
/// tokens ("ce", "x") and anything not starting with a letter — these are extraction noise, not steps.
fn is_real_step(s: &str) -> bool {
    s.len() >= 3 && s.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
}

/// Jaccard similarity of two call-skeletons (set overlap). The clone-detection structural-similarity
/// signal, deterministic — no encoder.
fn skeleton_sim(a: &[String], b: &[String]) -> f32 {
    use std::collections::HashSet;
    let sa: HashSet<&String> = a.iter().collect();
    let sb: HashSet<&String> = b.iter().collect();
    if sa.is_empty() && sb.is_empty() { return 0.0; }
    let inter = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    if union == 0.0 { 0.0 } else { inter / union }
}

/// HARVEST — scan `dir`, extract function structures, cluster the repeated ones (support>=2, sim gate),
/// and learn one blueprint per cluster (keep-first, so re-running init never clobbers edits). `walk` is
/// the caller-supplied file list (reuse the CLI's gitignore-aware walk); `read` reads a path to source.
pub fn harvest_blueprints<W, R>(
    brain: &mut SaidFile,
    files: W,
    read: R,
) -> HarvestReport
where
    W: IntoIterator<Item = std::path::PathBuf>,
    R: Fn(&std::path::Path) -> Option<String>,
{
    let mut report = HarvestReport::default();
    let clusters = harvest_scan(files, read, &mut report.files_scanned, &mut report.functions_seen);
    for c in clusters {
        // FALLBACK form only (headless / no agent): store the raw call-skeleton as sections. This is the
        // form 14.15 flags as suboptimal (call-tokens, not NL intent phases) -- preferred path is
        // agent-in-the-loop: `harvest_scan` -> host agent names NL phases -> learn_blueprint. See 14.15.
        let shape = derive_shape_name(&c.shape_hint, c.support);
        let sections_json = serde_json::json!({ "sections": c.calls }).to_string();
        let doc_id = crate::ask::learn_blueprint(brain, &shape, &sections_json, c.lang.as_deref(), Some("harvest"), false);
        report.clusters_found += 1;
        report.blueprints.push((shape, c.support, doc_id));
    }
    report
}

/// HARVEST SCAN — the deterministic half of agent-in-the-loop harvest. Walk the files, extract function
/// call-skeletons, cluster the repeated ones (support>=2, sim gate, quality floor), and RETURN the clusters
/// (calls + sample code) for the HOST AGENT to name into NL intent phases. Does NOT learn anything: the
/// agent calls `learn_blueprint` with the NL phases (the 14.15-mandated form). `.said` stays LLM-free.
pub fn harvest_scan<W, R>(
    files: W,
    read: R,
    files_scanned: &mut usize,
    functions_seen: &mut usize,
) -> Vec<HarvestCluster>
where
    W: IntoIterator<Item = std::path::PathBuf>,
    R: Fn(&std::path::Path) -> Option<String>,
{
    let mut sigs: Vec<FnSig> = Vec::new();
    for path in files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if crate::ask::ext_to_lang(ext).is_none() { continue; }
        let Some(src) = read(&path) else { continue };
        *files_scanned += 1;
        let lang = crate::ask::ext_to_lang(ext).map(|s| s.to_string());
        let file = path.to_string_lossy().replace('\\', "/");
        for chunk in crate::code_search::ast_chunk(&src, ext) {
            let k = chunk.kind.to_ascii_lowercase();
            if !(k.contains("function") || k.contains("method")) { continue; }
            *functions_seen += 1;
            let skeleton: Vec<String> = chunk.calls.into_iter().filter(|c| is_real_step(c)).collect();
            if skeleton.len() < MIN_SKELETON_STEPS { continue; }
            // keep a trimmed sample of the function body so the agent can read its INTENT.
            let sample: String = chunk.content.lines().take(30).collect::<Vec<_>>().join("\n");
            sigs.push(FnSig { name: chunk.name, lang: lang.clone(), skeleton, file: file.clone(), sample });
        }
    }

    // Greedy clustering by SKELETON SIMILARITY (structural Jaccard >= SIM_GATE), never by the name's verb.
    //
    // BLOCKING (record-linkage standard, Ravikumar VLDB'03 / Papadakis 2013): the old inner loop was
    // `for j in i+1..N`, i.e. all-pairs skeleton_sim — O(N^2). On a real code repo (~14.5k functions,
    // Wonga) that is ~105M Jaccard comparisons each allocating two HashSets, which HUNG harvest in
    // Phase 3. But Jaccard(A,B) >= 0.70 is IMPOSSIBLE unless A and B share at least one call token (a
    // zero intersection => similarity 0). So we index functions by call token and only compare `i`
    // against CANDIDATES that share >=1 token with it. The outer `for i` order and the greedy `used[]`
    // assignment are UNCHANGED, and candidates are visited in ascending index order, so the resulting
    // clusters are BIT-IDENTICAL to the all-pairs version — just without the guaranteed-miss pairs.
    let mut token_index: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for (idx, s) in sigs.iter().enumerate() {
        // dedup this fn's tokens so its posting appears once per token
        let mut seen_tok: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for call in &s.skeleton {
            if seen_tok.insert(call.as_str()) {
                token_index.entry(call.as_str()).or_default().push(idx);
            }
        }
    }
    let mut used = vec![false; sigs.len()];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..sigs.len() {
        if used[i] { continue; }
        let mut members = vec![i]; used[i] = true;
        // Gather candidate j>i that share >=1 call token with i (the only ones that CAN pass the gate).
        let mut cands: Vec<usize> = Vec::new();
        {
            let mut seen_tok: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for call in &sigs[i].skeleton {
                if !seen_tok.insert(call.as_str()) { continue; }
                if let Some(posting) = token_index.get(call.as_str()) {
                    for &j in posting {
                        if j > i && !used[j] { cands.push(j); }
                    }
                }
            }
        }
        cands.sort_unstable();
        cands.dedup();
        for j in cands {
            if used[j] { continue; } // may have been claimed by an earlier candidate this round
            if skeleton_sim(&sigs[i].skeleton, &sigs[j].skeleton) >= SIM_GATE { members.push(j); used[j] = true; }
        }
        groups.push(members);
    }

    let mut out = Vec::new();
    for members in groups {
        if members.len() < MIN_SUPPORT { continue; }
        let common = common_skeleton(&members.iter().map(|&j| &sigs[j].skeleton).collect::<Vec<_>>());
        if common.len() < MIN_COMMON_STEPS { continue; }
        let rep = &sigs[members[0]];
        out.push(HarvestCluster {
            shape_hint: leading_verb(&rep.name),
            support: members.len(),
            lang: rep.lang.clone(),
            calls: common,
            members: members.iter().map(|&j| sigs[j].name.clone()).collect(),
            sample_code: rep.sample.clone(),
            sample_file: rep.file.clone(),
        });
    }
    out
}

/// Steps present in ALL cluster members, kept in the first member's order (the common skeleton).
fn common_skeleton(members: &[&Vec<String>]) -> Vec<String> {
    let Some(first) = members.first() else { return Vec::new() };
    use std::collections::HashSet;
    first.iter()
        .filter(|step| members.iter().all(|m| {
            let set: HashSet<&String> = m.iter().collect();
            set.contains(step)
        }))
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .fold(Vec::new(), |mut acc, s| { if !acc.contains(&s) { acc.push(s); } acc }) // dedup, keep order
}

/// A human-readable shape name = the intent KEY (clean), not a place to stuff the skeleton. "create_invoice"
/// -> "create<Entity>". Recall keys on the intent FINGERPRINT (like recall_fix's action residue), NOT on
/// lexical name content — so the key stays clean and the skeleton lives in the sections (the payload).
fn derive_shape_name(rep_name: &str, support: usize) -> String {
    format!("{}<Entity> (harvested, {}x)", leading_verb(rep_name), support)
}

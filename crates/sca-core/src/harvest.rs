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
}

/// Normalize a function name to its SHAPE intent, dropping the entity noun so `create_invoice` and
/// `create_order` collapse to the same shape key "create". Uses the action residue (strips CamelCase /
/// snake entity tokens) plus the leading verb.
fn shape_key(name: &str, skeleton: &[String]) -> String {
    // plus the action residue of the skeleton (intent words, entity nouns stripped).
    let residue = crate::ask::action_residue(&skeleton.join(" "));
    let mut toks: Vec<&str> = residue.split_whitespace().collect();
    toks.sort_unstable();
    toks.dedup();
    format!("{}|{}", leading_verb(name), toks.join(" "))
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
    let mut sigs: Vec<FnSig> = Vec::new();
    let mut report = HarvestReport::default();

    for path in files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if crate::ask::ext_to_lang(ext).is_none() { continue; }
        let Some(src) = read(&path) else { continue };
        report.files_scanned += 1;
        let lang = crate::ask::ext_to_lang(ext).map(|s| s.to_string());
        for chunk in crate::code_search::ast_chunk(&src, ext) {
            // only functions/methods carry a reusable call-skeleton; skip types/structs/enums.
            let k = chunk.kind.to_ascii_lowercase();
            if !(k.contains("function") || k.contains("method")) { continue; }
            report.functions_seen += 1;
            if chunk.calls.len() < MIN_SKELETON_STEPS { continue; } // size floor
            sigs.push(FnSig { name: chunk.name, lang: lang.clone(), skeleton: chunk.calls });
        }
    }

    // Greedy clustering by shape_key first (cheap bucket), then split buckets by the skeleton sim gate.
    use std::collections::HashMap;
    let mut buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, s) in sigs.iter().enumerate() {
        buckets.entry(shape_key(&s.name, &s.skeleton)).or_default().push(i);
    }

    for (_key, idxs) in buckets {
        if idxs.len() < MIN_SUPPORT { continue; } // not a repeated structure -> skip
        // verify the members are actually similar (sim gate), not just same verb by luck.
        let rep = &sigs[idxs[0]];
        let members: Vec<usize> = idxs.iter().cloned()
            .filter(|&j| skeleton_sim(&rep.skeleton, &sigs[j].skeleton) >= SIM_GATE)
            .collect();
        if members.len() < MIN_SUPPORT { continue; }

        // the blueprint's sections = the COMMON ordered steps across the cluster (intersection, in the
        // representative's order) — what every entity of this shape reproduces.
        let common = common_skeleton(&members.iter().map(|&j| &sigs[j].skeleton).collect::<Vec<_>>());
        if common.len() < MIN_SKELETON_STEPS { continue; }

        let shape = derive_shape_name(&sigs[members[0]].name, members.len());
        let sections_json = serde_json::json!({ "sections": common }).to_string();
        let lang = sigs[members[0]].lang.as_deref();
        // keep-first: harvest never overwrites an existing (possibly hand-promoted) blueprint.
        let doc_id = crate::ask::learn_blueprint(brain, &shape, &sections_json, lang, Some("harvest"), false);
        report.clusters_found += 1;
        report.blueprints.push((shape, members.len(), doc_id));
    }
    report
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

/// A human-readable shape name from the representative function name. "create_invoice" -> "create<Entity>".
fn derive_shape_name(rep_name: &str, support: usize) -> String {
    format!("{}<Entity> (harvested, {}x)", leading_verb(rep_name), support)
}

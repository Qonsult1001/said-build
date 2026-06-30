//! Memory-evidence frames — the world-class memory standard (docs/said-structure/31).
//!
//! A NEW kind of `.said` memory ALONGSIDE coding-fixes (the 20%), blueprints (the 80%), and work-state
//! (compaction survival). It mirrors how Claude Code / Kimi / Gemini actually store + recall memory
//! (verified from their source): a memory is a CLAIM with structured EVIDENCE, recalled by a
//! MANIFEST (name + description) + the host LLM selecting — NOT by nearest-vector recall@1 (which has a
//! proven mathematical ceiling at a fixed embedding dim, arXiv:2508.21038).
//!
//! Each memory frame carries:
//!   - a `name` (doc_id slug) + `description` (the title) = the MANIFEST entry the LLM matches on,
//!   - a `type` (user/feedback/project/reference — Claude's 4 types, already mirrored in .said),
//!   - the CLAIM body (the agent's own words),
//!   - `link:<evidence>` edges (commit / source frame / concept) — the OKF wiki-tree to the SOURCE, so
//!     traversal (frames_linking_concept, the ask Engine-D bridge) REACHES the evidence from any entry
//!     point. That reachability is why recall@1 stops being the limiting factor.
//!
//! This module does NOT touch the fix / blueprint / work-state stores — they coexist in one brain,
//! separated by their own kind tags. `recall_coding_fixes` (FIX_KIND_TAG) and `recall_blueprints`
//! (BLUEPRINT_KIND_TAG) are unchanged.

use crate::frames::Pillar;
use crate::said_file::SaidFile;

/// Marker tag for memory-evidence frames — one source of truth, distinct from fix/blueprint/workstate.
pub const MEMORY_KIND_TAG: &str = "kind:memory";
pub const MEMORY_ID_PREFIX: &str = "memory::";
/// The four memory types (Claude `memoryTypes.ts`, mirrored in .said MEMORY.md).
pub const MEMORY_TYPES: &[&str] = &["user", "feedback", "project", "reference"];

fn frame_id(name: &str) -> String {
    format!("{}{}", MEMORY_ID_PREFIX, name.trim())
}

/// Normalize an evidence token into a `link:<concept>` edge (the OKF graph namespace). A git commit
/// `edd8a3c` -> `link:commit-edd8a3c`; a free concept `recall` -> `link:recall`. Lowercased single token.
fn evidence_link(ev: &str) -> String {
    let e = ev.trim().to_lowercase();
    // a hex-ish commit hash gets a commit- prefix so it reads as evidence, not a random concept.
    let looks_commit = e.len() >= 7 && e.len() <= 40 && e.chars().all(|c| c.is_ascii_hexdigit());
    if looks_commit { format!("link:commit-{}", e) } else { format!("link:{}", e.replace(' ', "-")) }
}

/// One manifest entry — what the host LLM sees when SELECTING (mirrors Claude's name+description scan).
#[derive(Debug, Clone)]
pub struct MemoryManifestEntry {
    pub doc_id: String,
    pub name: String,
    pub description: String,
    pub mtype: String,
}

/// A recalled memory — the full claim body plus its evidence links (the SOURCE pointers to verify).
#[derive(Debug, Clone)]
pub struct RecalledMemory {
    pub doc_id: String,
    pub claim: String,
    pub evidence_links: Vec<String>,
}

/// SAVE a memory-evidence frame (the doc-31 standard). `name` = manifest slug, `description` = the
/// one-line relevance hook (the title), `mtype` = one of MEMORY_TYPES, `claim` = the body (agent's
/// words, ideally "fact + **Why:** + **How to apply:**"), `evidence` = source tokens (commit hashes,
/// file/symbol names, concepts) recorded as `link:` edges so the OKF graph reaches the source.
/// Returns the frame doc_id. Latest-wins on the same name (re-saving updates the claim).
pub fn save_memory(
    brain: &mut SaidFile,
    name: &str,
    description: &str,
    mtype: &str,
    claim: &str,
    evidence: &[String],
) -> String {
    let id = frame_id(name);
    let mtype = if MEMORY_TYPES.contains(&mtype.trim()) { mtype.trim() } else { "project" };
    let mut tags = vec![
        MEMORY_KIND_TAG.to_string(),
        format!("memtype:{}", mtype),
    ];
    // user-type memories are GLOBAL (cross-project, like preferences); the rest carry the project scope
    // when SAID_PROJECT is set (mirrors .said's project scoping + Claude's user/team split).
    if mtype != "user" {
        if let Some(p) = crate::project::current_project() {
            tags.push(crate::project::project_tag(&p));
        }
    }
    // evidence -> link: edges (the wiki-tree to the source). De-duplicated.
    for ev in evidence {
        if ev.trim().is_empty() { continue; }
        let link = evidence_link(ev);
        if !tags.contains(&link) { tags.push(link); }
    }
    // Episodic pillar: a memory is a dated observation/claim (decays, recency-weighted) — distinct from
    // Procedural (fixes/blueprints). Title = the description (the manifest hook).
    brain.remember_with_pillar(Some(&id), claim.trim(), Some(description.trim()), Pillar::Episodic, tags);
    let _ = brain.build_index();
    id
}

/// THE MANIFEST — every memory frame's (name, description, type), the list the host LLM selects from.
/// This is the recall ENTRY point (Claude `scanMemoryFiles` + `findRelevantMemories`): no vector ranking,
/// the LLM picks relevant names from descriptions, then we follow each selected memory's evidence links.
pub fn manifest(brain: &SaidFile) -> Vec<MemoryManifestEntry> {
    let ids: Vec<String> = brain.frames.active_doc_ids().iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    for id in ids {
        let Some(meta) = brain.frames.get_meta(&id) else { continue };
        if !meta.tags.iter().any(|t| t == MEMORY_KIND_TAG) { continue; }
        let mtype = meta.tags.iter()
            .find_map(|t| t.strip_prefix("memtype:").map(|s| s.to_string()))
            .unwrap_or_else(|| "project".to_string());
        let name = id.strip_prefix(MEMORY_ID_PREFIX).unwrap_or(&id).to_string();
        out.push(MemoryManifestEntry {
            doc_id: id.clone(),
            name,
            description: meta.title.clone().unwrap_or_default(),
            mtype,
        });
    }
    out
}

/// READ a memory by name, with its evidence links (the source pointers to verify before acting —
/// Claude's drift rule). None if no such memory.
pub fn recall_memory(brain: &mut SaidFile, name: &str) -> Option<RecalledMemory> {
    let id = frame_id(name);
    let links: Vec<String> = brain.frames.get_meta(&id)
        .map(|m| m.tags.iter().filter(|t| t.starts_with("link:")).cloned().collect())
        .unwrap_or_default();
    let claim = brain.read(&id).filter(|s| !s.trim().is_empty())?;
    Some(RecalledMemory { doc_id: id, claim, evidence_links: links })
}

/// EVIDENCE TRAVERSAL — every frame in the brain that shares this memory's evidence concept (the OKF
/// reachability: from a memory, reach its source frames + sibling memories about the same evidence).
/// `concept` is an evidence token (e.g. "ask.rs" or "commit-edd8a3c"); returns the connected doc_ids.
pub fn evidence_neighbors(brain: &SaidFile, concept: &str) -> Vec<String> {
    let c = evidence_link(concept);
    let want = c.strip_prefix("link:").unwrap_or(&c);
    brain.frames_linking_concept(want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_saves_with_evidence_links_and_is_recalled_via_manifest() {
        let p = std::env::temp_dir().join(format!("mem_ev_{}.said", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let mut b = SaidFile::create(p.to_string_lossy().as_ref());
        assert!(b.auto_load_encoder());

        // SAVE a project memory with a claim + evidence (a commit + a source frame).
        save_memory(
            &mut b,
            "kind-axis-fix",
            "kind-axis pillar scope lifted recall@10 35%->85%",
            "project",
            "Classifying pillar at ingest + --pillar scope fixed the within-project recall problem.\n\
             **Why:** commits drowned by 93% code frames.\n**How to apply:** scope which-commit queries to episodic.",
            &["edd8a3c".to_string(), "ask.rs".to_string()],
        );

        // MANIFEST: the memory appears as a selectable (name, description, type) entry.
        let man = manifest(&b);
        let entry = man.iter().find(|e| e.name == "kind-axis-fix").expect("memory in manifest");
        assert_eq!(entry.mtype, "project");
        assert!(entry.description.contains("recall@10"), "description is the LLM's selection hook");

        // RECALL by name returns the claim verbatim + the evidence links (source pointers).
        let rec = recall_memory(&mut b, "kind-axis-fix").expect("recall the memory");
        assert!(rec.claim.contains("**Why:**"), "claim body preserved");
        assert!(rec.evidence_links.iter().any(|l| l == "link:commit-edd8a3c"), "commit evidence linked");
        assert!(rec.evidence_links.iter().any(|l| l == "link:ask.rs"), "source-frame evidence linked");

        // EVIDENCE TRAVERSAL: a frame sharing the commit evidence is reachable (the OKF reachability).
        save_memory(&mut b, "okf-default", "OKF default-on", "project",
            "Made OKF concept graph default-on.", &["edd8a3c".to_string()]);
        let neighbors = evidence_neighbors(&b, "edd8a3c");
        assert!(neighbors.len() >= 2, "both memories sharing commit-edd8a3c are reachable, got {:?}", neighbors);

        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
    }

    #[test]
    fn memory_coexists_with_fix_and_blueprint_in_one_brain() {
        let p = std::env::temp_dir().join(format!("mem_coexist_{}.said", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let mut b = SaidFile::create(p.to_string_lossy().as_ref());
        assert!(b.auto_load_encoder());

        // one brain holding THREE kinds at once
        crate::ask::learn_coding_fix(&mut b, "fix an LRU cache O(1)", "use HashMap + DLL", "[]", None);
        crate::ask::learn_blueprint(&mut b, "Create<Entity> endpoint",
            "{\"sections\":[\"validate\",\"persist\",\"respond\"]}", None, None, false);
        save_memory(&mut b, "team-note", "merge freeze 2026-03-05", "project",
            "Merge freeze begins 2026-03-05 for the mobile release cut.", &["release".to_string()]);

        // each kind recalls through its OWN path, unaffected by the others
        let fix = crate::ask::recall_coding_fix(&mut b, "implement an LRU cache", 0.0);
        assert!(fix.is_some(), "coding-fix store still works alongside memory frames");
        let bp = crate::ask::recall_blueprints(&mut b, "create a new entity endpoint", 5, 0.0);
        assert!(!bp.is_empty(), "blueprint store still works alongside memory frames");
        let mem = recall_memory(&mut b, "team-note");
        assert!(mem.is_some(), "memory store works in the same brain");
        // the memory manifest contains ONLY the memory frame, not the fix/blueprint
        let man = manifest(&b);
        assert_eq!(man.len(), 1, "manifest lists only memory-kind frames, not fixes/blueprints");
        assert_eq!(man[0].name, "team-note");

        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
    }
}

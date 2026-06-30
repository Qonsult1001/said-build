//! Project scoping for CODE-related memories (docs/said-structure/28-token-value-and-scoping.md).
//!
//! The keystone doc 28 named missing: the GENERIC ingest path never wrote `project:<name>`, so
//! commits/code/repo-docs landed untagged and recall couldn't scope to one project (measured: 8%
//! recall@1 on a 4,689-memory mixed corpus, because 93% was code competing with every query).
//!
//! BOUNDARY (owner 2026-06-30): this is for CODE-related project ingest ONLY. Bare general
//! `remember`s (personal facts, cross-cutting notes) must stay GLOBAL — un-scoped, reachable from
//! any project — so this module is called only from the code-ingest paths (`add --dir` over a repo,
//! and mirrors the `project:` tag `learn_coding_fix`/blueprints already write), never from the bare
//! `remember`/`add <text>` path.
//!
//! Scope is OPT-IN and additive: with `SAID_PROJECT` unset, ingest behaves exactly as before (no
//! tag). With `SAID_RECALL_PROJECT` set, recall returns that project's memories PLUS globals (untagged),
//! and never another project's; unset => everything (cross-project reuse / federation stays possible).

use crate::frames::Pillar;
use crate::said_file::SaidFile;

/// Tag prefix marking a memory's owning code project. Mirrors the tag `learn_coding_fix` writes.
pub const PROJECT_TAG_PREFIX: &str = "project:";
/// Concept-link prefix that wiki-links every memory of a project into one cluster, so the recall
/// graph fan-out (doc 3.5 Layer 6 / `frames_linking_concept`) reaches the whole project from any hit.
pub const PROJECT_LINK_PREFIX: &str = "link:project-";

/// Resolve the active code project from `SAID_PROJECT` (set by CLI/MCP/orchestrator from the repo/cwd
/// stem). None when unset/blank => ingest stays global (today's behavior).
pub fn current_project() -> Option<String> {
    std::env::var("SAID_PROJECT").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The scope requested for recall, from `SAID_RECALL_PROJECT`. None => no constraint.
pub fn recall_project() -> Option<String> {
    std::env::var("SAID_RECALL_PROJECT").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The `project:<name>` tag for a project.
pub fn project_tag(project: &str) -> String {
    format!("{}{}", PROJECT_TAG_PREFIX, project.trim())
}

/// The shared `link:project-<name>` wiki concept edge for a project.
pub fn project_link_tag(project: &str) -> String {
    format!("{}{}", PROJECT_LINK_PREFIX, project.trim().to_lowercase())
}

/// True if a frame's tags carry the given project (or it is global/untagged when `want` asks for that).
fn frame_in_project(tags: &[String], project: &str) -> bool {
    tags.iter().any(|t| t == &project_tag(project))
}

/// Whether a memory passes a project scope: matches the wanted project OR is GLOBAL (no project tag).
/// This is the recall predicate — scoped recall sees the project's own memories plus globals, never
/// another project's. Mirrors the `lang:`/`project:` filter already in `recall_coding_fixes`.
pub fn passes_scope(tags: &[String], want: Option<&str>) -> bool {
    match want {
        None => true, // no constraint
        Some(p) => {
            let has_any_project = tags.iter().any(|t| t.starts_with(PROJECT_TAG_PREFIX));
            !has_any_project || frame_in_project(tags, p)
        }
    }
}

/// INGEST a CODE-related project memory: stores `content` under `doc_id` and, when a project is
/// active, tags it `project:<name>` + the `link:project-<name>` wiki edge so it joins the project
/// cluster. With no active project the memory is stored plain (global) — identical to before.
/// Returns the doc_id. Code memories use the Semantic pillar (repo docs/commits/code summaries).
pub fn ingest_project_memory(
    brain: &mut SaidFile,
    doc_id: &str,
    content: &str,
    title: Option<&str>,
) -> String {
    let mut tags = Vec::new();
    if let Some(project) = current_project() {
        tags.push(project_tag(&project));
        tags.push(project_link_tag(&project));
    }
    brain.remember_with_pillar(Some(doc_id), content, title, Pillar::Semantic, tags);
    doc_id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_predicate_keeps_globals_and_own_project_excludes_others() {
        let global: Vec<String> = vec!["kind:note".into()];
        let mine: Vec<String> = vec![project_tag("said-build")];
        let other: Vec<String> = vec![project_tag("said-echo")];

        // unset scope => everything passes
        assert!(passes_scope(&global, None));
        assert!(passes_scope(&mine, None));
        assert!(passes_scope(&other, None));

        // scoped to said-build => own + globals pass; other project excluded
        assert!(passes_scope(&global, Some("said-build")), "globals must stay reachable when scoped");
        assert!(passes_scope(&mine, Some("said-build")));
        assert!(!passes_scope(&other, Some("said-build")), "another project must be excluded");
    }
}

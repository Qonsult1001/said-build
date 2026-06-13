//! Library API called by the `said-mcp` forge tools.
//!
//! Each function corresponds to one MCP tool. Output shapes are
//! JSON-serializable so the MCP side is a thin marshaller.

use crate::frame::BrainIo;
use crate::projection::is_complete;
use crate::{filter, forge_tag, sanitize_slug, ForgeError, ForgeResult, Story, StoryKind};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FORGE_GET_TOKEN_CAP: usize = 25_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryListItem {
    pub slug: String,
    pub title: String,
    pub kind: StoryKind,
    pub status: String,
    pub tags: Vec<String>,
}

/// List stories in the current directive. `brain` must implement BrainIo so
/// we can look up the latest directive and iterate its stories.
pub fn list_stories<B: BrainIo>(
    brain: &mut B,
    project_root: &Path,
    filter_expr: Option<&str>,
) -> ForgeResult<Vec<StoryListItem>> {
    let Some(hash) = crate::frame::latest_directive_hash(brain) else {
        return Ok(Vec::new());
    };
    let f = match filter_expr {
        Some(expr) => Some(filter::parse(expr)?),
        None => None,
    };
    let stories = read_stories(brain, &hash);
    let mut out = Vec::new();
    for s in stories {
        if let Some(ff) = &f {
            if !filter::matches(&s, ff) {
                continue;
            }
        }
        let status = if is_complete(project_root, &s.slug) {
            "completed"
        } else {
            "pending"
        };
        let tags: Vec<String> = s
            .fields
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        out.push(StoryListItem {
            slug: s.slug.clone(),
            title: s.title.clone(),
            kind: s.kind,
            status: status.into(),
            tags,
        });
    }
    Ok(out)
}

/// γ format: bundled markdown concatenating spec + plan + tasks + brain
/// per slug, under explicit headings. Total output capped at
/// FORGE_GET_TOKEN_CAP tokens via proportional truncation (Jina pattern).
pub fn get_bundled<B: BrainIo>(brain: &mut B, slugs: &[String]) -> ForgeResult<String> {
    let hash = crate::frame::latest_directive_hash(brain)
        .ok_or_else(|| ForgeError::Config("no directive loaded".into()))?;
    let mut raw = String::new();
    for slug_in in slugs {
        let slug = sanitize_slug(slug_in);
        raw.push_str(&format!("\n# Story: {}\n\n", slug));
        for ty in ["spec", "plan", "tasks", "brain"] {
            let tag = forge_tag(ty, &hash, &slug, None, None);
            let body = brain
                .find_body_by_tag(&tag)
                .unwrap_or_else(|| format!("(no {} frame yet)", ty));
            raw.push_str(&format!("\n## {} — {}\n\n", ty, slug));
            raw.push_str(&body);
            raw.push('\n');
        }
    }
    Ok(apply_token_cap(&raw, FORGE_GET_TOKEN_CAP))
}

/// Proportional truncation. Each section keeps ~cap/4 chars; oversize
/// sections get an explicit `[TRUNCATED — call get <frame-id>]` marker.
pub fn apply_token_cap(raw: &str, token_cap: usize) -> String {
    let char_cap = token_cap * 4;
    if raw.len() <= char_cap {
        return raw.to_string();
    }
    // Split on "\n## " markers to find sections.
    let mut sections: Vec<&str> = Vec::new();
    let mut last = 0usize;
    for (i, _) in raw.match_indices("\n## ") {
        sections.push(&raw[last..i]);
        last = i;
    }
    sections.push(&raw[last..]);
    let per_section = char_cap / sections.len().max(1);
    let mut out = String::with_capacity(char_cap + 200);
    for s in sections {
        if s.len() <= per_section {
            out.push_str(s);
        } else {
            // Truncate at a char boundary, not a byte boundary.
            let mut end = per_section;
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
            out.push_str(&s[..end]);
            out.push_str("\n[TRUNCATED — call get <frame-id> for full content]\n");
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryStatusItem {
    pub slug: String,
    pub status: String,
    pub last_run_n: u32,
    pub last_error: Option<String>,
}

pub fn status<B: BrainIo>(
    brain: &mut B,
    project_root: &Path,
    slugs: &[String],
) -> ForgeResult<Vec<StoryStatusItem>> {
    let hash = crate::frame::latest_directive_hash(brain)
        .ok_or_else(|| ForgeError::Config("no directive loaded".into()))?;
    let mut out = Vec::new();
    for s in slugs {
        let slug = sanitize_slug(s);
        let run_n = crate::frame::latest_run_n(brain, &hash, &slug);
        let meta_tag = forge_tag("run", &hash, &slug, Some(run_n), Some("meta"));
        let last_error = brain.find_body_by_tag(&meta_tag).and_then(|body| {
            serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str().map(String::from)))
        });
        let status = if is_complete(project_root, &slug) {
            "completed"
        } else if run_n > 0 {
            "incomplete"
        } else {
            "pending"
        };
        out.push(StoryStatusItem {
            slug,
            status: status.into(),
            last_run_n: run_n,
            last_error,
        });
    }
    Ok(out)
}

/// Read all story frames for a directive hash. Used by list_stories.
fn read_stories<B: BrainIo>(brain: &mut B, hash: &str) -> Vec<Story> {
    let prefix = format!("forge:story:{}:", hash);
    let matching_tags: Vec<String> = brain
        .iter_tags()
        .into_iter()
        .flat_map(|(_, tags, _)| tags.into_iter())
        .filter(|t| t.starts_with(&prefix))
        .collect();
    let mut out = Vec::new();
    for tag in matching_tags {
        if let Some(body) = brain.find_body_by_tag(&tag) {
            if let Ok(s) = serde_json::from_str::<Story>(&body) {
                out.push(s);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_token_cap_leaves_short_inputs_alone() {
        let s = "## a\n123\n## b\n456";
        assert_eq!(apply_token_cap(s, 25_000), s);
    }

    #[test]
    fn apply_token_cap_truncates_long_sections_proportionally() {
        let long = "\n## a\n".to_string() + &"x".repeat(200_000) + "\n## b\n" + &"y".repeat(200_000);
        let out = apply_token_cap(&long, 10_000); // char_cap = 40_000
        assert!(out.len() < long.len());
        assert!(out.contains("[TRUNCATED"));
    }

    #[test]
    fn apply_token_cap_handles_multibyte_utf8_boundaries() {
        // String with multi-byte UTF-8 chars that may fall on truncation boundary.
        let base = "\n## a\n".to_string() + &"é".repeat(30_000);
        let out = apply_token_cap(&base, 1_000); // char_cap = 4_000
        // Must not panic on char boundary.
        assert!(out.contains("[TRUNCATED"));
    }
}

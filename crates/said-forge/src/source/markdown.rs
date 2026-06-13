//! Markdown adapter — two auto-detected modes:
//! - **Checklist**: if the file contains `- [ ]` / `- [x]` items, each item is a story.
//! - **Heading**: otherwise, each heading at the configured level (default H2) is a story.
//!
//! Per spec §7.2.

use crate::source::{DirectiveSource, SourceCapabilities};
use crate::{sanitize_slug, DirectiveDoc, DirectiveMeta, ForgeError, ForgeResult, Story, StoryKind};
use async_trait::async_trait;

pub struct MarkdownSource {
    pub heading_level: u8,
}

impl Default for MarkdownSource {
    fn default() -> Self {
        Self { heading_level: 2 }
    }
}

#[async_trait]
impl DirectiveSource for MarkdownSource {
    fn name(&self) -> &'static str { "markdown" }
    fn supported_extensions(&self) -> &'static [&'static str] { &["md", "markdown"] }
    fn supported_url_schemes(&self) -> &'static [&'static str] { &[] }
    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            supports_incremental_reload: true,
            supports_filter_dsl: false,
            provides_schema_fields: false,
            requires_network: false,
        }
    }

    fn detect(&self, path_or_url: &str) -> bool {
        let lower = path_or_url.to_lowercase();
        lower.ends_with(".md") || lower.ends_with(".markdown")
    }

    async fn load(&self, path_or_url: &str, operator: &str) -> ForgeResult<DirectiveDoc> {
        let raw = std::fs::read(path_or_url).map_err(|cause| ForgeError::Io {
            path: path_or_url.to_string(),
            cause,
        })?;
        Ok(DirectiveDoc {
            source: path_or_url.to_string(),
            adapter: "markdown".into(),
            raw,
            meta: DirectiveMeta {
                loaded_at_utc: now_utc(),
                operator: operator.to_string(),
                content_type: Some("text/markdown".into()),
            },
        })
    }

    fn extract_stories(&self, doc: &DirectiveDoc) -> ForgeResult<Vec<Story>> {
        let text = std::str::from_utf8(&doc.raw).map_err(|e| ForgeError::Parse {
            path: doc.source.clone(),
            message: format!("utf-8: {}", e),
        })?;
        let hash = crate::forge_hash(&doc.source);

        if has_checklist_items(text) {
            extract_checklist(text, &hash, &doc.source)
        } else {
            extract_headings(text, self.heading_level, &hash, &doc.source)
        }
    }
}

fn now_utc() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn has_checklist_items(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]")
    })
}

fn extract_checklist(text: &str, hash: &str, source: &str) -> ForgeResult<Vec<Story>> {
    let mut stories = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let (content, checked) = if let Some(rest) = trimmed.strip_prefix("- [ ]") {
            (rest, false)
        } else if let Some(rest) = trimmed.strip_prefix("- [x]") {
            (rest, true)
        } else if let Some(rest) = trimmed.strip_prefix("- [X]") {
            (rest, true)
        } else {
            continue;
        };
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        let slug = sanitize_slug(content);
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("checked".into(), serde_json::Value::Bool(checked));
        fields.insert(
            "line".into(),
            serde_json::Value::Number((idx as u64 + 1).into()),
        );
        stories.push(Story {
            slug,
            title: content.to_string(),
            raw_text: content.to_string(),
            kind: StoryKind::Requirement,
            fields,
            directive_hash: hash.to_string(),
            source_adapter: "markdown".into(),
            source_anchor: format!("line:{}", idx + 1),
        });
    }
    if stories.is_empty() {
        return Err(ForgeError::Parse {
            path: source.to_string(),
            message: "checklist mode detected but no items extracted".into(),
        });
    }
    Ok(stories)
}

fn extract_headings(
    text: &str,
    target_level: u8,
    hash: &str,
    source: &str,
) -> ForgeResult<Vec<Story>> {
    let prefix = "#".repeat(target_level as usize);
    let marker = format!("{} ", prefix);
    let mut stories = Vec::new();
    let mut current_heading: Option<(usize, String)> = None;
    let mut body_buf = String::new();

    fn flush(
        stories: &mut Vec<Story>,
        current: &mut Option<(usize, String)>,
        body: &mut String,
        hash: &str,
    ) {
        if let Some((idx, title)) = current.take() {
            let raw_text = format!("{}\n\n{}", title, body.trim()).trim().to_string();
            let slug = sanitize_slug(&title);
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("section_path".into(), serde_json::Value::String(title.clone()));
            fields.insert(
                "line".into(),
                serde_json::Value::Number((idx as u64 + 1).into()),
            );
            stories.push(Story {
                slug,
                title: title.clone(),
                raw_text,
                kind: StoryKind::Requirement,
                fields,
                directive_hash: hash.to_string(),
                source_adapter: "markdown".into(),
                source_anchor: format!("line:{}", idx + 1),
            });
        }
        body.clear();
    }

    for (idx, line) in text.lines().enumerate() {
        if let Some(rest) = line.strip_prefix(&marker) {
            flush(&mut stories, &mut current_heading, &mut body_buf, hash);
            current_heading = Some((idx, rest.trim().to_string()));
        } else if line.starts_with('#') {
            // A heading at a different level — emit current if any.
            flush(&mut stories, &mut current_heading, &mut body_buf, hash);
        } else if current_heading.is_some() {
            body_buf.push_str(line);
            body_buf.push('\n');
        }
    }
    flush(&mut stories, &mut current_heading, &mut body_buf, hash);

    if stories.is_empty() {
        return Err(ForgeError::Parse {
            path: source.to_string(),
            message: format!("no H{} headings found", target_level),
        });
    }
    Ok(stories)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn detects_md_by_extension() {
        let s = MarkdownSource::default();
        assert!(s.detect("requirements.md"));
        assert!(s.detect("any.markdown"));
        assert!(!s.detect("petstore.yaml"));
        assert!(!s.detect("data.csv"));
    }

    #[tokio::test]
    async fn extracts_five_checklist_items() {
        let s = MarkdownSource::default();
        let doc = s.load(fixture("requirements.md").to_str().unwrap(), "test").await.unwrap();
        let stories = s.extract_stories(&doc).unwrap();
        assert_eq!(stories.len(), 5);
        let create = stories
            .iter()
            .find(|st| st.slug == "create-customer-account-with-kyc-verification")
            .unwrap();
        assert_eq!(create.kind, StoryKind::Requirement);
        assert_eq!(create.fields.get("checked"), Some(&serde_json::Value::Bool(false)));
        let view = stories
            .iter()
            .find(|st| st.slug == "view-transaction-history-for-a-given-account")
            .unwrap();
        assert_eq!(view.fields.get("checked"), Some(&serde_json::Value::Bool(true)));
    }

    #[tokio::test]
    async fn extracts_three_h2_headings() {
        let s = MarkdownSource::default();
        let doc = s.load(fixture("requirements-headings.md").to_str().unwrap(), "test").await.unwrap();
        let stories = s.extract_stories(&doc).unwrap();
        assert_eq!(stories.len(), 3);
        let titles: Vec<_> = stories.iter().map(|s| s.title.clone()).collect();
        assert_eq!(
            titles,
            vec!["Daily reconciliation", "Month-end close", "Sanctions screening"]
        );
        let recon = stories.iter().find(|s| s.slug == "daily-reconciliation").unwrap();
        assert!(recon.raw_text.contains("reconcile every card transaction"));
    }

    #[test]
    fn heading_level_is_configurable() {
        let source = MarkdownSource { heading_level: 3 };
        let doc = DirectiveDoc {
            source: "mem".into(),
            adapter: "markdown".into(),
            raw: "# ignored\n\n### Deep A\ntext A\n### Deep B\ntext B\n".as_bytes().to_vec(),
            meta: DirectiveMeta {
                loaded_at_utc: 0,
                operator: "t".into(),
                content_type: None,
            },
        };
        let stories = source.extract_stories(&doc).unwrap();
        assert_eq!(stories.len(), 2);
    }

    #[test]
    fn errors_on_empty_markdown() {
        let s = MarkdownSource::default();
        let doc = DirectiveDoc {
            source: "mem".into(),
            adapter: "markdown".into(),
            raw: "# only one top heading\n".as_bytes().to_vec(),
            meta: DirectiveMeta { loaded_at_utc: 0, operator: "t".into(), content_type: None },
        };
        let err = s.extract_stories(&doc).unwrap_err();
        assert!(matches!(err, ForgeError::Parse { .. }));
    }
}

//! Story + StoryKind + DirectiveDoc types.
//!
//! Per spec §6.3. Every story has a thin common core (slug/title/raw_text/kind)
//! plus flat source-specific fields, following the Elastic Connectors pattern
//! of "one common shape, per-source richness in `fields`".

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// The seven .said pillars (canonical taxonomy per
/// `docs/said-structure/04-four-pillars/README.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pillar {
    Episodic,
    Semantic,
    Procedural,
    External,
    Code,
    Memory,
    /// Vault / document pillar — structured files (PDFs, Office docs, etc.)
    /// ingested via said-vault. Added when sca-core gained Pillar::Document.
    Document,
}

impl Pillar {
    pub const ALL: &'static [Pillar] = &[
        Pillar::Episodic,
        Pillar::Semantic,
        Pillar::Procedural,
        Pillar::External,
        Pillar::Code,
        Pillar::Memory,
        Pillar::Document,
    ];
}

impl fmt::Display for Pillar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Pillar::Episodic => "Episodic",
            Pillar::Semantic => "Semantic",
            Pillar::Procedural => "Procedural",
            Pillar::External => "External",
            Pillar::Code => "Code",
            Pillar::Memory => "Memory",
            Pillar::Document => "Document",
        };
        f.write_str(name)
    }
}

/// Per spec §6.3 and §9.2 item 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoryKind {
    ApiEndpoint,
    Requirement,
    Ticket,
    TableRow,
    Generic,
}

impl fmt::Display for StoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            StoryKind::ApiEndpoint => "ApiEndpoint",
            StoryKind::Requirement => "Requirement",
            StoryKind::Ticket => "Ticket",
            StoryKind::TableRow => "TableRow",
            StoryKind::Generic => "Generic",
        };
        f.write_str(name)
    }
}

impl StoryKind {
    /// Pillars that should normally be populated to ground a story of this kind.
    /// Used by the generator's "absent pillar flagging" (spec §9.2 item 5).
    pub fn expected_pillars(&self) -> &'static [Pillar] {
        match self {
            StoryKind::ApiEndpoint => &[Pillar::Code, Pillar::External],
            StoryKind::Requirement => &[Pillar::External, Pillar::Semantic],
            StoryKind::Ticket => &[Pillar::Code],
            StoryKind::TableRow => &[Pillar::Code],
            StoryKind::Generic => &[],
        }
    }
}

/// The authoritative story record, serialized into
/// `forge:story:<hash>:<slug>` frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Story {
    /// kebab-case unique-within-directive identifier.
    pub slug: String,
    /// Human-readable one-liner (often OpenAPI summary / heading text).
    pub title: String,
    /// Free-text fallback — the raw snippet the adapter extracted.
    pub raw_text: String,
    pub kind: StoryKind,
    /// Adapter-specific structured data (OpenAPI: method/path/params;
    /// Markdown: section_path; Excel: column values).
    #[serde(default)]
    pub fields: BTreeMap<String, serde_json::Value>,
    /// Points back to `forge:directive:<hash>` that produced this story.
    pub directive_hash: String,
    /// Adapter name: "openapi", "markdown", etc.
    pub source_adapter: String,
    /// Anchor into the source — e.g. JSON pointer, heading path, row number.
    pub source_anchor: String,
}

/// Envelope around the raw directive bytes. One is written per
/// `said forge load` as a `forge:directive:<hash>` frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectiveDoc {
    /// Path or URL the directive was loaded from.
    pub source: String,
    /// Adapter name that parsed it.
    pub adapter: String,
    /// Raw bytes (YAML / JSON / XLSX / MD / bytes).
    #[serde(with = "serde_bytes")]
    pub raw: Vec<u8>,
    pub meta: DirectiveMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectiveMeta {
    /// UNIX timestamp, seconds.
    pub loaded_at_utc: i64,
    pub operator: String,
    pub content_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn story_roundtrips_through_json() {
        let s = Story {
            slug: "post-accounts".into(),
            title: "Create account".into(),
            raw_text: "POST /accounts".into(),
            kind: StoryKind::ApiEndpoint,
            fields: {
                let mut m = std::collections::BTreeMap::new();
                m.insert("method".into(), json!("POST"));
                m.insert("path".into(), json!("/accounts"));
                m
            },
            directive_hash: "a3f91".into(),
            source_adapter: "openapi".into(),
            source_anchor: "paths./accounts.post".into(),
        };
        let encoded = serde_json::to_string(&s).unwrap();
        let decoded: Story = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.slug, s.slug);
        assert_eq!(decoded.kind, StoryKind::ApiEndpoint);
        assert_eq!(decoded.fields.get("method"), Some(&json!("POST")));
    }

    #[test]
    fn story_kind_serializes_as_snake_case() {
        assert_eq!(serde_json::to_string(&StoryKind::ApiEndpoint).unwrap(), "\"api_endpoint\"");
        assert_eq!(serde_json::to_string(&StoryKind::Requirement).unwrap(), "\"requirement\"");
        assert_eq!(serde_json::to_string(&StoryKind::Ticket).unwrap(), "\"ticket\"");
        assert_eq!(serde_json::to_string(&StoryKind::TableRow).unwrap(), "\"table_row\"");
        assert_eq!(serde_json::to_string(&StoryKind::Generic).unwrap(), "\"generic\"");
    }

    #[test]
    fn story_kind_display_is_human_readable() {
        assert_eq!(StoryKind::ApiEndpoint.to_string(), "ApiEndpoint");
        assert_eq!(StoryKind::TableRow.to_string(), "TableRow");
    }

    #[test]
    fn directive_doc_holds_raw_bytes_and_meta() {
        let d = DirectiveDoc {
            source: "fixtures/petstore.yaml".into(),
            adapter: "openapi".into(),
            raw: b"openapi: 3.0.0\n".to_vec(),
            meta: DirectiveMeta {
                loaded_at_utc: 1714000000,
                operator: "user".into(),
                content_type: Some("application/yaml".into()),
            },
        };
        let encoded = serde_json::to_string(&d).unwrap();
        let decoded: DirectiveDoc = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.source, d.source);
        assert_eq!(decoded.raw, d.raw);
        assert_eq!(decoded.meta.loaded_at_utc, 1714000000);
    }

    #[test]
    fn story_kind_expected_pillars_matches_spec() {
        let api_expected = StoryKind::ApiEndpoint.expected_pillars();
        assert!(api_expected.contains(&Pillar::Code));
        assert!(api_expected.contains(&Pillar::External));

        let req_expected = StoryKind::Requirement.expected_pillars();
        assert!(req_expected.contains(&Pillar::External));
        assert!(req_expected.contains(&Pillar::Semantic));

        let generic_expected = StoryKind::Generic.expected_pillars();
        assert!(generic_expected.is_empty(), "Generic kind should not assume any pillar");
    }

    #[test]
    fn pillar_all_has_six_entries() {
        assert_eq!(Pillar::ALL.len(), 6);
    }

    #[test]
    fn pillar_display_is_titlecase() {
        assert_eq!(Pillar::Episodic.to_string(), "Episodic");
        assert_eq!(Pillar::External.to_string(), "External");
    }
}

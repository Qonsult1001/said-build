//! Directive source adapters (OpenAPI, Markdown, future: Excel/Word/CSV/Text).
//!
//! Per spec §7, follows the Elastic Connectors pattern: one trait, thin
//! common core, flat source-specific fields, capability advertisement.

use crate::{DirectiveDoc, ForgeError, ForgeResult, Story};

pub mod openapi;
pub mod markdown;
#[cfg(feature = "forge-xlsx")]
pub mod xlsx;
#[cfg(feature = "forge-xlsx")]
pub(crate) mod macro_strip;
#[cfg(feature = "forge-xlsx")]
pub mod xlsx_styles;

/// Capabilities advertised by each adapter so callers can branch on them
/// without a type hierarchy.
#[derive(Debug, Clone, Copy, Default)]
pub struct SourceCapabilities {
    pub supports_incremental_reload: bool,
    pub supports_filter_dsl: bool,
    pub provides_schema_fields: bool,
    pub requires_network: bool,
}

/// Every adapter implements this trait.
#[async_trait::async_trait]
pub trait DirectiveSource: Send + Sync {
    fn name(&self) -> &'static str;
    fn supported_extensions(&self) -> &'static [&'static str];
    fn supported_url_schemes(&self) -> &'static [&'static str];
    fn capabilities(&self) -> SourceCapabilities;

    /// Lightweight content/extension sniff. `false` means "try the next adapter".
    fn detect(&self, path_or_url: &str) -> bool;

    /// Fetch and parse into a raw directive envelope. May do I/O (file) or
    /// network (HTTPS). Operator is the UNIX user or session identifier,
    /// written into `DirectiveMeta`.
    async fn load(&self, path_or_url: &str, operator: &str) -> ForgeResult<DirectiveDoc>;

    /// Pure transformation — takes an already-loaded directive and extracts
    /// the story list. Called synchronously after `load`.
    fn extract_stories(&self, doc: &DirectiveDoc) -> ForgeResult<Vec<Story>>;
}

/// Dispatch registry. Tries each registered adapter in order; first `detect`
/// match wins. Spec §7 promises auto-detection with an optional `--source`
/// override handled at the CLI layer.
pub struct SourceRegistry {
    adapters: Vec<Box<dyn DirectiveSource>>,
}

impl Default for SourceRegistry {
    fn default() -> Self {
        let mut r = Self { adapters: Vec::new() };
        r.register(Box::new(openapi::OpenApiSource));
        r.register(Box::new(markdown::MarkdownSource::default()));
        r
    }
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self { adapters: Vec::new() }
    }

    pub fn register(&mut self, adapter: Box<dyn DirectiveSource>) {
        self.adapters.push(adapter);
    }

    /// Find the adapter for a given path or URL.
    pub fn detect(&self, path_or_url: &str) -> ForgeResult<&dyn DirectiveSource> {
        for a in &self.adapters {
            if a.detect(path_or_url) {
                return Ok(a.as_ref());
            }
        }
        Err(ForgeError::NoAdapter(path_or_url.to_string()))
    }

    /// Force a specific adapter by name (for `--source` override).
    pub fn by_name(&self, name: &str) -> ForgeResult<&dyn DirectiveSource> {
        for a in &self.adapters {
            if a.name() == name {
                return Ok(a.as_ref());
            }
        }
        Err(ForgeError::NoAdapter(format!("--source={}", name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_both_adapters_by_default() {
        let r = SourceRegistry::default();
        assert!(r.by_name("openapi").is_ok());
        assert!(r.by_name("markdown").is_ok());
    }

    #[test]
    fn registry_errors_on_unknown_adapter() {
        let r = SourceRegistry::default();
        // Can't call .unwrap_err() because Ok(&dyn DirectiveSource) isn't Debug.
        match r.by_name("excel") {
            Err(ForgeError::NoAdapter(_)) => {}
            Err(other) => panic!("wrong error variant: {:?}", other),
            Ok(_) => panic!("excel adapter should not exist"),
        }
    }

    #[test]
    fn registry_detects_openapi_yaml() {
        let r = SourceRegistry::default();
        let adapter = r.detect("fixtures/petstore.yaml");
        assert!(adapter.is_ok());
        assert_eq!(adapter.unwrap().name(), "openapi");
    }
}

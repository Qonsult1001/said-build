//! Plugin ecosystem — third-party integrations for `.said` that extend the
//! brain without modifying the core binary.
//!
//! The goal mirrors mem0's openmemory / openclaw / Cognee ecosystem:
//! external packs (Slack, Linear, Obsidian, GitHub, Gmail, …) that ingest
//! domain data and plug into the recall/remember/dream lifecycle. First-
//! party packs (SQL, codebase, PDF ingestion) are lifted to the same trait
//! so the contract is battle-tested by code we already ship.
//!
//! Everything here is a TRAIT + MANIFEST spec; the core binary loads
//! plugins from a registry but the plugin bodies live in separate crates
//! so `.said` keeps its zero-dep portable single-file story.
//!
//! Enterprise gating: each plugin declares whether it embeds content; the
//! loader refuses plugins with `embeds_content=true` on Enterprise brains
//! to preserve the pointer-only discipline.

use crate::said_file::SaidFile;
use crate::frames::Pillar;

/// What a plugin declares about itself at registration time.
///
/// Kept intentionally small — just enough for the loader to make correct
/// enterprise/pillar routing decisions without running the plugin first.
#[derive(Debug, Clone)]
pub struct PluginManifest {
    /// Short machine-readable identifier: `said-sql-pack`, `said-slack-pack`.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Semver version string — registry uses this for upgrade detection.
    pub version: String,
    /// Pillars the plugin writes to. The loader uses this to build the
    /// per-brain whitelist (so a Code-only plugin doesn't unexpectedly
    /// write Episodic frames).
    pub writes_pillars: Vec<Pillar>,
    /// Whether any of the plugin's writes embed content bytes rather than
    /// pointers. Enterprise brains auto-refuse plugins with this = true.
    pub embeds_content: bool,
    /// Freeform short description for `said plugin list`.
    pub description: String,
}

/// Plugin lifecycle — a trait any domain pack implements. All methods have
/// default no-op implementations so plugins only override the hooks they
/// care about.
pub trait SaidPlugin: Send + Sync {
    /// Return the manifest. Called once at registration.
    fn manifest(&self) -> PluginManifest;

    /// Hook called when a caller invokes `remember` that touches this
    /// plugin's domain. The default implementation does nothing; packs like
    /// `said-slack-pack` override it to parse Slack export events into
    /// structured memories.
    fn on_remember(&self, _brain: &mut SaidFile, _doc_id: &str, _content: &str) {}

    /// Hook called after `recall` / `ask` surfaces hits. Plugins can use
    /// this for telemetry or to render domain-specific previews (e.g.
    /// `said-linear-pack` formats ticket IDs as clickable URLs).
    fn on_recall(&self, _brain: &SaidFile, _query: &str, _hit_doc_ids: &[&str]) {}

    /// Hook called at dream cycle boundary. Plugins that build their own
    /// derived state (e.g. a `said-git-pack` that tracks commit graphs)
    /// can consolidate here. Default no-op.
    fn on_dream(&self, _brain: &mut SaidFile) {}
}

/// In-memory plugin registry. Enterprise MCP servers construct one at
/// startup from a TOML config; Portable mode keeps it empty by default.
pub struct PluginRegistry {
    plugins: Vec<Box<dyn SaidPlugin>>,
    /// When true, plugins declaring `embeds_content=true` are refused
    /// at register() time. MCP server sets this based on brain mode at
    /// open().
    enterprise_mode: bool,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self { plugins: Vec::new(), enterprise_mode: false }
    }

    pub fn set_enterprise_mode(&mut self, on: bool) {
        self.enterprise_mode = on;
    }

    /// Try to register a plugin. Returns `Err` on enterprise-mode violation.
    pub fn register(&mut self, plugin: Box<dyn SaidPlugin>) -> Result<(), String> {
        let manifest = plugin.manifest();
        if self.enterprise_mode && manifest.embeds_content {
            return Err(format!(
                "plugin '{}' declares embeds_content=true; refused on Enterprise brain. \
                 Use the pointer variant or switch to Portable.",
                manifest.id,
            ));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn list(&self) -> Vec<PluginManifest> {
        self.plugins.iter().map(|p| p.manifest()).collect()
    }

    /// Broadcast remember to every plugin — order of calls follows
    /// registration order. Plugins are expected to no-op if the doc_id or
    /// content isn't in their domain.
    pub fn broadcast_remember(&self, brain: &mut SaidFile, doc_id: &str, content: &str) {
        for p in &self.plugins {
            p.on_remember(brain, doc_id, content);
        }
    }

    pub fn broadcast_recall(&self, brain: &SaidFile, query: &str, hit_doc_ids: &[&str]) {
        for p in &self.plugins {
            p.on_recall(brain, query, hit_doc_ids);
        }
    }

    pub fn broadcast_dream(&self, brain: &mut SaidFile) {
        for p in &self.plugins {
            p.on_dream(brain);
        }
    }
}

impl Default for PluginRegistry {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingPlugin {
        manifest: PluginManifest,
        remember_calls: Arc<AtomicUsize>,
    }

    impl SaidPlugin for CountingPlugin {
        fn manifest(&self) -> PluginManifest { self.manifest.clone() }
        fn on_remember(&self, _brain: &mut SaidFile, _doc_id: &str, _content: &str) {
            self.remember_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn register_and_broadcast() {
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Box::new(CountingPlugin {
            manifest: PluginManifest {
                id: "test-pack".into(),
                name: "Test".into(),
                version: "0.1.0".into(),
                writes_pillars: vec![Pillar::Episodic],
                embeds_content: false,
                description: "test".into(),
            },
            remember_calls: counter.clone(),
        });
        let mut reg = PluginRegistry::new();
        reg.register(p).expect("register");
        assert_eq!(reg.list().len(), 1);

        let mut brain = SaidFile::create("tmp_plugin_test.said");
        reg.broadcast_remember(&mut brain, "doc1", "hello");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        // Cleanup — the file was only touched in memory since we didn't save.
        let _ = std::fs::remove_file("tmp_plugin_test.said");
    }

    #[test]
    fn enterprise_refuses_content_embedding_plugin() {
        let mut reg = PluginRegistry::new();
        reg.set_enterprise_mode(true);

        struct EmbeddingPack;
        impl SaidPlugin for EmbeddingPack {
            fn manifest(&self) -> PluginManifest {
                PluginManifest {
                    id: "embed-pack".into(),
                    name: "Embed".into(),
                    version: "0.1.0".into(),
                    writes_pillars: vec![Pillar::External],
                    embeds_content: true,
                    description: "".into(),
                }
            }
        }

        let r = reg.register(Box::new(EmbeddingPack));
        assert!(r.is_err(), "enterprise should refuse content-embedding plugin");

        struct PointerPack;
        impl SaidPlugin for PointerPack {
            fn manifest(&self) -> PluginManifest {
                PluginManifest {
                    id: "pointer-pack".into(),
                    name: "Pointer".into(),
                    version: "0.1.0".into(),
                    writes_pillars: vec![Pillar::External],
                    embeds_content: false,
                    description: "".into(),
                }
            }
        }
        assert!(reg.register(Box::new(PointerPack)).is_ok());
    }
}

# Row 50 — Plugin ecosystem trait

**Status:** ✅ trait + registry shipped 2026-04-22. Discovery loader + first-party lift deferred — see [Row 40 spec](row-40-plugin-spec.md).

## What it does

`SaidPlugin` trait + `PluginManifest` + `PluginRegistry` form the contract for third-party ingestion packs. Plugins declare which pillars they write to, whether they embed content (Enterprise-mode relevant), and hook into the `remember` / `recall` / `dream` lifecycle.

## Where it lives

[`crates/sca-core/src/plugin.rs`](../../../crates/sca-core/src/plugin.rs)

## Inputs

### Manifest
```rust
pub struct PluginManifest {
    pub id: String,                 // "said-slack-pack"
    pub name: String,               // "Slack"
    pub version: String,            // "0.3.1" (semver)
    pub writes_pillars: Vec<Pillar>,// pillars the plugin writes to
    pub embeds_content: bool,       // Enterprise refuses this = true
    pub description: String,        // for `said plugin list`
}
```

### Trait
```rust
pub trait SaidPlugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    fn on_remember(&self, _brain: &mut SaidFile, _doc_id: &str, _content: &str) {}
    fn on_recall(&self, _brain: &SaidFile, _query: &str, _hit_doc_ids: &[&str]) {}
    fn on_dream(&self, _brain: &mut SaidFile) {}
}
```

All hooks have default empty implementations; plugins override only what they care about.

### Registry
```rust
pub struct PluginRegistry { /* ... */ }

impl PluginRegistry {
    pub fn new() -> Self;
    pub fn set_enterprise_mode(&mut self, on: bool);
    pub fn register(&mut self, plugin: Box<dyn SaidPlugin>) -> Result<(), String>;
    pub fn list(&self) -> Vec<PluginManifest>;

    pub fn broadcast_remember(&self, brain: &mut SaidFile, doc_id: &str, content: &str);
    pub fn broadcast_recall(&self, brain: &SaidFile, query: &str, hits: &[&str]);
    pub fn broadcast_dream(&self, brain: &mut SaidFile);
}
```

## Outputs

- `register()` returns `Err` when enterprise mode is enabled AND `manifest.embeds_content == true`. Otherwise pushes the plugin onto the internal list.
- `list()` returns every registered manifest.
- `broadcast_*` fan events across all plugins in registration order.

## How to test

2 unit tests in [`plugin.rs`](../../../crates/sca-core/src/plugin.rs):

1. `register_and_broadcast` — instantiate a counting plugin, register, broadcast `on_remember`, assert the counter ticks
2. `enterprise_refuses_content_embedding_plugin` — set registry strict, try to register `embeds_content=true` plugin, assert `Err`; register `embeds_content=false` → `Ok`

All green as of 2026-04-22.

## How to extend

### New plugin
1. Create a struct
2. Implement `SaidPlugin`:
   ```rust
   impl SaidPlugin for MySlackPack {
       fn manifest(&self) -> PluginManifest { /* ... */ }
       fn on_remember(&self, brain: &mut SaidFile, doc_id: &str, content: &str) {
           // parse Slack event, optionally write a structured companion frame
       }
   }
   ```
3. Register via `registry.register(Box::new(MySlackPack))`

Until the CLI loader ships, this means compiling your plugin into a binary that includes `sca-core` and registers on startup.

### First-party lift
Pick an in-tree pack (SQL, codebase, PDF). Rewrite its entry point as `impl SaidPlugin` and have `said-cli` / `said-mcp` register it. Benefits: contract gets tested on every build; third parties have a real example to copy.

## Known limitations

### No discovery / loader
Plugins have to be compiled into the calling binary. An `.said-plugins/` directory with a `plugins.toml` manifest + `libloading` for `cdylib` crates would make installs a single command. Not shipped.

### No `said plugin install <name>` CLI
Same root cause as the loader gap.

### First-party packs not yet lifted
`said-sql-pack`, `said-codebase-pack`, `said-pdf-pack` still live in-tree with direct `put_with` calls. Lifting is a ~1-week task.

### No hook for `on_ingest`
Current hooks are `remember / recall / dream`. Document-ingest pipelines could benefit from an `on_ingest_file(path, mime)` hook for mid-pipeline inspection. Add when the first plugin asks for it.

## See also

- [Row 40 Plugin ecosystem (spec)](row-40-plugin-spec.md) — the planned discovery + first-party lift
- [6 Ingestion plugins](../06-ingestion-plugins/README.md)
- [Row 48 Migration adapters](row-48-migration.md) — similar trait-based pattern

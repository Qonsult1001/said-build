# Row 40 — Plugin ecosystem (spec)

**Status:** ⏳ trait shipped; discovery/loader + first-party lift deferred. See [Row 50](row-50-plugin-trait.md) for the shipped trait.

## What the roadmap entry says

> `SaidPlugin` trait + manifest + per-brain whitelist so third-party ingestion packs (Slack, Linear, Obsidian, GitHub, Gmail, etc.) can be installed via `said plugin install <name>` without touching the core binary. First-party packs (SQL, codebase, PDF) lifted to the same trait so the contract is battle-tested. Enterprise-mode gated: plugins that would embed content are auto-refused on Enterprise brains.

## Shipped: the trait + manifest + registry

See [Row 50](row-50-plugin-trait.md). You can already register a plugin from code:

```rust
let mut reg = PluginRegistry::new();
reg.set_enterprise_mode(brain.mode() == BrainMode::Enterprise);
reg.register(Box::new(MySlackPack))?;

reg.broadcast_remember(&mut brain, "doc1", "content");
```

What the registry does at register time:

1. Read the plugin's manifest
2. Check `manifest.embeds_content` against enterprise mode; refuse if conflicting
3. Add to internal `Vec<Box<dyn SaidPlugin>>`
4. Fire `on_remember` / `on_recall` / `on_dream` hooks as the brain operates

## Gaps

### `said plugin install <name>` CLI command
Not shipped. Today every plugin has to be compiled into a binary that explicitly calls `PluginRegistry::register`. An `.said-plugins/` directory with a `plugins.toml` manifest would let users install without rebuilding.

### First-party lift
Current in-tree packs don't go through the registry:
- `said-sql-pack` — SQL schema ingestion
- `said-codebase-pack` — AST + tree-sitter
- `said-pdf-pack` — PDF/DOCX/TXT/MD via pdfium + pdf-extract + quick-xml

Lifting each means rewriting its entry point as an `impl SaidPlugin` and having `said-cli` / `said-mcp` register them on startup. Main benefit: the plugin contract gets tested by code we ship on every build.

### Registry loader
Currently `PluginRegistry::register` is called from Rust. A TOML-based loader would:

```toml
# .said-plugins.toml
[plugins]
slack = { version = "0.3", repo = "github.com/acme/said-slack-pack" }
linear = { version = "0.1", path = "./vendor/said-linear-pack" }
```

Each plugin ships a `Cargo.toml` with `crate-type = ["cdylib"]` and exports a `register_plugin(registry: &mut PluginRegistry)` function via `extern "C"`. Loader uses `libloading` to dlopen and call it.

## Planned targets (third-party packs)

- **Slack** — export channels + DMs → Episodic frames
- **Linear** — tickets → External pointers
- **Obsidian** — vault markdown → Semantic frames
- **GitHub** — issues + PRs → External pointers (metadata) + Code frames (referenced files)
- **Gmail** — threads → Episodic frames with email-specific tags
- **Notion** — pages → External pointers in enterprise mode, Semantic in portable
- **Jira** — tickets → External pointers
- **Drive** — docs → External pointers

Each is ~1-2 days of work against the shipped trait once the loader exists.

## See also

- [Row 50 Plugin ecosystem trait](row-50-plugin-trait.md)
- [Row 48 Migration adapters](row-48-migration.md) — same pattern, different target
- [6 Ingestion plugins](../06-ingestion-plugins/README.md)
- [Roadmap](../12-roadmap.md)

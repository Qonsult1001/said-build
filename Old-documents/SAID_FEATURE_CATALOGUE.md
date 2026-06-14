# SAID-ECHO Feature Catalogue

Living reference for every shipped feature. Each section answers:

1. **What it does** — one-paragraph plain-English summary
2. **Where it lives** — files, modules, public APIs
3. **Inputs** — required arguments, formats, preconditions
4. **Outputs** — return types, frame tags, persisted state, stdout/JSON shape
5. **How to test** — fixtures, commands, expected behaviour
6. **How to extend** — what a contributor changes to add new behaviour

The table-row summaries in [`SAID_MVP_PLAN.md`](SAID_MVP_PLAN.md) say **whether** a feature exists. This catalogue says **how** it behaves, so future tests, refactors, and plugin integrations start from the same baseline.

**Versioning rule:** when a row changes behaviour materially, update both docs in the same commit. MVP plan gets the new status; this catalogue gets the new behaviour contract.

---

## Table of contents (by MVP plan row)

- [Row 30 — Pillar enum on FrameMeta](#row-30--pillar-enum-on-framemeta)
- [Row 31 — Per-pillar retrieval scope](#row-31--per-pillar-retrieval-scope)
- [Row 32 — Explicit episodic writer + tool hooks](#row-32--explicit-episodic-writer--tool-hooks)
- [Row 33 — Salience scorer v0](#row-33--salience-scorer-v0)
- [Row 34 — Dream pipeline v1 (plumbing)](#row-34--dream-pipeline-v1-plumbing)
- [Row 35 — Brain-state auto-dream (Decision 5 v2)](#row-35--brain-state-auto-dream-decision-5-v2)
- [Row 36 — External pillar: Enterprise pointer mode](#row-36--external-pillar-enterprise-pointer-mode)
- [Row 37 — Immutable brain deployment mode](#row-37--immutable-brain-deployment-mode)
- [Row 41 — Surprise / reconsolidation detector](#row-41--surprise--reconsolidation-detector)
- [Row 42 — Admin CLI (recycle bin + compliance)](#row-42--admin-cli-recycle-bin--compliance)
- [Row 44 — MCP admin tool](#row-44--mcp-admin-tool)
- [Row 45 — Audit section (AUDT) + AppGrant](#row-45--audit-section-audt--appgrant)
- [Row 46 — Procedural pillar writer](#row-46--procedural-pillar-writer)
- [Row 47 — Code pillar writer](#row-47--code-pillar-writer)
- [Row 48 — Migration adapters (mem0, memvid)](#row-48--migration-adapters-mem0-memvid)
- [Row 49 — Competitor benchmark harness](#row-49--competitor-benchmark-harness)
- [Row 50 — Plugin ecosystem trait](#row-50--plugin-ecosystem-trait)
- [Planned but not yet implemented](#planned-but-not-yet-implemented)

---

## Row 30 — Pillar enum on FrameMeta

**What it does.** Every frame stored in a `.said` file carries a `Pillar` label — Episodic, Semantic, Procedural, External, Code, or Memory. The label is the primary routing discriminator for Decision 2's retrieval scope filter and for dream / admin tooling.

**Where it lives.**
- `crates/sca-core/src/frames.rs` — `enum Pillar`, field `FrameMeta.pillar`, `Pillar::from_memory_type`
- On-disk format — pillar is one byte per frame in the TOC section

**Inputs.** A `Pillar` value when writing a frame (set by `remember_with_pillar` and `remember_as_*` helpers). Frames loaded from pre-Decision-1 `.said` files default to `Pillar::from_memory_type(Factual) → Semantic` for backwards compatibility.

**Outputs.** `FrameMeta.pillar` returned by all frame-iteration APIs (`get_all_frames`, `active_doc_ids`, lineage walks, admin helpers). `pillar:<name>` also written as a tag so tag-scoped retrieval and filter paths that predate the enum still work.

**How to test.** Create a brain, call `remember_with_pillar(..., Pillar::Procedural, ...)`, read back via `brain.frames.get_meta(doc_id).unwrap().pillar`, assert it equals `Pillar::Procedural`. See `examples/pillar_writers_probe.rs`.

**How to extend.** Add a new pillar variant: add to `enum Pillar`, update `from_memory_type`, update `from_byte`, bump the serializer's fallback comment. Then extend `remember_with_pillar`'s `memory_type` mapping and any pillar-scoped retrieval code.

---

## Row 31 — Per-pillar retrieval scope

**What it does.** The `search` / `ask` path can narrow results to a subset of pillars. A caller searching for "the API spec" can pass `pillar=semantic,external` to exclude episodic chat turns and procedural runbooks.

**Where it lives.**
- `crates/sca-core/src/recall.rs` — `search_full_scoped_pillars`
- `crates/sca-core/src/said_file.rs` — `SaidFile::recall_by_pillar`
- `crates/said-mcp/src/tools.rs` — `SearchTool.pillar` field
- `crates/said-mcp/src/handler.rs` — `handle_search` parses the comma-separated list

**Inputs.** `Option<&HashSet<Pillar>>` — None means all pillars; Some narrows. MCP tool accepts comma-separated names (`episodic`, `semantic`, `procedural`, `external`, `code`, `memory`); unknown names are silently ignored.

**Outputs.** `Vec<RecallResult>` filtered by the pillar set; otherwise identical to the normal recall pipeline.

**How to test.** Regression check: with pillar filter `None`, LoCoMo R@10 and MTEB must match pre-Decision-2 numbers. The harness at `crates/sca-core/examples/locomo_baseline.rs` already covers this.

**How to extend.** Adding new filters (e.g. `tag=` or `created_after=`) should follow the same pattern: compose a pre-filter HashSet at the top of `search_full_scoped_pillars` and let the existing scoring pipeline treat the narrowed set as the whole corpus.

---

## Row 32 — Explicit episodic writer + tool hooks

**What it does.** Three entry points populate Episodic frames: an explicit `/remember` tool, an automatic `session_end` flush, and a per-tool-call `tool_completion` hook. Agents using `.said` via MCP get all three; the CLI provides the explicit path.

**Where it lives.**
- `crates/sca-core/src/said_file.rs` — `SaidFile::remember_with_pillar`
- `crates/said-mcp/src/tools.rs` — `RememberTool`, `SessionEndTool`, `ToolCompletionTool`
- `crates/said-mcp/src/handler.rs` — `handle_remember`, `handle_session_end`, `handle_tool_completion`

**Inputs.** Content string + optional doc_id + optional title + pillar + extra tags. `session_end` takes a freeform session summary; `tool_completion` takes `{tool_name, args, result}`.

**Outputs.** A new frame id. Tags on the resulting frame include `pillar:<name>`, whatever the caller passed, plus `session_end:true` or `tool_completion:true` for the auto-writers. Returns to the MCP client a one-line summary including the new frame count.

**How to test.** Call each tool via stdio MCP (see `crates/said-mcp/src/main.rs`), then assert via `get` that a frame with the expected doc_id exists. Integration test lives inside the `remember_with_salience` probe (`examples/surprise_probe.rs`).

**How to extend.** To add a new auto-writer (e.g. `/commit-end` hook), register a new `*Tool` struct, wire it into `SaidTools`, route to a thin handler that calls `remember_with_pillar(Pillar::Episodic, …)` with a conventionally-tagged body.

---

## Row 33 — Salience scorer v0

**What it does.** Every call to `remember_with_salience` scores the content's importance on a 0..=100 scale using eight deterministic signals (explicit markers, correction markers, decision markers, assertion shape, length band, chit-chat penalty, question penalty, pillar bias). Band `Low` / `Medium` / `High` comes from the score. A session-level `SalienceAccumulator` crosses threshold 150 to fire a dream.

**Where it lives.**
- `crates/sca-core/src/salience.rs` — `score_turn`, `Salience`, `SalienceBand`, `SalienceAccumulator`
- `crates/sca-core/src/said_file.rs` — `remember_with_salience`
- MCP tool `salience` exposes standalone scoring

**Inputs.** Lowercased text plus a `Pillar` bias. Stateless — same input = same output, no global state read.

**Outputs.**
- `Salience { score: u32, band: SalienceBand, tags: Vec<String> }`
- Tags include `salience:<band>` always; plus `reconsolidation` / `decision` / `explicit` when those markers fire.

**How to test.** Unit tests sit in the module. For new markers, add a fixture to `score_turn` testing the marker yields the expected band without regressing the Low default for chit-chat. Probe example: `examples/surprise_probe.rs` logs `[salience]` per frame.

**How to extend.** To swap in a trained model (v1 in the plan): keep `score_turn`'s shape, replace the internals with a Model2Vec + linear head inference call, ensure the score range stays 0..=100 and band boundaries remain at 30 / 60.

---

## Row 34 — Dream pipeline v1 (plumbing)

**What it does.** Historical — Decision 5 v1 shipped a content-consolidation pipeline (episodic → semantic via fingerprint clustering). Measurement showed ~2-point LoCoMo R@10 regression, so v2 (below) disabled the content path.

**Status.** Deprecated in-place. The `DreamCycle` / `DreamParams` / `DreamReport` types remain for API compatibility; `SaidFile::run_dream_content` is a no-op stub.

**How to test.** `run_dream_content(cycle, &params)` should always return a zero-count report; MTEB and LoCoMo baselines must be unaffected.

**How to extend.** Content consolidation moves to the caller's LLM under BYO-LLM. If someone wants a new content-side dream, they should build it in the harness layer (scripts/…) and call `remember_with_pillar(Pillar::Semantic)` from there.

---

## Row 35 — Brain-state auto-dream (Decision 5 v2)

**What it does.** Three automatic brain-state operations run on every `ask` / `search` call with zero user action:
1. **S_slow tensor** accumulates every remember's embedding (64×64 outer product with decay 0.999) — supports cross-doc synthesis scoring at query time.
2. **Recall-weight reconsolidation** — each doc's `recall_weight` bumps on access, decays on idle. Warm docs rank higher next time.
3. **Fingerprint-threshold drift** — query embeddings accumulate; when the count crosses `dynamic_dream_threshold(active_frames)` the brain drifts its corpus mean/std toward the query distribution.

Threshold scales with corpus size: 50 queries on a small brain (fast adaptation), 500 on an enterprise-scale brain (stable).

**Where it lives.**
- `crates/sca-core/src/brain.rs` — `s_slow_write`, `s_slow_read`, `consolidate`, `dream`
- `crates/sca-core/src/ask.rs` — `dynamic_dream_threshold`
- CLI `said ask` — auto-fires in `cmd_ask`
- MCP `ask` + `search` — auto-fires in each handler

**Inputs.** None from the caller. Driven internally by write activity + query count.

**Outputs.** Persisted in BRAN section; visible via `said stats` (`s_slow magnitude`, `Dream cycles`, `Pending dream`, auto-threshold value).

**How to test.** Open a brain, run N queries where N > threshold, confirm `brain_cycles` incremented and `query_emb_count` reset. `examples/realworld_recall_probe.rs` exercises this organically.

**How to extend.** To change the threshold curve, edit `dynamic_dream_threshold` in `sca_core::ask`. Any new brain-state component (e.g. an attention-head on top of S_slow) lives inside `Brain` and gets called from the same auto-fire sites.

---

## Row 36 — External pillar: Enterprise pointer mode

**What it does.** Registers a searchable pointer frame (URI + mime + title + summary) without embedding content. Agents and users looking for "the quarterly report PDF" can find it by topic; fetching the actual bytes is the caller's responsibility.

**Where it lives.**
- `crates/sca-core/src/said_file.rs` — `remember_as_external_pointer`
- CLI — `said ingest <path> --pointer --summary "…"`
- MCP — `ingest` tool with `pointer=true` + `summary`

**Inputs.** `uri: &str`, `mime: Option<&str>`, `title: Option<&str>`, `summary: &str`, extra tags.

**Outputs.** One new frame with body `"<uri>\nmime: <m>\ntitle: <t>\nsummary: <s>"`, tagged `pillar:external`, `external:pointer`, `mime:<type>`. Frame's uncompressed length is just the summary size — no blob stored.

**How to test.** Create a brain, call `remember_as_external_pointer("file:///test.pdf", Some("pdf"), Some("Q3 Report"), "Summary of Q3 numbers", vec![])`, search `"Q3 numbers"`, verify the frame ranks within top-10. Verify `uncompressed_len` matches summary length (no blob). Verified end-to-end in production `willie.said` via `said ingest --pointer`.

**How to extend.** To add a portable-mode counterpart (`remember_as_external_embedded` storing bytes in an XBLB section), implement the blob-store section format and mirror the pointer writer's API with an extra `content: &[u8]` parameter.

---

## Row 37 — Immutable brain deployment mode

**What it does.** At `said create` the brain is tagged **Portable** (embeds content, offline-friendly) or **Enterprise** (pointer-only, refuses content embeds). The mode is IMMUTABLE — licensing keeps Personal and Enterprise separate, so swapping is blocked by design. An operator who needs the other model creates a new brain and re-ingests.

**Where it lives.**
- `crates/sca-core/src/said_file.rs` — `BrainMode`, `SaidFile::create_with_mode`, `SaidFile::mode`, `SaidFile::ensure_content_ingest_allowed`
- On-disk — `MODE` section (8 bytes), absent = Portable (back-compat)
- CLI — `said create <file> --mode {portable|enterprise}`
- MCP — `create` tool with `mode=enterprise`

**Inputs.** The `--mode` flag (or MCP parameter) at create time. After create, mode is read-only.

**Outputs.** `brain.mode()` returns the current mode. `said stats` shows `Brain mode: portable | enterprise`. Any content-embedding write path (`cmd_ingest` without `--pointer`, `cmd_init`, `cmd_add_dir`, MCP `ingest` without `pointer=true`) calls `ensure_content_ingest_allowed()` and errors out on Enterprise.

**How to test.** Create an Enterprise brain, try `said ingest file.pdf` (no `--pointer`) — must error. Try `said ingest file.pdf --pointer` — must succeed. Confirm the error message points to `--pointer`, not to a mode-switch command (which doesn't exist).

**How to extend.** Any new ingest surface (plugin, migration adapter, scripted bulk import) must call `ensure_content_ingest_allowed()` before writing content-bearing frames. The audit / migration paths already do this.

---

## Row 41 — Surprise / reconsolidation detector

**What it does.** When a new frame would contradict or update an existing frame about the same topic, the writer tags it so downstream tools (dream, admin UI, agent feedback) can surface the conflict. Two detection paths:

1. **Lexical** — `score_turn` sees `actually`, `no wrong`, `i meant` etc. and tags `reconsolidation`.
2. **Semantic** — `find_prior_match` looks up the Hamming-nearest existing frame and classifies via `classify_surprise(PriorMatch { similarity, token_overlap })`:
   - similarity < 0.25 → Benign
   - overlap ≥ 0.90 → TopicalUpdate (restatement / expansion)
   - 0.40 ≤ overlap < 0.90 → Contradiction (same topic, key tokens diverge)
   - overlap < 0.40 → Benign (superficial topic match)

**Where it lives.**
- `crates/sca-core/src/salience.rs` — `Surprise`, `PriorMatch`, `classify_surprise`
- `crates/sca-core/src/said_file.rs` — `find_prior_match`, hook in `remember_with_salience`
- MCP — `remember` surfaces contradictions in the response line

**Inputs.** New content (any frame write path that goes through `remember_with_salience`).

**Outputs.** Tags appended to the new frame:
- `reconsolidation` + `reconsolidation:contradicts` + `contradicts:<prior_doc_id>` on silent overrides
- `reconsolidation` + `reconsolidation:update` + `updates:<prior_doc_id>` on expansions
- None on Benign

**How to test.** `examples/surprise_probe.rs`. Expected behavior verified on a 5-frame fixture covering empty brain, silent contradiction, unrelated topic, explicit marker, topical expansion.

**How to extend.** Thresholds live inside `classify_surprise`. Tuning them changes precision/recall; add new fixtures to `surprise_probe.rs` when tuning so regressions show up. A swap to an embedded ML classifier (v1) only has to preserve `Surprise`'s enum and the tag shape.

---

## Row 42 — Admin CLI (recycle bin + compliance)

**What it does.** `said admin <action>` exposes the tombstone lineage as a real admin surface:

- `list-tombstones [--like <substr>]` — every non-active frame newest-first
- `restore <doc_id>` — flip newest tombstone back to Active, demote previous head
- `who-deleted <doc_id>` — full lineage trail with `superseded_by` + attribution tags
- `legal-hold-add <doc_id> <case>` — tag `legal_hold:<case>` blocks retention sweeps
- `legal-hold-release <doc_id> <case>` — strip tag
- `retention-sweep [--older-than-days N] [--keep-per-doc N]` — reap aged tombstones, honor legal holds
- `audit [--verify] [--actor <a>] [--kind <k>]` — show or verify the AUDT chain

**Where it lives.**
- `crates/sca-core/src/frames.rs` — `admin_tombstone_records`, `admin_restore_tombstoned`, `admin_add_legal_hold`, `admin_release_legal_hold`, `mark_frame_deleted`, `drop_tombstones` (honors `legal_hold:*`)
- `crates/sca-core/src/said_file.rs` — thin wrappers + audit hooks
- `crates/said-cli/src/main.rs` — `AdminAction` enum + `cmd_admin`

**Inputs.** Subcommand + flags (see list above). Brain mode determines whether destructive operations need confirmation (currently they don't; a `--yes` guard could be added).

**Outputs.** Text (default) or JSON (`--json` at cli root) printing the result plus a one-line summary. Every mutating admin action writes an audit entry and persists via `brain.save()`.

**How to test.** Create a brain with a multi-version doc_id, tombstone by re-writing, then walk the five admin actions. Assert legal-hold tag is honored across `retention-sweep` — frame must survive the sweep. See interactive smoke test in commit history for 2026-04-22.

**How to extend.** New admin action = new `AdminAction` variant + new `cmd_admin` match arm + optional MCP mirror in `handle_admin`. Always audit-log mutating actions via `brain.audit_mut()`. If the action touches tombstones it MUST respect `is_under_legal_hold`.

---

## Row 44 — MCP admin tool

**What it does.** Mirror of the CLI admin surface for MCP agents. Single tool with an `action` discriminator so agents only learn one tool name; all actions from row 42 are available via JSON fields.

**Where it lives.**
- `crates/said-mcp/src/tools.rs` — `AdminTool` struct
- `crates/said-mcp/src/handler.rs` — `handle_admin` matches on `t.action`

**Inputs.** `{ action: String, doc_id?: String, like?: String, case?: String, older_than_days?: u64, keep_per_doc?: u32 }`. For the `audit` action, `doc_id` is repurposed as the actor filter and `case` as the kind filter (documented in the tool description).

**Outputs.** Text content describing the action's result. For `list-tombstones`, one line per frame. For `audit`, up to 200 entries + overflow count. Errors return `CallToolError` with a specific reason (missing doc_id, unknown action, etc.).

**How to test.** `printf '{…initialize…}\n{…notifications/initialized…}\n{…admin action=list-tombstones…}' | ./said-mcp.exe`. Expected: JSON response with a `result.content[].text` containing the same output you'd see from `said admin list-tombstones`.

**How to extend.** Any new admin action added to the CLI should get mirrored here: add the action name to the match in `handle_admin`, reuse `AdminTool`'s existing fields or add new ones. Update the tool description string — MCP clients read it to build argument forms.

---

## Row 45 — Audit section (AUDT) + AppGrant

**What it does.** Append-only log of every mutating operation. Each entry carries `{seq, timestamp, actor, kind, target, detail, hash}` and the `hash` is BLAKE3 over the previous hash + all other fields. Tampering with any field in any past entry invalidates the chain. AppGrant layer restricts which apps can invoke which action kinds (enterprise mode uses `strict=true`).

**Where it lives.**
- `crates/sca-core/src/audit.rs` — `AuditLog`, `AuditEntry`, `AppGrant`, `AppGrantRegistry`
- `crates/sca-core/src/said_file.rs` — hooks in `remember_with_pillar`, `forget`, `admin_*`
- On-disk — `AUDT` section, absent = fresh empty log for back-compat

**Inputs.**
- Write side — implicit; hooks fire on every mutating path
- Read side — `brain.audit()` returns the log; CLI `said admin audit`; MCP `admin action=audit`
- Actor override — `brain.audit_mut().set_actor("app:slack-pack")` before a mutating call

**Outputs.**
- `AuditLog::verify() → Result<(), String>` — Ok on valid chain, Err with first break point
- `AuditLog::entries() → &[AuditEntry]` — full list in write order
- CLI / MCP print one line per entry

**How to test.**
- Unit: `cargo test -p sca-core audit::`
- Functional: create brain, write a few frames, flip an entry's `kind` manually, run `said admin audit --verify` — must error with "chain broken at seq=X"
- Roundtrip: save + reopen must preserve the chain exactly

**How to extend.** New audit kinds = add a `brain.audit.append(kind, target, detail)` call at the new mutating site. Kinds are freeform strings; document new ones here. For cross-brain audit export, use `AuditLog::serialize()` / `deserialize()` directly.

---

## Row 46 — Procedural pillar writer

**What it does.** Records an action recipe (Voyager-style skill) with trigger, ordered steps, and outcome. Dream layer can rank Procedural frames by task match × success rate when the retrieval plan ships.

**Where it lives.**
- `crates/sca-core/src/said_file.rs` — `remember_as_procedural`
- `crates/sca-core/src/frames.rs` — `FrameStore::set_pillar` guarantees persisted pillar

**Inputs.**
- `doc_id: Option<&str>`
- `trigger: &str` — natural-language condition ("when the build fails with X")
- `steps: &[&str]` — ordered recipe
- `outcome: Option<&str>` — first word used as status tag (`success`, `failure`, `partial`)
- `extra_tags: Vec<String>`

**Outputs.** New frame with body:
```
TRIGGER: <trigger>
STEPS:
  1. <step>
  2. <step>
OUTCOME: <outcome>
```
Tags include `pillar:procedural`, `procedural:outcome=<first-word>`, plus caller tags. `FrameMeta.pillar == Pillar::Procedural`.

**How to test.** `examples/pillar_writers_probe.rs` covers the full shape + tag + pillar-persistence path.

**How to extend.** New ranking for Procedural retrieval would live in `recall::rerank_by_pillar`; the writer is already pillar-correct. Adding new outcome statuses is a tag change — no code.

---

## Row 47 — Code pillar writer

**What it does.** Records a source-code chunk with language, optional symbol name, and source path. Plays nicely with the existing `said sym` + `record_symbol` + tree-sitter AST pipeline.

**Where it lives.**
- `crates/sca-core/src/said_file.rs` — `remember_as_code`
- Integrates with `SaidFile::record_symbol` and trigram / symbol index

**Inputs.**
- `doc_id: Option<&str>`
- `language: &str` — `rust`, `python`, `typescript`, …
- `source: &str` — the code
- `symbol: Option<&str>` — function/class name
- `source_path: Option<&str>` — original file
- `extra_tags: Vec<String>`

**Outputs.** Body = `"[<language>] <symbol> (<path>)\n<source>"`. Tags `pillar:code`, `lang:<language>`, `symbol:<name>` when present, `source:<path>` when present. Title defaults to symbol or source_path.

**How to test.** `examples/pillar_writers_probe.rs`. Also grep: `said ask "parse_header"` should rank a Code frame with matching symbol tag first.

**How to extend.** To add language-aware search boosts (e.g. Python queries should match `lang:python` tag more strongly), add the boost in `recall_fused` keyed off the `lang:` prefix — no writer change needed.

---

## Row 48 — Migration adapters (mem0, memvid)

**What it does.** One-line import from a competitor's export file. Current adapters:

- **memvid** (JSON array) — `{id, content, timestamp, metadata}` rows → Episodic frames
- **mem0** (JSONL) — `{id, memory, user_id, created_at, categories, metadata}` rows → pillar routed by `categories`

**Where it lives.**
- `crates/sca-core/src/migrate.rs` — `MigrationAdapter` trait, `MigratedRecord`, `run_migration`, `MemvidAdapter`, `Mem0Adapter`, `adapter_for`, `registered_adapters`
- CLI — `said import --from <name> --source <path>` / `--list`

**Inputs.** The competitor's export file path + the adapter name. Enterprise brains refuse content-bearing records that aren't External-pillar pointers — caller is told to retry on a Portable brain.

**Outputs.** `MigrationReport { source_system, records_read, records_written, records_skipped, per_pillar: HashMap, errors: Vec<String> }`. All written frames carry `imported_from:<system>` plus adapter-specific attribution (`user_id:<id>`, `ingested_at:<unix>`, `category:<name>`).

**How to test.**
- Unit: `cargo test -p sca-core migrate::` (3 tests)
- Functional: `examples/pillar_writers_probe.rs` covers memvid + mem0 JSONL end-to-end, asserts pillar mapping
- Manual: write a tiny fixture file, run `said import --from memvid --source fixture.json --path brain.said`, verify with `said admin list-tombstones` (nothing tombstoned) + `said search imported_from:memvid`

**How to extend.** Add an adapter = implement `MigrationAdapter`, register in `adapter_for`, add the name to `registered_adapters`. Keep the competitor's own id stable (prefix with system name) so provenance queries work.

---

## Row 49 — Competitor benchmark harness

**What it does.** Writes `docs/competitor_benchmark.json` with `.said`'s live MTEB + LoCoMo numbers plus placeholder rows for every significant competitor (mem0 OSS + graph, Zep, LangMem, Letta, memvid, pgvector, ChromaDB, HippoRAG, LightRAG, Cognee). A human-readable matrix is printed; the JSON is the machine-readable artefact for CI.

**Where it lives.** `crates/sca-core/examples/competitor_bench.rs`

**Inputs.**
- `--out <path>` (default `docs/competitor_benchmark.json`)
- `--mteb-json <path>` (optional — reads per-task JSON lines if an external harness ran `mteb_rust --json`)
- `--locomo-f1 <f>` (optional override)

**Outputs.** `SystemRow { name, implementation, offline, ingest_calls_llm, locomo_f1, mteb_*, notes }` for every system. Matrix JSON written to disk + printed to stdout.

**How to test.** `cargo run --release -p sca-core --example competitor_bench --features "static-embed"`. Expected: `SAID-ECHO` row shows `LoCoMo F1 0.856`, `Needle 1.00`, `WikimQA 1.00`; competitor rows are `—` except where published numbers exist (mem0 0.669 / 0.684).

**How to extend.** Add a real competitor row = fill in an existing placeholder's scores after running that stack against LoCoMo and MTEB on the same datasets. Add a new competitor = append to `placeholder_rows()` with the same shape. Keep `notes` honest — "where they beat us / where we win".

---

## Row 50 — Plugin ecosystem trait

**What it does.** Third-party integrations (Slack, Linear, Obsidian, GitHub, Gmail) install into `.said` via a declarative trait + manifest without touching the core binary. First-party ingestion packs (SQL, codebase, PDF) remain in-tree but can be lifted to the same trait over time.

**Where it lives.** `crates/sca-core/src/plugin.rs`
- `PluginManifest { id, name, version, writes_pillars, embeds_content, description }`
- `SaidPlugin` trait — `manifest()`, `on_remember`, `on_recall`, `on_dream`
- `PluginRegistry { plugins, enterprise_mode }` — `register`, `list`, `broadcast_*`

**Inputs.** A `Box<dyn SaidPlugin>` passed to `PluginRegistry::register`. Manifest declares everything the loader needs to decide enterprise compatibility.

**Outputs.**
- `register` returns `Err` on enterprise-mode violation (content-embedding plugin attempted on Enterprise brain)
- `broadcast_remember` / `_recall` / `_dream` fan events across all plugins in registration order
- `list()` returns every registered manifest for `said plugin list`-style UIs

**How to test.**
- Unit: `cargo test -p sca-core plugin::` (2 tests)
- Functional: instantiate a counting plugin, broadcast events, assert the counter ticks
- Enterprise guard: set registry strict, try to register `embeds_content=true` plugin, must error

**How to extend.** New plugin = new struct implementing `SaidPlugin`. For in-tree packs, lift existing ingestion code into an `impl SaidPlugin`. Plugin discovery (loading from `.said-plugins/` or a `plugins.toml`) is the next step — design in the UI sprint (row 43).

---

## Known limitations and stubs (shipped-but-imperfect)

Every feature below is functional for its common path but has a narrowly-scoped gap that a future iteration should close. Documenting here so nothing is silently deferred.

### Pillar persistence on direct `put_with` callers (rows 30, 46, 47)

**Gap.** `FrameStore::put_with` derives the stored `FrameMeta.pillar` via `Pillar::from_memory_type(memory_type)`. That mapping sends `MemoryType::Factual → Pillar::Semantic`, so a direct caller that wants to store a Code / Procedural / External frame via `put_with` would get a frame stamped `Semantic` on disk — even though tags say otherwise.

**Fix shipped (2026-04-22).**
- `remember_with_pillar` now calls `FrameStore::set_pillar(frame_id, pillar)` immediately after `put_with`. The `remember_as_*` family routes through `remember_with_pillar`, so they're all correct (verified in `pillar_writers_probe.rs`).
- New helper `FrameStore::put_with_pillar(opts, pillar)` wraps both in one call for callers that use `put_with` directly.

**Still to do.** The existing direct `put_with` callers in `document_ingest.rs`, `code_search.rs`, `whisper_ingest.rs`, and `said-cli/main.rs`'s `cmd_init` / `cmd_add_dir` / `cmd_ingest` paths have NOT been migrated yet. They still produce `pillar:<name>` tags (discriminator preserved) but store `FrameMeta.pillar == Semantic`. Migrating them to `put_with_pillar` is a low-risk sweep: change the call site, set the correct pillar, no behavior change for retrieval (tag-scoped filtering already works). Schedule when doing the next pillar-touching change.

### Migration adapter — mem0 format coverage (row 48)

**Shipped.** JSONL format from `mem0.export_memories()`. Fields: `id`, `memory`, `user_id`, `created_at`, `categories`, `metadata`.

**Stubbed / deferred.**
- **mem0 SQLite dumps** — mem0's default on-disk store is SQLite. A real operator running mem0 with local persistence has a `memories.db` file, NOT a JSONL export. We didn't add a SQLite adapter because it pulls `rusqlite` into `sca-core` (~1 MB of C deps); shipping as a separate crate / feature-gated adapter is the right next step. Until then, users must run `mem0.export_memories()` first.
- **mem0 cloud / API exports** — the hosted SaaS uses a different REST shape. No adapter yet.

**Stubbed / deferred.**
- **`ZepAdapter`** — declared in the spec, not implemented. Zep's session export JSON format is stable enough; ~1 day of work.
- **`LangMemAdapter`** — LangChain memory primitives export to JSONL but the schema has moved twice in 2026; wait for stabilization.

**What works today.** `MemvidAdapter` (JSON array) and `Mem0Adapter` (JSONL) are both real, tested, and shipping. `said import --from mem0 --source memories.jsonl` works end-to-end.

### Plugin ecosystem — discovery loader not implemented (row 50)

**Shipped.** The `SaidPlugin` trait, `PluginManifest`, `PluginRegistry` with enterprise-mode refusal. Register a plugin via `registry.register(Box::new(MyPlugin))` and the lifecycle hooks fire correctly (tested).

**Stubbed / deferred.**
- **`said plugin install <name>`** — the CLI command doesn't exist yet. The registry has no way to load plugins from a `.said-plugins/` directory or a `plugins.toml` manifest file. Every plugin currently has to be compiled into a binary that calls `PluginRegistry::register` explicitly.
- **First-party lift** — `said-sql-pack`, `said-codebase-pack`, `said-pdf-pack` still live in-tree and DON'T go through `PluginRegistry`. The trait was designed to hold them; the lift is scheduled with the UI sprint.

**Interim workaround.** Operators can fork `said-mcp`, add their plugins to `main.rs` registration, rebuild. Fine for one or two plugins; doesn't scale to an ecosystem.

### Audit log — actor attribution depends on caller discipline (row 45)

**Shipped.** BLAKE3-chained log, persists, verifies, surfaces in CLI/MCP admin.

**Gap.** The `actor` field defaults to `"owner"` for Portable brains. Enterprise brains SHOULD have MCP dispatch set the actor per-call based on the session's app-id, but the MCP dispatch layer doesn't yet wire this up — it currently still writes `"owner"` for every MCP-originated action.

**Still to do.** Extract the caller's app-id from the MCP request context (transport-level), call `brain.audit_mut().set_actor(app_id)` before each `handle_*`, then restore to `"owner"` after. Low-risk; one patch in `handler.rs`.

### Enterprise mode — AppGrant registry not wired into MCP dispatch (row 45)

**Shipped.** `AppGrantRegistry` + strict-mode support with unit tests for permission checking.

**Gap.** The registry isn't instantiated by the MCP server and no dispatch-level `registry.check(app_id, action)` happens before tool execution. Enterprise deployments today get the log but not the enforcement.

**Still to do.** Add a config load step in `said-mcp/src/main.rs` that reads a TOML grants file for Enterprise brains, constructs the registry with `strict=true`, and inserts a `check` call at the top of every mutating `handle_*`.

### Competitor benchmark — placeholder rows not populated (row 49)

**Shipped.** Harness + JSON writer + human-readable matrix. SAID-ECHO row is live (LoCoMo F1 0.856, MTEB 1.00 / 1.00 / 0.98 / 0.89). Mem0's two published numbers (0.669 / 0.684) are encoded.

**Gap.** All other competitor rows are `null`. Filling them requires actually running Zep, LangMem, Letta, memvid, pgvector, ChromaDB, HippoRAG, LightRAG, Cognee against LoCoMo + MTEB + BEIR on the same data. That's an ops task (containerize each competitor, script a common harness, run in CI), not a code task.

**Interim.** The matrix is published empty-column so callers know what we compare ourselves against; numbers fill in when the CI job runs.

### Retention sweep — age filter is best-effort (row 42)

**Gap.** `said admin retention-sweep --older-than-days N` filters tombstones by `created_at < now - N*86400`. Leap seconds, clock skew, and timezone drift can push frames just under/over the cutoff. For GDPR compliance — which typically specifies retention in calendar days, not wall-clock seconds — this is within error bars but isn't formally defensible.

**Still to do.** Formalize retention timestamp semantics (UTC day-boundaries, documented). Cheap fix when the admin UI lands.

---

## Planned but not yet implemented

These rows in the MVP plan are `⏳ planned`:

### Row 38 — Migration adapters (Zep + LangMem)
Shape already defined by `MigrationAdapter` trait (row 48). Their export formats are still in flux — waiting for stable dumps.

### Row 39 — Competitor benchmark sweep (populate rows)
Harness shell ships (row 49). External stacks' numbers fill in when a CI job has those systems available.

### Row 40 — Plugin ecosystem (first-party lift + registry loader)
Trait ships (row 50). Lifting `said-sql-pack`, `said-codebase-pack`, `said-pdf-pack` to the trait and building the `said plugin install <name>` registry loader remain.

### Row 43 — User-management UI for admin operations
Backend ready (rows 42, 44, 45). Tauri / web UI consuming the MCP admin tool is the next build.

### Row 51 — said-forge — spec-driven workspace generator
**Shipped 2026-04-23.** Feature-gated crate `crates/said-forge/` that generates a per-story workspace from a `.said` brain + a directive (OpenAPI / Markdown). See [docs/said-structure/05-features/forge.md](said-structure/05-features/forge.md) for the full feature page.

| Column | Value |
|---|---|
| Status | Shipped 2026-04-23 |
| Cargo feature | `forge` on `said-cli` + `said-mcp` (NOT an `sca-core` feature) |
| Crate | [`crates/said-forge/`](../crates/said-forge/) |
| CLI verbs | `forge load\|list\|show\|status\|run\|reset` |
| MCP tools | `forge_list`, `forge_get`, `forge_status`, `forge_load`, `forge_run` (deferred — returns CLI hint), `forge_reset` |
| Tool count | 25 baseline → 31 with `--features forge` |
| LLM providers | AnthropicProvider (tools-based structured output + prompt caching), OpenAICompatibleProvider (`response_format: json_schema strict`) |
| Frame tags | `forge:directive:<hash>`, `forge:story:<hash>:<slug>`, `forge:request:*:rN`, `forge:run:*:rN:{input,prompt,output,meta}`, `forge:{spec,plan,tasks,brain}:<hash>:<slug>` |
| Projection | `.forge/<slug>/{story.md, plan.md, tasks.md, brain.md, .forge-meta}` + `.claude/skills/<slug>/SKILL.md` |
| Tests | 151 (135 lib + 16 E2E acceptance), `cargo test -p said-forge --features stub-llm` |
| Smoke | 20-op petstore round-trips through CLI; 6 MCP tools exercised over stdio JSON-RPC |
| Spec | [`docs/superpowers/specs/2026-04-22-said-forge-design.md`](superpowers/specs/2026-04-22-said-forge-design.md) |
| Feature page | [`docs/said-structure/05-features/forge.md`](said-structure/05-features/forge.md) |

### Future (research spike)
DeltaNet parametric memory injection — separate repo (SAID-LAM-private) + GPU time needed.

---

## Maintenance rules

1. **When shipping a new feature, add a row to the MVP plan AND a section here in the same commit.**
2. **When behavior changes materially, update this catalogue.** Row summary in MVP plan points to here; here points to source files with line-precise links.
3. **Every section must have a testable "How to test" paragraph.** If you can't write that, the feature isn't done.
4. **Extension notes are required.** A future contributor should be able to add a variant (pillar, adapter, action, plugin hook) by reading one section.

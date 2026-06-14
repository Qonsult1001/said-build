# Known limitations + proposed enhancements

Every shortfall surfaced during the MVP build, grouped by subsystem. Each item has:

- **Limitation** — what's broken / missing / brittle today
- **Impact** — what it costs a user, and in what scenario
- **Enhancement** — what we would change to fix or upgrade it
- **Tracked in roadmap §** — pointer to the matching checklist entry in [12-roadmap.md](12-roadmap.md)

If you change a limitation here, update the matching roadmap entry in the same commit.

---

## 1. Retrieval

### 1.1 LoCoMo temporal category sits at 0.391 R@10
- **Limitation** — no time-aware scoring. Queries like "what did Alice say last Thursday" don't prefer frames whose timestamp falls in the relevant range.
- **Impact** — weakest LoCoMo category by a wide margin. Most real-world chat recall is temporal.
- **Enhancement** — parse temporal phrases from the query ("yesterday", "last week", "on March 5") via a tiny NLU, then boost frames whose `created_at` overlaps the range. No LLM needed; a 200-line grammar covers 90% of phrasings.
- **Roadmap §** — [Retrieval / Temporal scoring](12-roadmap.md#retrieval)

### 1.2 NarrativeQA at 0.721 — below the MTEB class average
- **Limitation** — graph fan-out finds the right paragraph in only ~72% of long-form narratives. Multi-hop bridge kicks in, but shallow.
- **Impact** — weakest MTEB number. Blocks any "book QA" or "long report" use case.
- **Enhancement** — deeper multi-hop walk (3-hop instead of 2) + pillar-aware rerank that boosts Episodic frames when the query language is narrative ("what happens after", "who does X meet").
- **Roadmap §** — [Retrieval / Multi-hop depth](12-roadmap.md#retrieval)

### 1.3 Relative cutoff (0.30 × top) is static
- **Limitation** — the `drop-results-below-top×0.30` rule is a constant. Some queries have one clearly-correct doc (cutoff should be 0.50); some have many equally-valid docs (cutoff should be 0.10).
- **Impact** — occasional relevant result pruning on loosely-specified queries.
- **Enhancement** — dynamic cutoff = f(top confidence, candidate distribution variance). If top is 1.0 and rank 2 is 0.9, narrow; if top is 0.5 and rank 2 is 0.49, widen.
- **Roadmap §** — [Retrieval / Dynamic cutoff](12-roadmap.md#retrieval)

### 1.4 BM25 fusion is route-gated (on for ask, off for search)
- **Limitation** — `search` skips BM25 deliberately to preserve the "pure semantic" contract. But some `search` callers would benefit from fusion.
- **Impact** — inconsistent behaviour between `ask` and `search`; requires users to know which one to use.
- **Enhancement** — optional `--fuse` flag on `search`, off by default. Keep the contract, let power users opt in.
- **Roadmap §** — [CLI / Search uniformity](12-roadmap.md#cli--mcp)

### 1.5 No tri-gram for Unicode beyond ASCII
- **Limitation** — trigram index tokenizes on 7-bit ASCII. CJK / Arabic / Hebrew corpora get no trigram hits; they fall back to SCA-only.
- **Impact** — slower + less precise retrieval for non-Latin-script brains.
- **Enhancement** — UTF-8-aware trigram with Unicode normalization (NFKC) before windowing.
- **Roadmap §** — [Retrieval / Unicode trigrams](12-roadmap.md#retrieval)

## 2. Pillars + memory model

### 2.1 `Factual → Semantic` collapse in legacy ingestion paths
- **Limitation** — frames written via `put_with` (the pre-pillar API, still used by `document_ingest`, `code_search`, `whisper_ingest`) collapse to Semantic even when they should be Procedural/Code/External. Shipped mitigation: `remember_with_pillar` explicitly calls `set_pillar` after `put_with`, but direct callers bypass it.
- **Impact** — per-pillar retrieval scope can miss frames that logically belong to Procedural/Code but are tagged Semantic.
- **Enhancement** — sweep the three remaining call sites to go through `put_with_pillar`. Low-risk, tracked.
- **Roadmap §** — [Pillars / legacy call-site sweep](12-roadmap.md#pillars)

### 2.2 No Episodic-timeline pillar distillation
- **Limitation** — Episodic frames pile up without any summary anchor. After 10k chat turns, the cheapest retrieval (for "what did we discuss yesterday") is still to scan all Episodic frames.
- **Impact** — LoCoMo temporal bottleneck, real-world latency ceiling.
- **Enhancement** — daily/weekly rollup — a Semantic frame that summarizes the last N Episodic frames (built offline, BYO-LLM). The brain stores the rollup as a content-addressable frame; the original Episodic frames stay as backup.
- **Roadmap §** — [Pillars / timeline distillation](12-roadmap.md#pillars)

### 2.3 Pillar is not enforced on ingest paths
- **Limitation** — `said ingest file.pdf` picks External pillar by default. `said add "note"` defaults to Semantic. Users can't override at the CLI.
- **Impact** — awkward for batch workflows.
- **Enhancement** — add `--pillar` flag to `add / ingest / init`. Trivial change.
- **Roadmap §** — [CLI / pillar flag](12-roadmap.md#cli--mcp)

## 3. Dream + brain state

### 3.1 Dream v1 content concatenation was pulled
- **Limitation** — v1 attempted to distill Episodic clusters by concatenating content. Hurt LoCoMo R@10 by 2pt. v2 only moves S_slow + recall_weight; leaves content alone.
- **Impact** — we don't yet get the "memory consolidation" benefit; only the fingerprint-drift benefit.
- **Enhancement** — v3 should distill content too, but via an **external** LLM call the caller makes, writing the distilled result as a new Semantic frame with `source_frames:` tags pointing to the original Episodics. The `.said` binary stays LLM-free.
- **Roadmap §** — [Dream / v3 distillation](12-roadmap.md#dream)

### 3.2 Dream threshold is only time-bucket aware
- **Limitation** — `dynamic_dream_threshold(N)` scales with corpus size linearly. Doesn't account for *topical* density (a burst of 50 related queries should trigger faster).
- **Impact** — slight lag before dream picks up a new topic.
- **Enhancement** — track rolling topic-variance; trigger when variance drops below a threshold regardless of count.
- **Roadmap §** — [Dream / adaptive threshold](12-roadmap.md#dream)

## 4. Surprise detector

### 4.1 Heuristic only — no ML model
- **Limitation** — surprise uses `(similarity, token_overlap)` thresholds. Works well on dominance-based similarity, but misses nuanced contradictions (paraphrase with flipped polarity).
- **Impact** — false-negatives on subtle reconsolidation events.
- **Enhancement** — optional ML scorer trained on SNLI-style entailment pairs, behind a feature flag. Keep the heuristic as fallback.
- **Roadmap §** — [Surprise / ML scorer](12-roadmap.md#surprise)

## 5. File format + storage

### 5.1 Save rewrites the entire file
- **Limitation** — `SaidFile::save()` writes the full brain every time, even for single-frame changes. OK at 10 MB; painful at 1 GB.
- **Impact** — perceptible latency on large brains; wears SSDs harder than needed.
- **Enhancement** — append-only incremental write path: keep existing BLKT/SYMS/TRGM sections, append a delta section, rewrite FTOC only. Compact periodically merges deltas.
- **Roadmap §** — [Storage / incremental save](12-roadmap.md#storage)

### 5.2 BRAN bloat fix is in place but legacy files may still be oversized
- **Limitation** — the BRAN deserialize bug (read past its section) was fixed (memory: `project_bran_bloat_fix.md`). But brains saved by pre-fix builds are still 2× bloated.
- **Impact** — disk space only; no behavioural bug.
- **Enhancement** — add `said admin compact --rebuild-bran` that re-serializes BRAN from the in-memory state.
- **Roadmap §** — [Storage / BRAN re-pack](12-roadmap.md#storage)

### 5.3 Windows file-lock fragility during rename-on-save
- **Limitation** — mmap handles on Windows sometimes survive the atomic rename, leaving `.said` files with stale blocks. Mitigated by careful mmap drop; still fragile.
- **Impact** — rare but when it happens, requires `said clean` or a restart.
- **Enhancement** — switch to `std::fs::rename` with retry loop + explicit `drop(mmap)` before rename. Already partly in; make it universal.
- **Roadmap §** — [Storage / Windows rename robustness](12-roadmap.md#storage)

### 5.4 No cross-platform path normalization on ingest
- **Limitation** — Windows paths stored as `C:\users\…`; Unix paths as `/home/…`. Cross-syncing a brain between OSes breaks `source:` tag resolution.
- **Impact** — Dropbox / team-share cross-OS workflows.
- **Enhancement** — normalize to forward-slash + `~/` or project-relative on ingest; resolve at query time.
- **Roadmap §** — [Storage / portable paths](12-roadmap.md#storage)

## 6. Ingestion plugins

### 6.1 Scanned PDF OCR requires `pdfium` at runtime
- **Limitation** — `ocr` feature needs `pdfium.dll` / `libpdfium.so` reachable. Without it, scanned PDFs skip silently.
- **Impact** — surprise failure for users who compiled with `ocr` but never installed pdfium.
- **Enhancement** — bundle a fallback rasterizer (slower, no external dep) that kicks in when pdfium isn't found.
- **Roadmap §** — [Ingestion / pdfium fallback](12-roadmap.md#ingestion)

### 6.2 Whisper on macOS / Linux is CPU-only
- **Limitation** — `directml` feature only works on Windows. On Mac/Linux, whisper runs on CPU.
- **Impact** — 10-50× slower transcription on non-Windows.
- **Enhancement** — wire Metal (macOS) and CUDA/ROCm (Linux) backends via sherpa-rs feature flags.
- **Roadmap §** — [Ingestion / whisper GPU cross-platform](12-roadmap.md#ingestion)

### 6.3 Code plugin: no Kotlin, Swift, Scala, Ruby, PHP
- **Limitation** — tree-sitter grammars cover 7 languages today.
- **Impact** — can't cleanly ingest JVM non-Java, mobile non-TS, Rails, WordPress.
- **Enhancement** — add grammars. Each is ~15 lines in Cargo.toml + a dispatch case in `code_chunk`.
- **Roadmap §** — [Ingestion / more languages](12-roadmap.md#ingestion)

### 6.4 Init is recursive but gitignore-aware; ingest (MCP) is shallow
- **Limitation** — CLI `init` walks with gitignore; MCP `ingest` on a folder walks only one level deep. Inconsistent.
- **Impact** — agents invoking MCP to ingest nested trees get partial coverage.
- **Enhancement** — unify behind a shared walker; keep gitignore behaviour by default.
- **Roadmap §** — [Ingestion / walker uniformity](12-roadmap.md#ingestion)

## 7. CLI / MCP

### 7.1 `said watch` deferred
- **Limitation** — Row 29. The "watch a folder, re-ingest on change" command is specced but not shipped.
- **Impact** — users manually run `said ingest path` after edits.
- **Enhancement** — `notify`-based watcher that debounces + calls the same ingest codepath.
- **Roadmap §** — [CLI / said watch](12-roadmap.md#cli--mcp)

### 7.2 No `said diff <ver1> <ver2>`
- **Limitation** — `history` lists versions but doesn't show content diffs.
- **Impact** — debugging "what changed between v3 and v4" is manual.
- **Enhancement** — `said diff doc_X v3 v4` → unified diff on content.
- **Roadmap §** — [CLI / diff](12-roadmap.md#cli--mcp)

### 7.3 MCP can't stream
- **Limitation** — MCP returns all `ask` results in one JSON-RPC response. Large result sets pause the client.
- **Impact** — UI feel on 100+ result queries.
- **Enhancement** — MCP streaming tool variant — emit notifications as results arrive. Requires protocol upgrade; deferred.
- **Roadmap §** — [MCP / streaming](12-roadmap.md#cli--mcp)

### 7.4 Admin UI not shipped (Row 43)
- **Limitation** — admin surface is CLI-only. No web UI for legal hold management / audit viewing.
- **Impact** — enterprise adopters need a UI for compliance auditors.
- **Enhancement** — small local web app (React + Rust axum backend) mapping 1:1 to `said admin`. Can start as an MCP tool called from Claude Code.
- **Roadmap §** — [Admin / UI](12-roadmap.md#admin--audit)

## 8. Module workflow (snapshot + sandbox)

### 8.1 Snapshot doesn't auto-refresh on parent changes
- **Limitation** — lens `frame_ids` are snapshot at creation time. New parent frames tagged `module:card` don't appear until re-snapshot.
- **Impact** — stale views. Users forget to re-run.
- **Enhancement** — `said snapshot card --live` mode: store a query in the lens instead of a frozen frame_id set; resolve at every query. Plus a `said snapshot --refresh` shortcut for the frozen case.
- **Roadmap §** — [Snapshot / live lens](12-roadmap.md#module-workflow)

### 8.2 Hub threshold hard-coded at ≥ 5 FKs
- **Limitation** — the "is this a hub table" rule is a magic number.
- **Impact** — some brains want 3, some want 10.
- **Enhancement** — `said snapshot card --hub-threshold 3`.
- **Roadmap §** — [Snapshot / config](12-roadmap.md#module-workflow)

### 8.3 Sandbox is SQL Server-only
- **Limitation** — the compose + schema-rendering templates are SQL Server 2022 specific.
- **Impact** — Postgres / MySQL / MongoDB monoliths can't use sandbox.
- **Enhancement** — template dispatch on brain metadata tag `sql_dialect:<postgres|mysql|mssql>`. One template file per dialect.
- **Roadmap §** — [Sandbox / dialects](12-roadmap.md#module-workflow)

### 8.4 Sandbox dependency sort is best-effort
- **Limitation** — function-ref grep is whole-word + comment/string stripping. Schema-qualified identifiers and dynamic SQL can evade it.
- **Impact** — rare mis-ordering; surfaces as schema-load errors the run-counter reports.
- **Enhancement** — proper SQL parser (sqlparser-rs) for the dep graph. Heavier dep, more accurate.
- **Roadmap §** — [Sandbox / accurate dep-sort](12-roadmap.md#module-workflow)

### 8.5 No cross-OS path portability in sandbox output
- **Limitation** — `docker-compose.yml` uses relative paths but `run.sh` uses `/bin/bash` — doesn't run on Windows without WSL / Git Bash.
- **Impact** — Windows-native users need WSL for bring-up.
- **Enhancement** — also emit `run.ps1` and `run.cmd`.
- **Roadmap §** — [Sandbox / multi-shell](12-roadmap.md#module-workflow)

## 9. Enterprise / compliance

### 9.1 Legal hold is single-tag
- **Limitation** — `legal_hold:CASE-A` is one tag; stacking multiple cases on one frame just adds more tags. Works but scales awkwardly at 100+ cases.
- **Impact** — enterprise use with many concurrent matters.
- **Enhancement** — dedicated `legal_holds` section (sorted list) alongside `tags`. Indexable separately.
- **Roadmap §** — [Audit / legal hold index](12-roadmap.md#admin--audit)

### 9.2 AppGrant registry is in-process only
- **Limitation** — AppGrant entries (which apps can write to which pillars) live in the brain's own AUDT section. Fine for single-brain use; not portable across a fleet.
- **Impact** — fleet deployments need to sync grants manually.
- **Enhancement** — export/import grant bundles; optional central registry file.
- **Roadmap §** — [Audit / grant fleet sync](12-roadmap.md#admin--audit)

### 9.3 Audit chain is per-brain; no cross-brain attestation
- **Limitation** — BLAKE3 chain protects against in-file tampering. Doesn't protect against wholesale file swap.
- **Impact** — strong adversary can replace the whole `.said`.
- **Enhancement** — sign the head hash with an external key (OS keychain, HSM, or remote signing service) on every save. Verify on open.
- **Roadmap §** — [Audit / external attestation](12-roadmap.md#admin--audit)

## 10. Migration + interop

### 10.1 Migration adapters cover mem0 + memvid only
- **Limitation** — Row 48 shipped adapters for mem0 (JSONL + category→pillar map) and memvid. Zep, LangMem, Letta are specced but not shipped.
- **Impact** — users on those platforms have to hand-export.
- **Enhancement** — implement the remaining adapters; the trait is stable.
- **Roadmap §** — [Migration / more adapters](12-roadmap.md#migration)

### 10.2 No round-trip export to mem0 / memvid
- **Limitation** — adapters import only. If a user wants to move off `.said`, they have to write their own exporter.
- **Impact** — lock-in perception, even though the format is open.
- **Enhancement** — `said export --format mem0 out.jsonl`.
- **Roadmap §** — [Migration / export](12-roadmap.md#migration)

## 11. Plugin ecosystem (Row 50)

### 11.1 `SaidPlugin` trait shipped; no discovery/marketplace
- **Limitation** — the trait exists. But there's no `~/.said/plugins/` discovery path, no signing, no manifest repo.
- **Impact** — plugin authors have to fork + rebuild.
- **Enhancement** — directory-based discovery with a signed manifest; small registry file at `plugins.said-lam.dev`.
- **Roadmap §** — [Plugins / discovery](12-roadmap.md#plugins)

### 11.2 Enterprise-mode refusal is coarse
- **Limitation** — `PluginRegistry` refuses any plugin in Enterprise mode unless explicitly whitelisted. All-or-nothing.
- **Impact** — enterprise users who trust a specific vendor can't opt in per-plugin without changing code.
- **Enhancement** — per-plugin allow-list with capability scopes (read-only / pillar-limited / full).
- **Roadmap §** — [Plugins / enterprise scopes](12-roadmap.md#plugins)

## 12. Documentation

### 12.1 Rows 1-28 are thematically folded, not individually paged
- **Limitation** — rows 30-50 each have a dedicated `05-features/row-XX-*.md`. Rows 1-28 are rolled into the themed folders ([10-benchmarks](10-benchmarks/), [06-ingestion-plugins](06-ingestion-plugins/), etc.).
- **Impact** — links from git commits referencing "row 17" don't have a single destination.
- **Enhancement** — add `05-features/themes/row-17.md` stubs that redirect to the themed folder. Low priority.
- **Roadmap §** — [Docs / row stubs](12-roadmap.md#docs)

### 12.2 No architecture diagram yet
- **Limitation** — the relationship between pillars / fingerprints / trigram / SCA / brain state is explained in prose only.
- **Impact** — harder for new contributors to orient.
- **Enhancement** — one Mermaid diagram in [01-overview](01-overview/) showing the ingest → index → query flow.
- **Roadmap §** — [Docs / architecture diagram](12-roadmap.md#docs)

---

## 13. said-forge (MVP 2026-04-23)

### 13.1 `forge_run` over MCP is deferred
- **Limitation** — the MCP `forge_run` tool does not actually run the batch. `self.brain: Arc<Mutex<SaidFile>>` uses `std::sync::Mutex`, whose guard is not `Send` across `.await`. `run_one()` awaits the LLM inside the generation loop.
- **Impact** — AI editors that want to trigger generation from MCP must call the CLI instead (`said forge run --all --yes`). The tool returns a CLI-command hint when invoked.
- **Enhancement** — switch `self.brain` to `tokio::sync::Mutex` or an actor, then port the runner loop natively. Architectural change beyond the forge MVP scope.
- **Roadmap §** — [Forge / MCP run](12-roadmap.md#forge)

### 13.2 Projection folder overwrites engineer edits
- **Limitation** — re-running `said forge run` for a story atomically rewrites `.forge/<slug>/story.md` / `plan.md` / `tasks.md` / `brain.md`. Hand-edits to those files are lost without warning.
- **Impact** — engineers who want to customize the projection must commit to external git before re-running, or use `said forge reset` + edit the brain's `forge:spec:*` frame directly (which the lineage preserves).
- **Enhancement** — three-way merge (previous-generation / current-edit / new-generation) with conflict markers. Milestone C (sandbox/runtime) explores a worktree-per-story model that avoids the overwrite problem entirely.
- **Roadmap §** — [Forge / edit preservation](12-roadmap.md#forge)

### 13.3 `confirm:true` gate is convention, not enforced
- **Limitation** — the MCP write tools (`forge_load`, `forge_reset`) refuse calls without `confirm:true`. The skill file instructs the AI to ask the user before setting it — but a hostile or non-compliant client can set `confirm:true` directly.
- **Impact** — not a security boundary. Destructive actions require cooperation from the AI, not hard authorization.
- **Enhancement** — a separate pre-flight confirm-token tool the AI must echo back. Adds latency; not in MVP.
- **Roadmap §** — [Forge / hard confirm gate](12-roadmap.md#forge)

### 13.4 `forge_get` 25k-token cap truncates bluntly
- **Limitation** — when the bundled markdown exceeds 25 000 tokens (≈100 000 chars), each section is proportionally truncated at a UTF-8 char boundary with a `[TRUNCATED — call get <frame-id>]` marker. The AI must drill down via individual `get <frame-id>` calls for full body.
- **Impact** — edge case. Only triggers for stories with unusually long grounding or spec bodies.
- **Enhancement** — structured response that includes the truncation metadata separately so clients can render a "fetch full" button. Not blocking MVP.
- **Roadmap §** — none (accept)

### 13.5 Single active directive per brain
- **Limitation** — loading a new directive via `said forge load` does not remove the previous directive's frames — they stay as history (`said history forge:directive:*`). The CLI's `latest_directive_hash` picks whichever was committed most recently. A single brain can't simultaneously hold multiple active directives.
- **Impact** — multi-project brains must use separate `.said` files, one per directive.
- **Enhancement** — explicit directive selection flag on `forge run` / `forge list`. Not in MVP.
- **Roadmap §** — [Forge / multi-directive](12-roadmap.md#forge)

### 13.6 Anthropic prompt-cache TTL is 5 minutes
- **Limitation** — the prompt-cache discount (90% on cache-read tokens) only applies within Anthropic's 5-minute TTL. Long batches where cold stories take >5 min to reach will re-pay the full prelude cost.
- **Impact** — cost-efficient only when stories are generated in ≥50-story contiguous groups.
- **Enhancement** — chunk the batch into groups of 50 and insert explicit cache-warmup at the start. Opportunistic; v2.
- **Roadmap §** — none (accept)

### 13.7 Server-side MCP tag filtering not implemented
- **Limitation** — all 6 forge tools carry `[forge]` in their description string, but there's no server-side `--mcp-tags +forge` / `-forge` flag to filter the tool listing.
- **Impact** — AI editors see all 31 tools regardless of need. Client-side filtering by description prefix works.
- **Enhancement** — wire a `--mcp-tags` flag through `said-mcp` that filters at `tools/list` time.
- **Roadmap §** — [Forge / mcp tag filter](12-roadmap.md#forge)

---

## Maintenance rule

When any item above is fixed:

1. Delete the entry from this file.
2. Tick the matching checkbox in [12-roadmap.md](12-roadmap.md).
3. If the fix changed public behaviour, add a note to the relevant page (e.g. fix for 1.1 updates [10-benchmarks/locomo.md](10-benchmarks/locomo.md)).

Every limitation is a promise to do something — or an explicit decision not to. No silent carry-overs.

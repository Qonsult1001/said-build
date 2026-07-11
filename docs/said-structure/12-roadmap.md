# Roadmap

This is the action side of [11-known-limitations.md](11-known-limitations.md). Every limitation listed there has a matching checkbox below. When an item is shipped, tick the box here AND delete the limitation entry there.

Ordering within each section is priority — top entries ship first unless otherwise noted.

---

## OUTSTANDING FOR LAUNCH — skill packs / shop (see [18-skill-pack-linking-and-trust.md](18-skill-pack-linking-and-trust.md))

The skill-pack system ships in gates; these are the must-finish items before selling packs:

- [x] **Multi-brain mount** (Gate 1+2) — auto-discover + federate read-only packs (commit 857c684).
- [x] **Canonical dedup** — coding-fix doc_id keys on task identity; re-learn supersedes, no pollution (commit aca446e).
- [x] **Ed25519 signing/verify (Gate 3), DETACHED** — `sca_core::pack_sign` + verify-at-mount against `~/.said/publishers` allowlist; refuse unknown/tampered. Sidecar `<pack>.said.sig`.
- [ ] **⚠️ Merge the signature INTO the `.said` file (in-file `SIGN` section).** Signing is currently a DETACHED `.sig` sidecar (zero file-format risk). Before launch, fold it into row-52 §1.2's in-file `SIGN` section (magic+sig+pubkey+alg, header slot) so a pack is ONE self-describing file with no sidecar to lose/strip. Same crypto; only the signature's location changes. **Launch blocker for the shop.**
- [ ] **`said pack` CLI** — `keygen` / `sign` / `verify` publisher + consumer commands (currently only the library API + verify-at-mount exist).
- [ ] **Gate 4 — sell/encrypt/license.** Paid packs = `Locked` + signed + AES-256-GCM encrypted; shop issues a license wrapping the content key to the buyer (`~/.said/licenses/`); decrypt-on-mount. Required to sell `apply.said` without leaking the IP.
- [ ] **`BrainMode::Locked`** at publish (row-52 §1.1) — engine-level read-only, immutable post-publish.
- [ ] **Registry** (`hub.said.app`: `registry.json` + `publishers.json`) + `said skills add/list/remove` CLI.

---

## Integrations — the Q2/Q3/Q4 plan (see [13-integrations.md](13-integrations.md))

Governed by the two rules: **(1) offline-first; online-only when inherent. (2) LLM agents are a separate process.** LEANN-validated local-file pattern is the Q2 push; OAuth pilots start Q3.

### Q2 — six offline integrations (now → 3 months)

All offline. No OAuth work. LEANN's [`apps/`](../../research/LEANN/apps/) directory is the porting reference.

- [ ] **`said-watch`** — filesystem watcher; auto-ingest on change via existing content-type crates. ~1 week.
- [ ] **`said-git`** — local `.git` dir ingester via `git2`; one Episodic frame per commit, Code frames for per-file blame. ~1 week.
- [x] **`said-mail-local`** — SHIPPED as `said import email` (`.mbox` + Apple Mail `.emlx`; covers Gmail-Takeout + Outlook/M365 export). Ported from [LEANN `email_rag.py`](../../research/LEANN/apps/email_rag.py). Outlook **PST** still open; live Gmail/M365 API sync is the Q3 OAuth pilot below. See [personal-import](06-ingestion-plugins/personal-import.md).
- [x] **`said-browser-local`** — SHIPPED as `said import browser` (auto-detects every Chromium browser + profile: Chrome/Edge/Brave/Opera/Vivaldi, history SQLite, read-only + offline, GLOBAL recency). Ported from [LEANN `browser_rag.py`](../../research/LEANN/apps/browser_rag.py). Firefox/Safari still open.
- [ ] **`said-chat-local`** — iMessage chat.db + WhatsApp/Signal/Telegram/Slack/Discord/Teams exports. Port from [LEANN `imessage_rag.py`](../../research/LEANN/apps/imessage_rag.py) + [`slack_rag.py`](../../research/LEANN/apps/slack_rag.py). ~1.5 weeks.
- [ ] **`said-obsidian`** — Obsidian vault ingester, preserves `[[wikilinks]]` as cross-doc tags. ~3-5 days.

Total: ~6 focused weeks. Every item demonstrable end-to-end, every item offline.

### Q3 — the OAuth pilot

- [ ] **`said-gmail-live`** — Gmail IMAP + OAuth2. First live-API integration; proves token-refresh + incremental-sync pattern. 2-3 weeks.
- [ ] **`said-slack-live`** (stretch) — Slack Web API + OAuth. ~1.5 weeks once Gmail pattern is proven.

### Q4 — online integrations + `said-think`

- [ ] **`said-notion-live`** — Notion API + OAuth. ~1.5 weeks.
- [ ] **`said-jira-live`** — Atlassian REST + OAuth 2.0 3LO. ~1.5 weeks.
- [ ] **`said-github-live`** — GitHub API (simpler auth model). ~1 week.
- [ ] **`said-think` v1** — the one LLM-using agent, separate process, MCP client. Dream v3 + KG build. ~3-4 weeks including audit story. See [13 § said-think](13-integrations.md#said-think--the-one-online-component).

### Year-one target

**12-15 shipped integrations + `said-think`.** Matches LEANN's validated breadth; differs from Dume/Composio by staying local-first per [Rule 1](13-integrations.md#rule-1--offline-first-online-only-when-inherent).

## Retrieval

- [ ] **Temporal scoring** — parse temporal phrases ("last week", "on March 5") from query; boost frames whose `created_at` overlaps. Target: LoCoMo Cat 3 from 0.391 → 0.55+. Blocks conversational-memory product.
- [ ] **Multi-hop depth** — 3-hop bridge walk + pillar-aware rerank on narrative-language queries. Target: NarrativeQA 0.721 → 0.80+.
- [ ] **Dynamic relative cutoff** — replace static 0.30 factor with variance-aware rule. Expected MTEB delta: +0.01 average, much better worst-case. See [14.3 relative cutoff](14-novel-mechanisms/14.3-relative-cutoff.md).
- [ ] **Unicode trigrams** — NFKC normalize + UTF-8-aware windowing. Enables CJK / Arabic / Hebrew brains.

## Novel mechanisms — horizon items (see [14-novel-mechanisms/](14-novel-mechanisms/))

Four mechanisms promoted from the novel-mechanisms chapter as concrete work items, in priority order:

- [ ] **Latent PageRank** — ~100 lines of Rust. Power-iterate `S_slow`'s eigenstructure to get per-latent-axis centrality; use it to reweight query components. Target: LoCoMo Cat 2 +0.03, NarrativeQA +0.03. Smallest-effort horizon item with measurable lift. Spec: [14.10](14-novel-mechanisms/14.10-latent-pagerank.md).
- [x] **Float rerank of returned top-K** (2026-06-22) — recall@1 among one entity's many short memories was poor not because of a "64-dim ceiling" (an earlier wrong call) but because the 1-bit doc fingerprint discards per-dim magnitude, so near-collision candidates tie. Fix: in `ask`, when the result set is semantic-led (no discriminating multi-keyword lexical hit), re-encode the query + each returned candidate's text and reorder by full 64-dim float cosine — the documented QJL near-collision recovery ([14.1](14-novel-mechanisms/14.1-qjl-asymmetric.md)/[14.8](14-novel-mechanisms/14.8-matryoshka-signbit.md)). Zero storage change (re-encode the handful returned, ~µs each). Measured: same-entity 2/5→5/5, recall@1 at N=200 0.195→0.855, lexical needle preserved 30/30, recall@10 1.0. Test: `tests/test_float_rerank_value.rs`.
- [ ] **Differential privacy via fingerprint dithering** — ~300 lines Rust + LaTeX proof + academic paper. Adds formal (ε, δ)-DP to retrieval; first shipping memory system with this property. Utility cost ≤ −0.005 at ε=0.1. Publishable at VLDB / CIDR. Spec: [14.11](14-novel-mechanisms/14.11-differential-privacy.md).
- [ ] **Holographic K-view fingerprint** — ~200 lines Rust. $K$ rotated fingerprints per frame, median Hamming across views. Effective 1024-bit resolution at 128 bytes. Infra hook already exists in `ScaEngine`. Target: SummScreenFD 0.98 → 0.99+. Spec: [14.9](14-novel-mechanisms/14.9-holographic-k-view.md).
- [ ] **Crystalline annealing** — ~500 lines Rust. Periodic eigenbasis re-alignment of fingerprints to `S_slow`'s principal axes. Enables compressed (16/32-bit) truncated retrieval modes as a secondary benefit. Spec: [14.12](14-novel-mechanisms/14.12-crystalline-annealing.md).
- [ ] **Query-side rehearsal** (DEFERRED — wait-and-see) — needs session benchmark harness first; revisit after the four above ship. Spec: [14.13](14-novel-mechanisms/14.13-query-rehearsal.md).

## Pillars

- [ ] **Legacy call-site sweep** — port `document_ingest`, `code_search`, `whisper_ingest` from raw `put_with` to `put_with_pillar`. Kills the Factual→Semantic collapse (limitation 2.1).
- [ ] **Episodic timeline distillation** — daily/weekly Semantic rollup frame backed by BYO-LLM caller. Biggest single LoCoMo lift available.

## Dream

- [ ] **v3 content distillation (BYO-LLM)** — caller-driven distillation writes Semantic rollups with `source_frames:` back-pointers; `.said` binary stays LLM-free.
- [ ] **Adaptive threshold** — include rolling topic-variance; trigger dream on topic burst independent of count.

## Surprise

- [ ] **ML scorer behind feature flag** — optional SNLI-trained entailment model; heuristic remains default. Captures subtle contradictions heuristic misses.

## Storage

- [ ] **Incremental save** — append-only delta section + FTOC-only rewrite; `compact` merges deltas. Removes the full-file-rewrite latency cliff on large brains.
- [ ] **BRAN re-pack** — `said admin compact --rebuild-bran` to clean up files saved by pre-fix builds.
- [ ] **Windows rename robustness** — explicit `drop(mmap)` + retry loop on every save path.
- [ ] **Portable paths** — forward-slash + project-relative on ingest; resolve at query time. Enables cross-OS Dropbox workflows.

## Ingestion

- [ ] **🟡 large-repo ingest: crash + Phase-3 hangs FIXED, ingests end-to-end — 580 MB peak open on low-RAM**
  (known-limitation [1.6](11-known-limitations.md)). **DONE:** (a) CSV data-dump exclusion (`should_enroll`
  5 MB cap) — the real crash cause; (b) OKF title-scan O(N²)→linear + harvest clustering O(N²)→blocked (both
  record-linkage blocking, deterministic) — the two Phase-3 hangs; (c) per-system spill budget
  (`clamp(RAM×12%, 16 MB, 512 MB)`, sysinfo); (d) WIDX disk-backed word index. **Proven:** full-defaults
  Wonga (OKF + harvest + auto-spill) ingests END-TO-END — 82.3 MB brain, 37,790 memories, 14,560 symbols,
  recall verified. Phase-1 read 200–370 s → 14 s. **Still open:** peak on a high-RAM machine is ~2 GB, from
  the **compact transient** (`frames.rs::compact_block_dict`: `raw_frames` all-frames-decompressed + a
  second `flat` copy for zstd dict training + all compressed blocks collected before merge) — NOT the word
  index (459 MB) or frames (285 MB). **Remaining actions:** (1) window the block compression + drop the
  `flat` full-copy (the last item to hit 580 MB on low-RAM devices); (2) parallelize Phase-1 read+chunk for
  more speed; (3) incremental + resumable init (BLAKE3 fast path). **Target:** 37 k-frame code brain within
  the 580 MB ceiling on low-RAM devices, re-init in seconds.
- [ ] **pdfium fallback** — bundled slow rasterizer when `pdfium.dll` not found at runtime.
- [ ] **Whisper GPU cross-platform** — Metal (macOS) + CUDA/ROCm (Linux) via sherpa-rs features.
- [ ] **More tree-sitter languages** — Kotlin, Swift, Scala, Ruby, PHP.
- [ ] **Walker uniformity** — MCP `ingest` on folder walks recursive + gitignore (matches CLI `init`).

## Forge

Follow-up work for the spec-driven workspace generator (MVP shipped 2026-04-23 — see [05-features/forge.md](05-features/forge.md)).

- [ ] **Forge MCP run** (Known §13.1) — switch `said-mcp`'s `self.brain` from `std::sync::Mutex` to `tokio::sync::Mutex`, port the runner loop to run natively over MCP. Removes the "use the CLI instead" fallback.
- [ ] **Forge edit preservation** (Known §13.2) — three-way merge on `.forge/<slug>/` rewrites so hand-edits survive a re-run. Alternative: worktree-per-story (Milestone C).
- [ ] **Forge hard confirm gate** (Known §13.3) — separate pre-flight confirm-token tool so `confirm:true` can't be set unilaterally by a non-compliant client.
- [ ] **Forge multi-directive** (Known §13.5) — explicit directive selection flag; multiple active directives in a single brain.
- [ ] **Forge MCP tag filter** (Known §13.7) — `--mcp-tags +forge` / `-forge` server-side filter on `tools/list`.
- [ ] **Milestone C — sandbox/runtime** — consume `.forge/<slug>/` + SKILL.md, spawn a worktree, execute the plan in isolation, commit results back. Separate spec.
- [ ] **Cursor editor adapter** — write `.cursor/rules/<slug>.mdc` alongside Claude's SKILL.md.
- [ ] **Copilot editor adapter** — write `.github/instructions/<slug>.instructions.md`.
- [ ] **Word directive adapter** — reuses `sca-core::document_ingest::extract_docx`, heading-mode extraction.
- [ ] **Plain-text directive adapter** — reuses `sca-core::document_ingest::extract_text`, one story per line.
- [ ] **Excel directive adapter** (two-PR) — (a) add XLSX extractor to `sca-core::document_ingest` via calamine; (b) add forge adapter reading rows as stories.
- [ ] **CSV directive adapter** (two-PR) — same shape as Excel.

## CLI + MCP

- [ ] **Pillar flag** — `--pillar` on `add / ingest / init`. Trivial.
- [ ] **Search `--fuse`** — opt-in BM25 fusion on `search` (default off, keeps semantic-only contract).
- [ ] **said watch** (Row 29) — notify-based folder watcher with debounced re-ingest.
- [ ] **said diff** — `said diff doc_X v3 v4` content diff between versions.
- [ ] **MCP streaming** — emit result notifications as they arrive. Requires protocol upgrade; deferred until an agent framework needs it.

## Admin + audit

- [ ] **Admin UI** (Row 43) — local web app (axum + React) wrapping `said admin`. Can ship as an MCP-invoked UI from Claude Code first.
- [ ] **Legal hold index** — dedicated section alongside tags; indexed separately for fleets with 100+ cases.
- [ ] **Grant fleet sync** — export/import AppGrant bundles; optional central registry file.
- [ ] **External attestation** — sign head hash with OS keychain / HSM / remote signer on save; verify on open.

## Module workflow

- [ ] **Live lens** — `said snapshot card --live` stores a query instead of frozen frame_ids; resolves at query time. No more stale views.
- [ ] **Snapshot config** — `--hub-threshold N` instead of hard-coded 5.
- [ ] **Sandbox SQL dialects** — template dispatch on `sql_dialect:<pg|mysql|mssql>` tag; Postgres first.
- [ ] **Accurate sandbox dep-sort** — swap greppy dep detection for sqlparser-rs-backed AST walk.
- [ ] **Multi-shell sandbox** — emit `run.ps1` + `run.cmd` alongside `run.sh`.

## Migration

- [ ] **Zep, LangMem, Letta adapters** — implement remaining `MigrationAdapter`s from spec (Row 38).
- [ ] **Export to mem0 / memvid** — `said export --format mem0 out.jsonl`. Removes perceived lock-in.

## Plugins

- [ ] **Plugin discovery** — `~/.said/plugins/` directory scan + signed manifests + small registry at `plugins.said-lam.dev`.
- [ ] **Enterprise per-plugin scopes** — allow-list with capability scopes (read-only / pillar-limited / full).

## Docs

- [ ] **Row stubs** — `05-features/themes/row-NN.md` redirect stubs for rows 1-28 so commit-referenced rows resolve to a single URL. Low priority.
- [ ] **Architecture diagram** — one Mermaid diagram in [01-overview](01-overview/) covering ingest → index → query.

## Ingestion format coverage gaps

We do NOT currently ingest a large class of everyday formats. Each one is a different feature flag / plugin. Prioritized by how often real users hand us the format and expect it to work.

### Office + tabular
- [ ] **XLSX / XLS** — spreadsheet ingest. Each sheet → frames, with row-level granularity for small sheets and column-summary frames for wide ones. Needs `calamine` or `umya-spreadsheet`.
- [ ] **CSV / TSV** — direct tabular ingest with header-aware column naming + per-row frames, plus a schema-summary frame.
- [ ] **Parquet / Arrow IPC** — for data-heavy workflows. Stream rows, not load-all. Needs `arrow-rs`.
- [ ] **ODS / ODT** — LibreOffice. Same story as xlsx / docx but different container format.
- [ ] **PPTX / KEY** — slide decks. One frame per slide with notes + body.
- [ ] **RTF** — legacy Word exports still common in legal.

### Mail + messaging
- [~] **MBOX / EML / MSG** — basic message ingest **SHIPPED** as `said import email` (`.mbox` + Apple Mail `.emlx`; headers → `from:`/`date:`/`sent_at:` tags; deduped by Message-ID). Still open: thread → lineage, attachments → auto-ingested with `email:` tag, `.msg` support.
- [ ] **PST / OST** — Outlook archives. Needs `libpff` or a Rust port.
- [ ] **Slack export (JSON)** — bulk team export → per-channel, per-thread frames.
- [ ] **Teams export** — same shape, Microsoft's export format.
- [ ] **WhatsApp / Signal / Telegram export** — personal chat archives.
- [ ] **iMessage SQLite** — macOS `chat.db` direct ingestion.
- [ ] **Discord export** — JSON from DiscordChatExporter.

### Direct data-source connections
- [ ] **Postgres / MySQL / MSSQL live connections** — `said ingest postgres://...` streams rows/tables with schema as metadata; no dump-to-file step. Needs `sqlx` + a pagination story for large tables.
- [ ] **SQLite file direct** — `said ingest foo.db` treats every table as a corpus.
- [ ] **MongoDB / Elasticsearch** — document stores. One frame per doc, collection → pillar tag.
- [ ] **Snowflake / BigQuery / Redshift** — cloud warehouses, read-only credentials.
- [ ] **Neo4j / Kuzu** — graph DBs, nodes + edges as frames with relationship tags.
- [ ] **Redis** — key-value snapshots, rarely needed but trivial.

### Dev + code ecosystems
- [ ] **Git commit + blame ingestion** — each commit → Episodic frame; each file blame → Code frame with author/time.
- [ ] **GitHub / GitLab APIs** — issues, PRs, reviews, comments.
- [ ] **Jira / Linear / Asana** — ticket bodies + comments as Episodic frames with `ticket:PROJ-123` tags.
- [ ] **Notion** — page export + live API.
- [ ] **Confluence** — wiki pages.
- [ ] **Obsidian vault** — Markdown + links → frames with symbol-style cross-refs.
- [ ] **Roam / Logseq** — block-level graph ingestion.

### Media + specialized
- [ ] **Audio — MP3 / M4A / OGG / FLAC** — expand beyond WAV/MP4. Symphonia already supports most; wire them through.
- [ ] **Video — MP4 / MKV / MOV** — frame-grab + ASR transcript + OCR on frames. Heavy but high-value.
- [ ] **Images — PNG / JPG / WEBP** — OCR pass + optional vision-caption frame (BYO-vision-model, not baked in).
- [ ] **EPUB / MOBI / AZW3** — ebooks. Chapter → frame with TOC as symbol index.
- [ ] **HTML (standalone) + MHTML** — saved web pages.
- [ ] **Markdown variants** — MDX, AsciiDoc, reST, Org-mode.
- [ ] **JSON / YAML / TOML** — structured config. Frame-per-top-level-key with path-addressable retrieval.
- [ ] **XML generic** — not just DOCX; also SOAP / DITA / arbitrary XML.
- [ ] **ZIP / TAR archives (transparent)** — unpack and recurse ingestion.

### IoT + streaming
- [ ] **Log files streaming** — `tail -f` mode for `said ingest --follow`; debounced batch writes.
- [ ] **MQTT topic subscription** — ingest sensor/IoT streams as Episodic frames.
- [ ] **Webhook endpoint** — `said serve --webhook` accepts POST JSON, persists as Episodic.

## Competitor parity sweep

We've got a competitor bench harness ([Row 49](05-features/row-49-competitor-bench.md)) but have NOT done a full feature-parity sweep against the current market. Each entry below is: what the competitor is best at, what we'd need to match or beat, whether we already do it, and the action item.

### Heavy-hitter memory systems

#### Mem0 — Production SaaS & Agents
- **Core tech:** Vector + Graph
- **Differentiator:** massive ecosystem integration (CrewAI, LangGraph); mature SDK
- **Parity status:** retrieval F1 0.8555 vs mem0 0.684 (we win). Ecosystem integration: missing.
- [ ] **Action:** CrewAI + LangGraph + AutoGen adapters; Python SDK parity (`pip install said`).

#### Zep / Graphiti — Temporal Reasoning
- **Core tech:** Temporal Knowledge Graph
- **Differentiator:** remembers WHEN things happened and how goals evolved
- **Parity status:** weakest LoCoMo category (0.391 temporal) — directly addresses their strength.
- [ ] **Action:** Temporal scoring (already in roadmap / retrieval); temporal edge inference between Episodic frames.

#### MemoryLake — Persistent AI Continuity
- **Core tech:** Portable memory layer
- **Differentiator:** portable across LLMs (GPT / Claude / Gemini)
- **Parity status:** we're already BYO-LLM — `.said` never calls an LLM. Portability is a core property, not a feature.
- [ ] **Action:** marketing-only — document the portability story explicitly in [01-overview](01-overview/what-is-said.md). No code change.

#### Cognee — Document-Heavy Tasks
- **Core tech:** KG + Vector
- **Differentiator:** builds a full typed knowledge graph before you query
- **Parity status:** we have a **retrieval-time entity graph** — Layer-6 bridge walk in [`recall.rs`](../../crates/sca-core/src/recall.rs) + entity/speaker/FK tag co-occurrence — documented in [3.9 Graph layer](03-core-subsystems/3.9-graph-layer.md). Matches Cognee on most multi-hop factual queries (we score 1.0 on WikimQA). Loses on (a) **exhaustive enumeration** ("list every X"), (b) **typed-relation filters** (our edges have no predicate), (c) **very-large-corpus traversal cost** (Layer 6 re-encodes per bridge).
- [ ] **Action:** optional `said build-graph` post-ingest step — extract typed `(subject, relation, object)` triples as `kg_edge:true` frames + persist a per-brain edge index section. Keeps the retrieval-time walker as default; promotes the pre-built graph when the index exists. Opt-in so brains that don't need it don't pay the build cost.

#### Hindsight — Institutional Accuracy
- **Core tech:** 4-Network Hybrid
- **Differentiator:** highest current accuracy benchmarks; open-source
- **Parity status:** they publish numbers on niche benchmarks we don't run.
- [ ] **Action:** add their benchmark suite to [10-benchmarks/](10-benchmarks/); compare head-to-head.

#### usecortex.ai — Commercial Personalization
- **Core tech:** commercial memory layer
- **Differentiator:** high-accuracy production voice + coding agents
- **Parity status:** we have the coding pillar (Code pillar writer, Row 47) and whisper audio ingest. Voice-agent integration: missing.
- [ ] **Action:** real-time voice agent harness — audio in → episodic out, with query-in-flight support.

### Personal + consumer

#### Rewind / Limitless — Personal Recall
- **Differentiator:** records screen, audio, video; "search engine for your life"
- **Parity status:** we don't capture screen. We have audio + docs + code.
- [ ] **Action:** `said capture-screen` background daemon writing periodic OCR'd snapshots as Episodic frames (BYO screenshot tool).

#### Dume.ai — Workflow Automation
- **Differentiator:** connects 50+ tools (Gmail, Slack, Jira) for fact memory
- **Parity status:** we don't have tool connectors. Listed in "Mail + messaging" + "Dev ecosystems" above.
- [ ] **Action:** prioritize Gmail + Slack + Jira adapters first (covers ~60% of Dume's claim).

#### Mem.ai — Knowledge Building
- **Differentiator:** smart note-taking with semantic connection
- **Parity status:** Obsidian-like note use case. Our Episodic/Semantic split covers it but UX is CLI/MCP, not a note app.
- [ ] **Action:** Obsidian vault adapter (above) + a minimal web UI for browsing.

#### Lindy — Executive Support
- **Differentiator:** rule-based memory, workflow automation
- **Parity status:** we have Procedural pillar (Row 46) which is the right abstraction; no rule-engine surface yet.
- [ ] **Action:** `said rule add` / `said rule match` — tiny rule matcher on top of Procedural pillar.

#### myNeutron — Developer Continuity
- **Differentiator:** prevents "context reset" by storing codebase docs persistently
- **Parity status:** this is literally what we do for code. Code pillar + symbol index + MCP integration with IDEs.
- [ ] **Action:** publish a "developer context continuity" positioning doc; ship VS Code extension that surfaces `said ask` on cursor.

### Overall parity roadmap item

- [ ] **Full competitor matrix** — scaffold shipped at [10-benchmarks/competitor-matrix.md](10-benchmarks/competitor-matrix.md); axes defined, table skeleton in place, SAID row filled. Next: run `competitor_bench` harness per competitor and tick rows as measurements land. Columns: {retrieval accuracy, latency p50/p99, binary size, offline, format coverage, temporal, graph, pillar model, ecosystem, BYO-LLM, license}.

---

## Cross-cutting quality gates

Every roadmap item must hold these before merge:

- [ ] MTEB Needle / Passkey / WikimQA stay at 1.0
- [ ] MTEB SummScreenFD stays ≥ 0.97
- [ ] MTEB QMSum stays ≥ 0.88
- [ ] LoCoMo R@10 stays ≥ 0.554 (improvements welcome; regressions require waiver)
- [ ] 30/30 chambers pass
- [ ] `sca-core` unit tests green
- [ ] `said-cli` + `said-mcp` release builds succeed with default feature matrix

## How to use this document

1. Pick the next uncheckeded item from the relevant section.
2. Read the matching entry in [11-known-limitations.md](11-known-limitations.md) for context.
3. Ship the change; measure the gates above.
4. On merge: tick the box here, delete the limitation entry there, update any affected docs.

Every change that fixes a limitation must touch **three files in the same commit**: the code, this roadmap (tick box), and 11-known-limitations.md (delete entry).

## Release planning

Rough grouping — not binding, but a steering signal:

**Next minor release (v0.3)**
- Temporal scoring (retrieval / 1.1)
- Legacy pillar sweep (pillars / 2.1)
- Episodic timeline distillation (pillars / 2.2)
- said watch (cli / 7.1)
- Pillar flag (cli / 2.3)

**Release after (v0.4)**
- Multi-hop depth (retrieval / 1.2)
- Incremental save (storage / 5.1)
- Whisper GPU cross-platform (ingestion / 6.2)
- Admin UI (admin / 7.4)

**Later**
- Dream v3 (dream / 3.1)
- Plugin discovery + marketplace (plugins / 11.1)
- Sandbox Postgres/MySQL dialects (module / 8.3)
- External attestation (audit / 9.3)

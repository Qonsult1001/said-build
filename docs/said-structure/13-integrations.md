# 13 — Integrations architecture

Every way `.said` gets data from outside the file, and the rule that governs what's in-process vs out-of-process.

## The two rules

The entire integration surface follows two rules. Every crate and every feature proposal must answer which rule it falls under. If you can't tell, the proposal is ambiguous and should be clarified before any code is written.

### Rule 1 — Offline-first; online-only when inherent

**Default: offline.** An integration must be offline by default. "Offline by default" means: no network call is required for the integration to produce value. The user plugs in a folder / file / local database, we ingest, we're done.

**Online is the exception,** permitted only when the source of truth is inherently online and no local snapshot exists — for example, a live Gmail IMAP pull when the user does not have a local mbox. Even in online integrations, prefer **user-driven local snapshots** (Google Takeout → mbox, Slack admin export → zip, Notion export → zip) as the first-ship path, with a live-API variant as a follow-on.

### Rule 2 — LLM calls live outside the binary

**Default: no LLM.** The shipping SAID binary never calls an LLM. Not for ingest, not for retrieval, not for consolidation. This is the BYO-LLM contract (see [01-overview/what-is-said.md](01-overview/what-is-said.md)) and it is the entire reason SAID stays small, offline, and portable.

**LLM agents are a separate process,** MCP-client to `said-mcp`. Today this is exactly one component: [`said-think`](#said-think--the-one-online-component). If we ever ship another LLM-using agent, same pattern — separate process, MCP client, not linked into `said-mcp`.

## The crate inventory

### Core crates — always linked

| Crate | Role |
|---|---|
| `sca-core` | retrieval, file format, brain state |
| `said-cli` | CLI dispatcher |
| `said-mcp` | MCP server binary — the one entry point |
| `said-forge` | authoring pipeline (forge Phase 2 in flight) |

### Content-type crates — offline, already feature-gated in sca-core

These exist today as Cargo features inside `sca-core`. Promotion to standalone crates is a **refactor, not new work**; do it when one of them needs its own tests/benchmarks or independent versioning.

| Crate | Cargo feature today | Role |
|---|---|---|
| `said-doc` | `docs` | PDF / DOCX / PPTX / XLSX / RTF / ODT / EPUB |
| `said-ocr` | `ocr` | scanned-image PDFs, images |
| `said-voice` | `whisper` | audio / video transcription |
| `said-code` | `code` | tree-sitter AST chunking for 7 languages |

All offline. All already shipped.

### Integration crates — offline-first, new work ahead

One crate per integration, feature-gated inside `said-mcp`'s Cargo.toml. All offline unless the `-live` suffix appears.

| Crate | Source | OAuth / network? | Inspiration |
|---|---|---|---|
| `said-watch` | filesystem events | No | original |
| `said-git` | local `.git` dir via `git2` | No | myNeutron-style developer memory |
| `said-mail-local` | Apple Mail emlx, mbox, eml, Outlook PST | No | [LEANN email_rag](#leann-precedent) |
| `said-browser-local` | Chrome/Edge/Brave/Firefox/Safari history SQLite | No | [LEANN browser_rag](#leann-precedent) |
| `said-chat-local` | iMessage chat.db, WhatsApp/Signal/Telegram exports, Slack/Discord/Teams exports | No | [LEANN imessage_rag, slack_rag](#leann-precedent) |
| `said-obsidian` | Obsidian vault on disk | No | Mem.ai / notes use case |
| `said-gmail-live` | Gmail IMAP + OAuth2 | **Yes** — Q3 | the OAuth pilot |
| `said-slack-live` | Slack Web API + OAuth | **Yes** — Q4 | follow-on after Gmail pattern lands |
| `said-notion-live` | Notion API + OAuth | **Yes** — Q4 | same |
| `said-jira-live` | Atlassian REST + OAuth 2.0 3LO | **Yes** — Q4+ | same |

Rule-of-thumb: if the user can already produce a local snapshot (Google Takeout, Slack admin export, Notion export zip), **ship the local reader first** and put the live-API variant behind a `-live` suffix for a later milestone.

### `said-think` — the one online component

Separate process. MCP client to `said-mcp`. Shipped as its own binary, installable separately (`cargo install said-think` or prebuilt binary). Not part of the default SAID install.

Responsibilities, three opt-in (all strictly off-by-default, all routed through the user's configured LLM provider via `said-llm`):

- **Dream v3** — content distillation. Reads N recent Episodic frames via MCP, calls the user's configured LLM (Anthropic / OpenAI / Ollama / local), writes a distilled Semantic rollup with `source_frames:` back-pointers. Per the BYO-LLM contract, the LLM call lives in `said-think`, not in `said-mcp`.
- **KG build** — typed `(subject, relation, object)` triple extraction via LLM, persisted as `kg_edge:true` frames. Complements [14.7 Layer-6 graph](14-novel-mechanisms/14.7-layer6-graph.md) by giving exhaustive-enumeration queries a proper edge index.
- **Phase Q rerank** — per-query LLM re-scoring of the top-50 retrieval candidates down to a tight top-10. Lifts LongMemEval R@10 from 98.72 % → 99.79 % when wired in. Today it lives as `crates/sca-core/examples/phaseq_rerank.rs` (offline benchmark binary that already consumes `said-llm`); production wiring is a CLI flag (`said ask --rerank`) and an MCP-tool boolean (`rerank: true`). Phase Q also has a query-rewrite mode (see `phaseq_validate.rs`) that recovers R@50 misses by asking the LLM to rephrase a failed query — same `said-llm` channel, separate trigger.

**Why separate process specifically:**

1. The BYO-LLM audit story stays clean. Run `ls`, see whether `said-think` is installed, get a definitive yes/no on "does this SAID install call an LLM?"
2. LLM calls are 500-5000 ms per request. The per-call IPC overhead of MCP (< 5 ms) is lost in the noise. Speed is not an argument against the separation here.
3. Licensing can diverge. `said-mcp` stays MIT. `said-think` may become MIT + commercial support (enterprise LLM-integration tier) without affecting the core.

**It is not a new plugin system.** It is one specific program that happens to be an MCP client. If we ever need a second LLM agent (e.g. `said-classify`, `said-tag`), it follows the same pattern — but each is a first-class, named, shipping binary, not a discovered plugin.

### MCP prompts — `onboard` + `answerer`

`said-mcp` exposes two first-class prompts via the MCP `prompts/list` and `prompts/get` operations. MCP clients (Claude Desktop, Cursor, etc.) surface these in their UI menus so users can invoke them directly without typing.

| name | source | purpose |
| --- | --- | --- |
| `onboard` | inline in `said-mcp` handler | guided first-contact setup (pick brain name, ingest, explore) |
| `answerer` | `said-prompts` crate | canonical .said agent system prompt — Anthropic-aligned, single source of truth |

The `answerer` prompt is sourced from the [`said-prompts`](../../crates/said-prompts/) Rust crate, which is also consumed by the WASM browser agent. **Same prompt text reaches every caller** — MCP clients and the browser admin panel get byte-identical instructions. See [3.10 Prompt architecture](03-core-subsystems/3.10-prompt-architecture.md).

### WASM addendum — how the three opt-in LLM responsibilities map to the browser

`said-think` exists because CLI / desktop / server installs have a process model: a separate binary that the user installed deliberately, runs in the background, and shows up under `ls` for the audit story.

The browser has no separate process. Everything happens inside the WASM tab. The audit story still works (look at the URL, look at the DevTools network panel — see whether requests go anywhere) but the architecture for **where each LLM call physically executes** has to be rethought per-responsibility because the call patterns are different:

| Responsibility | Trigger | Frequency | Latency tolerance | Brain mutation? |
| --- | --- | --- | --- | --- |
| **Dream v3** | Background after N queries / on idle / nightly | Rare (once per N hundred queries) | High — async | Writes Semantic frames |
| **KG build** | Per-document at ingest time | One-time per doc | Medium — during ingest | Writes `kg_edge:true` frames |
| **Phase Q rerank** | Per-query, blocks the answer | Every opt-in query | Low — user is waiting | Read-only |

**Shared infrastructure (browser):** the WASM agent already has `llmConfig` (Groq / Anthropic / OpenRouter / Ollama / local). All three opt-in features re-use that same provider rather than asking the user to configure a second one. Settings panel adds three checkboxes — **Phase Q rerank**, **Dream v3 consolidation**, **KG build at ingest** — all default OFF.

**Phase Q rerank (browser):**

- The interactive WASM agent loop **already does an implicit rerank** — every iteration, the model picks one candidate from `ask_fused`'s top-N and `read`s it. Multi-turn read-back is rerank by another name.
- Explicit Phase Q rerank in WASM only adds value when the query is genuinely ambiguous and the agent would otherwise iterate twice or pick wrong on the first read. Default fire condition: top-1 vs top-3 confidence gap < 0.1 OR query ≥ 20 words.
- When it fires, surface a separate trace step: `🧠 LLM rerank · 1.2s` so the user sees the extra cost.
- Headless single-shot consumers (CLI `said ask --rerank`, MCP `ask` with `rerank: true`) get the full LongMemEval-grade benefit because they don't have an agent loop to compensate.

**Dream v3 (browser):**

- Browser sessions are transient. Auto-firing Dream silently writes Semantic frames the user never asked for, then the tab closes — bad UX, breaks user trust.
- Ship as **explicit user action** only: a "Consolidate into Semantic memory" button in the brain panel. Fires Dream v3 on demand using the configured LLM, with progress UI.
- Dream writes go to `autoDirty` (silent dirty flag, included in any "Save .said" the user triggers) — never `userDirty` (no banner / no pulse).
- Future option: idle-hint when the user has had ≥ N salient turns in a session — toast "Want me to distill these N turns into Semantic memory?" with a single accept/dismiss. Still explicit consent.

**KG build (browser):**

- Triggered only at ingest time. Browser already has an ingest panel (drag-and-drop folder / files).
- Setting: "Extract knowledge graph during ingest (slower, uses LLM)" — default OFF.
- When on, ingest progress shows: "Extracting entities: 12/47 docs..." with a cancel button.
- Each LLM call is small (per-doc triple extraction); whole batch is bounded by document count and visible to the user.

**The decision pattern, codified:**

1. If the responsibility is **interactive and per-query** (Phase Q rerank), check whether the agent loop already does the equivalent work. In WASM it does — so explicit rerank is opt-in for ambiguous queries only, and the headline win is for headless consumers.
2. If the responsibility is **brain-mutating and async** (Dream v3), require explicit user action in transient environments. Don't background-write.
3. If the responsibility is **bounded and at-source** (KG build at ingest), gate it on a per-ingest checkbox with progress UI.

**The audit story in WASM:** all three features stay BYO-LLM. The user's configured provider is the only place LLM calls go. DevTools network panel shows every request. No `.said` engine code calls an LLM independently — every call originates from a feature the user explicitly toggled on.

## The integration template

Every new `said-*` integration crate implements the same six pieces. The first offline integration (`said-watch`) sets the template; later ones stamp from it.

### 1. Source reader
Read raw data from the local source. For `said-mail-local`, this is the emlx / mbox parser. For `said-browser-local`, this is the SQLite query against `History`. For `said-chat-local`, same shape, different schema.

### 2. Normalizer
Convert source-native records into SAID's frame-shaped JSON: `{doc_id, title, content, tags, pillar, created_at}`. This is where source-specific decisions get made — "how do I turn a Slack block into a title?", "what timestamp do I use for an email?" — and where the majority of per-integration code lives.

### 3. Incremental sync
For sources that grow (mail, history, chat), maintain a checkpoint so re-ingest skips already-seen records. For static sources (a git repo at a given SHA, an Obsidian vault snapshot), the checkpoint is trivial.

### 4. Pillar + tag writer
Decide pillar (Episodic for events, Semantic for notes, Code for source files, External for URL-pointered items) and tag shape (`source:<integration>`, `speaker:<sender>`, `thread:<id>`, etc.). Write to `sca-core` via the already-audit-logged `remember_with_pillar()` API.

### 5. Frame deduplication
SAID's content-addressable dedup handles byte-exact duplicates. The integration's job is to **produce stable `doc_id`s** so re-running `said-mail-local` against the same mbox doesn't create Nth copies of every message.

### 6. CLI / MCP surface
Each integration exposes one CLI subcommand (`said mail ingest ~/Library/Mail`) and/or one MCP tool (`ingest_mail`). Usually both.

These six pieces are what a new contributor has to build. Everything else — fingerprinting, retrieval, ranking, dedup, audit, lineage, pillar plumbing — lives in `sca-core` and is already done.

## LEANN precedent

[LEANN](../../research/LEANN/) (MIT-licensed, Python) has shipped exactly this offline-first integration playbook. Their `apps/` directory is directly usable prior art for our integration crates — not something we have to invent.

What we can port (MIT-licensed, attribution-preserving ports):

| LEANN file | SAID crate |
|---|---|
| [`apps/email_data/LEANN_email_reader.py`](../../research/LEANN/apps/email_data/) + [`apps/email_rag.py`](../../research/LEANN/apps/email_rag.py) | `said-mail-local` — emlx reader, mbox parser, Apple Mail auto-detection |
| [`apps/history_data/history.py`](../../research/LEANN/apps/history_data/) + [`apps/browser_rag.py`](../../research/LEANN/apps/browser_rag.py) | `said-browser-local` — Chrome SQLite reader, cross-OS profile detection |
| [`apps/imessage_rag.py`](../../research/LEANN/apps/imessage_rag.py) + [`apps/imessage_data/`](../../research/LEANN/apps/imessage_data/) | `said-chat-local` — iMessage chat.db reader |
| [`apps/slack_rag.py`](../../research/LEANN/apps/slack_rag.py) + [`apps/slack_data/`](../../research/LEANN/apps/slack_data/) | `said-chat-local` — Slack export parser |
| [`apps/code_rag.py`](../../research/LEANN/apps/code_rag.py) | `said-code` — repo-wide AST indexing polish |
| [`apps/document_rag.py`](../../research/LEANN/apps/document_rag.py) | `said-doc` — already shipped; LEANN has minor patterns to borrow |

We port the *patterns*, not the code. Python → Rust, SAID frames instead of LEANN's vector-store records, `sca-core` retrieval instead of LEANN's FAISS. But the integration list, the offline-first stance, and the local-file-reader approach are all validated by LEANN's shipping product. It's strong precedent.

**We also do things LEANN does not:**
- 64-bit fingerprints for retrieval (LEANN uses dense vectors via FAISS)
- Four-pillar memory model with typed routing (LEANN flat)
- BYO-LLM at query time (LEANN can call LLMs in-process for some RAG paths)
- Single-file portable brain (LEANN stores an index + a vector DB separately)

So LEANN is a reference point for *integration surface area*, not for *core architecture*. We own the core; we borrow the integration playbook.

## What about Composio / Dume?

Composio (and the products built on top of it like Dume.ai) use a fundamentally different model: **cloud-routed integration aggregation**. User data transits Composio's servers on the way from Gmail/Slack/Notion to the user's app.

This is incompatible with SAID's offline-first / local-first / single-file-portable positioning. We are not ruling it out philosophically — users can still compose SAID with Composio via their own glue scripts or custom Skill-style recipes — but **we do not ship a first-party Composio adapter**. Shipping one would make "SAID is offline" a marketing fiction whenever that adapter is enabled.

The competitive positioning is two products for two audiences:

| If the user prioritises… | …they pick |
|---|---|
| Offline, local, private, single-file | SAID |
| Cloud-mediated breadth, 500+ apps out of the box | Dume (via Composio) |

We don't try to be both. See the [competitor-matrix](10-benchmarks/competitor-matrix.md) for full positioning.

## Realistic roadmap

### Q2 — offline-first push (now → 3 months)

Six integrations, all offline, no OAuth work. Pattern:

1. `said-watch` — ~1 week
2. `said-git` — ~1 week
3. `said-mail-local` — ~1.5 weeks (port from LEANN email_reader)
4. `said-browser-local` — ~1 week (port from LEANN history.py)
5. `said-chat-local` — ~1.5 weeks (port from LEANN imessage + slack readers)
6. `said-obsidian` — ~3-5 days

Total: ~6 weeks of focused work. All shipping, all demoable, all offline.

### Q3 — OAuth pilot

`said-gmail-live` — 2-3 weeks. First live-API integration. Proves the OAuth pattern, the token-refresh loop, the incremental-sync checkpoint model. Everything after is stamped from this.

Stretch: `said-slack-live` (~1.5 weeks once OAuth pattern is proven).

### Q4 — live integrations + `said-think`

- `said-notion-live` — ~1.5 weeks
- `said-jira-live` — ~1.5 weeks
- `said-github-live` — ~1 week (simpler auth model)
- **`said-think` v1** — the LLM agent. Dream v3 + KG build. ~3-4 weeks including the audit story.

### Year-one target

**12-15 shipped integrations + `said-think`.** This is honest scope. Compare with Dume's "50" (which is Composio's catalog re-skinned) or LEANN's ~15 (who validated the pattern). We match LEANN on breadth, exceed them on core architecture (latent space, pillars, single-file), differ from Dume on privacy/offline.

## The extension escape hatch — skills

For integrations we will never build — internal corporate tools, weird SaaS, research-specific scripts — the escape hatch is a **`.said/skills/` folder with SKILL.md instructions**. This is not a plugin system; it's a markdown file that tells an agent (Claude Code, an automation bot, the user themselves) how to use SAID's MCP tools to compose a custom workflow.

```
.said/skills/my-corp-tool/
└── SKILL.md
```

```markdown
---
name: my-corp-tool-ingest
description: Ingest our internal ticketing system's export into SAID.
---

# Instructions

1. Run `scripts/fetch-tickets.py` which produces JSONL to stdout.
2. For each line, call the MCP `remember` tool with:
   - title = the ticket summary
   - content = the ticket body + comments
   - tags = ["source:internal-tickets", "ticket:<id>"]
   - pillar = "episodic"
3. Report count.
```

**Importantly, SAID itself doesn't need a plugin discovery system for this.** The skills folder is read by whatever agent is using SAID (Claude Code, an IDE, a custom script). They discover and execute the skill; SAID just receives regular MCP tool calls. Zero new code for us.

The Anthropic Skills model is well-documented and proven — it's exactly the model Claude Code uses. Borrowing the pattern costs us nothing and gives users an unlimited extension surface for the long tail we'll never build ourselves.

## See also

- [01-overview/what-is-said.md](01-overview/what-is-said.md) — the offline / BYO-LLM / single-file thesis these rules derive from
- [10-benchmarks/competitor-matrix.md](10-benchmarks/competitor-matrix.md) — positioning vs Dume / Composio / LEANN / Mem0 / Cognee
- [12-roadmap.md](12-roadmap.md) — Q2/Q3/Q4 integration milestones as tickets
- [09-cargo-features.md](09-cargo-features.md) — the feature-gate mechanism every integration crate opts into
- [`research/LEANN/apps/`](../../research/LEANN/apps/) — the offline-first integration playbook we're porting from

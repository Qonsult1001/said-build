# Personal-data import (`import` — browser / email / chat exports)

**The free-tier "bring your own data" capability.** A distinct `said import <source>` verb that pulls the
user's OWN personal data — browser history, email, exported AI chats — into a `.said` brain as
**recall-by-meaning memories**. This is the world-class free hook (doc
[42](../42-world-class-launch-requirements.md)): it fixes cold-start and capture-friction at once,
because the brain arrives pre-populated from data the user already has, **fully offline, no account**.

## The load-bearing distinction: `import` ≠ `init`/`ingest`

This is deliberate and must not blur (we spent real effort making the memory-vs-code line honest — doc
[40](../40-build-tier-capability-matrix.md)):

| Verb | Reads | Stores | Pillar | Tier | Purpose |
|---|---|---|---|---|---|
| **`import` (this plugin)** | the user's OWN live/exported personal data (browser DB, mbox, chat export) | **external pointers** (URI + title) or distilled text — the source stays the source of truth | **External** (browser/mail) / Episodic (chats) | **FREE brain** | *a memory feature* — recall your life by meaning |
| `init` / `ingest` | a folder of **source code / documents** | embedded, AST-chunked content | Code / Semantic | **coding / full** | *code intelligence* — symbol/AST search over a repo |

`import` is a **memory** feature (personal recall over your own data); `init` is a **code** feature
(searchable source). Different verb, different pillar, different tier — they never collide, and a user is
never told the free brain "indexes code" (it doesn't). The word **`import`** (not `ingest`) is chosen so
`ingest`/`init` stay unambiguously the code-tier tools.

## Sources

| Command | Reads (locally, offline) | Store model | Status |
|---|---|---|---|
| `said import browser` | Chrome/Edge/Brave `History` SQLite (auto-find profiles) | External pointer per page (`external:uri=<url>` + title) | **reader built** (`sca-core::browser_ingest`); needs `import` command + `brain`-bundle wiring |
| `said import chatgpt <export>` | ChatGPT `chat.html` / export zip | Episodic memory per conversation (distilled) | port from LEANN `chatgpt_reader` (HTML/zip parse) |
| `said import claude <export>` | Claude `conversations.json` | Episodic memory per conversation | port from LEANN `claude_reader` (JSON parse — simpler) |
| `said import email <mbox/emlx>` | local mbox / Apple Mail emlx | External pointer or distilled per message | port from LEANN `MboxReader` (Rust `mail-parser`) |

**Prior art (MIT, local-only):** [`local-research/LEANN/apps/`](../../../local-research/LEANN/apps/) —
`browser_rag` / `chatgpt_data` / `claude_data` / `email_data`. Our `browser_ingest.rs` is a Rust port of
`browser_rag`'s reader; the others follow the same pattern. (The `local-research/` clone is a temporary
reference, gitignored — delete after the ports land.)

## Dynamic, idempotent — NOT a one-shot `--max` sample

`import` is **re-runnable and self-deduplicating**: each entry gets a **stable doc_id derived from its
source key** (the URL for browser; message-id for mail; conversation-id for chats), so re-running
`import` UPDATES existing entries and adds new ones — a **re-sync**, not a pile-up. There is no capped
"sample" mode; the design is "pull my latest, whenever." Optional filters (`--since <date>`,
`--min-visits N`) narrow the pull without changing the idempotent contract. (The existing
`browser_ingest` already implements URL-keyed dedup and a recency-ordered read — see
[`browser_ingest.rs`](../../../crates/sca-core/src/browser_ingest.rs).)

## Why external pointers (browser/mail)

Browser history and mail are **live sources** — the browser DB and mailbox keep changing. We store an
`external:uri` pointer + a searchable title, not a copy of the page/message body. Recall finds the entry
by meaning; the agent then opens the live URL / message. This is LEANN's live-data model (arXiv
2506.08276) and works even in **Enterprise pointer-mode** brains (no content embedded). Chat *exports*
are static, so those distill to Episodic memories.

## Feature + bundle wiring (to decide at build time)

- The reader dep is the existing `browser = ["dep:rusqlite"]` feature in `sca-core`; email adds a
  mail-parser dep, chats add an HTML/JSON parser.
- **Free-tier plan:** wire these into the **`brain`** bundle (they're memory features), NOT gated behind
  `code`. Keep the code-tier `init`/`ingest` exactly where they are. This is the one place a *specific,
  first-party importer of the user's own data* belongs in the free build — distinct from general bulk
  ingest.
- Privacy/consent: `import browser`/`email` read sensitive local data. The command must state plainly
  what it reads and that it stays on-device (offline, no network), and default to the pointer store.

## Privacy & safety rules

- **Read-only on the live source.** `browser_ingest` opens the History DB `mode=ro&immutable=1` so it
  never disturbs the browser. Mail is read-only. Never move/delete the source.
- **Local + offline.** No network call at import time (the binary never calls an LLM either).
- **User-driven.** `import` runs when the user asks — no background watching in the free build (passive
  `said-watch` is a separate, opt-in roadmap item, doc [12](../12-roadmap.md)).

## See also

- [42 — world-class launch requirements](../42-world-class-launch-requirements.md) — why this is the free hook.
- [40 — build-tier capability matrix](../40-build-tier-capability-matrix.md) — the memory-vs-code boundary this respects.
- [13 — integrations](../13-integrations.md) — the broader `said-browser-local` / `said-mail-local` roadmap.
- [`crates/sca-core/src/browser_ingest.rs`](../../../crates/sca-core/src/browser_ingest.rs) — the existing reader.

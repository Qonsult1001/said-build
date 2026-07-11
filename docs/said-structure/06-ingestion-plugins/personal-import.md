# Personal-data import (`import` — browser / email / chat exports)

**The free-tier "bring your own data" capability.** A distinct `said import <source>` verb that pulls the
user's OWN personal data — browser history, email, exported AI chats — into a `.said` brain as
**recall-by-meaning memories**. This is the world-class free hook (doc
[42](../42-world-class-launch-requirements.md)): it fixes cold-start and capture-friction at once,
because the brain arrives pre-populated from data the user already has, **fully offline, no account**.

**Agent-native, not just CLI.** The same capability is exposed as the MCP **`import`** tool (brain +
`full` bundles, `feature = "browser"`), so a connected agent can trigger it from natural language — "import
my browsing history", "import my email", "import my ChatGPT conversations", or even a bare "what was the
last website I visited?" (the agent imports, then answers). It mirrors the CLI verb across **all four
sources** via a `source` param: `browser` (default), `email` (`mail` path), `chatgpt`/`claude` (`export`
path) — auto-detect/read-only/deduped/recency-tagged as each source warrants. A coding/coding-plus build
(no `browser` feature) does NOT advertise `import` — it's a memory tool, absent from the code tiers by
design.

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

## Sources — and an HONEST friction split

Not all sources are equally frictionless, and we must not over-claim. **Browser is the zero-friction
wow** (data is a local file we auto-read); **chat imports are high-value but higher-friction** (the data
lives on the provider's servers, so the user must request an export first).

| Command | Where the data lives | Friction | Store model | Status |
|---|---|---|---|---|
| `said import browser` | **local disk** (Chrome/Edge/Brave/Opera/Vivaldi `History` SQLite) — auto-detected | **zero** — just run it | External pointer per page (`external:uri` + title + `ingest:browser`/`domain:`/`visits:`/`typed:`/`profile:`/`visited:<date>`/`visited_at:<epoch>`/`recency:<rank>` tags) | **SHIPPED** — auto-detects all Chromium browsers + profiles; GLOBAL recency across profiles (see below) |
| `said import chatgpt <export>` | **OpenAI's servers** — user exports first | **higher** — request export → wait for email → unzip → point at it | Episodic memory per conversation (title + transcript) | **SHIPPED** — parses `conversations.json` (JSON, not HTML) |
| `said import claude <export>` | **Anthropic's servers** — user exports first | **higher** — same export dance | Episodic memory per conversation | **SHIPPED** — parses `conversations.json` |
| `said import email <mbox/emlx>` | **local** mail files (Thunderbird `.mbox`; Apple Mail `.emlx`; **Gmail via Google Takeout**; **Outlook/M365 via export** — both export to `.mbox`) | medium — user points at the mbox/emlx (chat-style export for Gmail/M365) | Distilled Episodic memory per message (subject + from + body), tagged `ingest:email`/`source:mbox`\|`source:emlx`/`from:`/`date:<yyyy-mm-dd>`/`sent_at:<epoch>`/`recency:<rank>` | **SHIPPED (local files)** — dependency-free RFC-822 reader; deduped by Message-ID, global recency by `sent_at:`. Live Gmail/M365 API sync (OAuth) is a **separate, later** feature (see below) |

**The export step (chat sources).** ChatGPT/Claude keep conversations on their servers, so the user
requests a data export (ChatGPT → Settings → Data Controls → Export data; Claude → Settings → Privacy →
Export data), gets an emailed link, downloads + **unzips** it, and points `import` at that folder's
`conversations.json`. We only ever read the **local unzipped file** — offline, no account, no API. The
"export not found" error walks the user through this so they're never stuck.

**Positioning consequence:** lead the free-tier story with **`import browser`** (the instant, offline,
"ask your web history anything" wow no competitor ships free). Present `import chatgpt/claude` as the
one-time "seed my brain with everything I've discussed with AI" — powerful, but honestly a fetch-first
step, not frictionless.

## Global recency across profiles / accounts (the "last site I visited" guarantee)

Temporal recall ("what was the **last** website I visited?", "my **most recent** email") must return the
newest item **by wall-clock time, across every profile and every import** — never a stale page from an
old profile just because it was imported last. Two design points make this correct:

- **Every item carries an ABSOLUTE timestamp tag** — `visited_at:<unix_secs>` (browser) / `sent_at:<unix_secs>`
  (email). This is the sort key. `ask` ranks temporal queries by it **descending across all sources**, so a
  page you opened yesterday in Chrome/Profile 1 always beats a three-year-old page in Chrome/Profile 3.
- **`recency:<rank>` is per-import display only, NOT the global key.** Each import restarts the ordinal at 1
  (its own newest item), so three profiles each have a `recency:1`. Ranking by that ordinal was the bug —
  it let a stale profile's `recency:1` tie the truly-newest item. The absolute `visited_at:`/`sent_at:` tag
  is globally comparable and fixes it. (Regression-tested: `global_recency_across_profiles` /
  `global_recency_across_mailboxes` import an OLD source AFTER a NEW one and assert the newest still wins.)
- **`profile:<browser>/<name>`** (e.g. `profile:Chrome/Profile 1`) is stamped on each browser page so recall
  can be **scoped** to one profile when asked ("last site in my work profile"), while the default temporal
  query stays global. Email keeps `source:` + `from:` for the same scoping role.

This is the multi-instance answer: user-1 / user-2 / user-3 profiles all feed one brain, and "last site I
visited" serves the globally-latest, not a per-profile latest.

## Email: local files (shipped) vs live Gmail/M365 (auth, separate)

Email splits along **exactly the same "is the data a local file or behind a login?" line** — and the
answer decides which surface it belongs on:

- **Local mail files → `import email`, shipped, zero-auth, offline, headless.** A `.mbox` mailbox or an
  Apple Mail `.emlx` folder is just a file on disk. This is the direct analogue of `import browser`.
  Critically, **Gmail and Outlook/M365 both fit here** the moment the user does a one-time *export*:
  - **Gmail** → Google Takeout → *Mail* → download the `.mbox`.
  - **Outlook / M365** → export / "save as" to `.mbox` (or a folder of `.eml`/`.emlx`).
  So the free tier can honestly say "import your Gmail/Outlook" — via the export file, with **no account
  access at all**. `import email` never opens a socket to a mail server.

- **Live server mailboxes → a separate OAuth connector, NOT in `import email`.** Reading a *live* Gmail
  or M365 inbox (no export step) means the **Gmail API** (`users.messages`, OAuth against Google, GCP
  client id, `gmail.readonly`) or the **Microsoft Graph API** (`/me/messages`, OAuth against Entra/Azure
  AD, `Mail.Read`). Neither is a file; both need an interactive browser login + token refresh.

**Where the auth lives — the WASM interface, by design.** OAuth needs a browser redirect and a place to
hold + refresh a token; a headless CLI/MCP binary can't run that flow (the same reason the CLI refuses
interactive connector auth). So the live-mail connectors split cleanly:

| Layer | Job |
|---|---|
| **WASM web interface** | runs the OAuth login (Google/Microsoft), holds + refreshes the token, hands **only the access token** down |
| **sca-core connector** | given a token, calls Gmail/Graph, pulls messages, writes them into the `.said` brain with the SAME pillar/tag/recency model as `import email` |
| **CLI / MCP `import`** | **local** sources only (`.mbox`/`.emlx`) — no token, fully offline & headless |

This keeps the free-tier promise intact (local mail is offline, no account) while the authenticated
live-sync layers on top **only** in the WASM surface — it never pretends to work headless. Live
Gmail/M365 sync is therefore a distinct roadmap item, not part of the shipped `import email`.

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

# MCP tool: import

Pull the user's OWN personal data — browser history, a local mail file, or an AI-chat export — into the
brain as recall-by-meaning memories. The agent-native twin of CLI
[`said import`](../07-cli-reference/import.md) (browser / email / chatgpt / claude). A **memory**
feature: it does NOT index code.

**Bundle availability.** Advertised only when the server is built with the `browser` feature — the
**brain** and **full** bundles. A **coding / coding-plus** build does NOT list `import` in `tools/list`
(it's a memory tool, absent from the code tiers by design). See
[40 — build-tier capability matrix](../40-build-tier-capability-matrix.md).

## Schema

```json
{
  "name": "import",
  "arguments": {
    "source": "string, optional — \"browser\" (default) | \"email\" | \"chatgpt\" | \"claude\"",
    "since_days": "integer, optional — [browser] only pages visited within N days (0/omitted = all)",
    "min_visits": "integer, optional — [browser] only pages visited ≥ N times (default 1)",
    "max": "integer, optional — cap items imported (0/omitted = no cap; newest first). browser/email.",
    "db": "string, optional — [browser] import ONE specific History SQLite instead of auto-detecting all",
    "mail": "string, optional — [email] path to a .mbox file OR a folder of Apple Mail .emlx (required when source=email)",
    "export": "string, optional — [chatgpt/claude] the unzipped export folder or its conversations.json (required when source=chatgpt/claude)",
    "max_chars": "integer, optional — [email/chatgpt/claude] cap each message/transcript (default 20000; 0 = no cap)"
  }
}
```

## Description

> Import the user's OWN personal data into the brain as recall-by-meaning memories.
> `source="browser"` (default) auto-detects EVERY installed Chromium browser + profile (Chrome, Edge,
> Brave, Opera, Vivaldi) and imports them all — zero config, fully offline, the live browser DB opened
> READ-ONLY. `source="email"` imports a LOCAL mail file (`.mbox` or Apple Mail `.emlx`, passed as
> `mail`) — offline, no login; a `.mbox` covers Thunderbird, Gmail (via Google Takeout → Mail) and
> Outlook/M365 (via export), and NEVER logs into a mail account. Re-run anytime to re-sync (deduped).
> A MEMORY feature — it does NOT index code (that is the coding build's `ingest`/`init`).

Trigger phrases: "import my browsing history", "import my email", "what was the last website I
visited?" (the agent imports, then answers from the freshest item).

## Behavior

### `source="browser"` (default)

1. Auto-detect every Chromium browser + profile (or read `db` if given), open the `History` SQLite
   **read-only** (`mode=ro&immutable=1`)
2. Each page → an **External-pointer** memory (title + live URL — the browser stays source of truth)
3. Deduped by URL; `build_index()` + `save()`

**Tags per page:** `ingest:browser`, `visits:<n>`, `domain:<host>`, `typed:<n>` (if typed),
`profile:<Browser>/<Profile>` (e.g. `profile:Chrome/Profile 1`), `visited:<yyyy-mm-dd>`,
`visited_at:<unix_secs>` (absolute epoch — the temporal sort key), `recency:<rank>` (per-import display
ordinal only).

### `source="email"`

1. Read the `.mbox` mailbox or Apple Mail `.emlx` folder at `mail` — **offline, no login, headless**
2. Each message → a distilled **Episodic** memory (subject + from + body); deduped by `Message-ID`
3. `build_index()` + `save()`

**Tags per message:** `ingest:email`, `source:mbox`|`source:emlx`, `from:<addr>`, `date:<yyyy-mm-dd>`,
`sent_at:<unix_secs>` (absolute epoch — the temporal sort key), `recency:<rank>` (per-import display
ordinal only).

> **Local files only — no live-mailbox sync here.** `import email` never opens a socket to a mail
> server. A `.mbox` fits Gmail/Outlook the moment the user does a one-time **export** (Google Takeout /
> M365 export). Reading a *live* Gmail/M365 inbox needs the Gmail API / Microsoft Graph over OAuth —
> a **separate, later** feature that lives in the WASM/web surface (which can run the OAuth redirect
> and hold the token), NOT in this offline binary. Don't claim live sync is shipped.

### `source="chatgpt"` / `source="claude"`

1. Read the AI-chat **data export** at `export` (the unzipped export folder or its `conversations.json`)
   — offline, no account access
2. Each conversation → a distilled **Episodic** memory (title + transcript); deduped by conversation id
3. `build_index()` + `save()`

**Tags per conversation:** `ingest:chatgpt`|`ingest:claude`, `source:chatgpt`|`source:claude`,
`date:<yyyy-mm-dd>` (when a timestamp is present).

> **Export first (the data lives on the provider's servers).** The user requests an export
> (ChatGPT → Settings → Data controls → Export data; Claude → Settings → Privacy → Export data),
> downloads and unzips it, then points `export` at that folder. We only ever read the **local unzipped
> file** — no API, no login. A missing/invalid `export` returns a clean `isError` result walking the
> user through the export steps.

## Global recency (the "last website / last email" guarantee)

Temporal queries rank by the **absolute** `visited_at:` / `sent_at:` tag, **descending across every
profile, account, and import** — so yesterday's page always beats a three-year-old page regardless of
import order. The per-import `recency:<rank>` is display-only (each import restarts it at 1, so it is NOT
globally comparable — ranking by it was a bug, now fixed). Temporal queries respect **source routing**:
"last website" → browser only; "last email" → email only; a bare "most recent" → both merged by
absolute time. The `profile:` tag enables profile-scoped recall ("last site in my work profile").

## Examples

```json
{"method":"tools/call","params":{"name":"import","arguments":{"source":"browser"}}}
```

Response:
```
✓ Imported 1284 pages from 3 browser profiles (Chrome, Edge).
These are searchable — ask "what was the last website I visited?" or filter with a domain: tag.
```

```json
{"method":"tools/call","params":{"name":"import","arguments":{
  "source":"browser","since_days":30,"min_visits":2
}}}
```

```json
{"method":"tools/call","params":{"name":"import","arguments":{
  "source":"email","mail":"/home/u/mail/archive.mbox","max":500
}}}
```

## Enterprise mode

Browser pages are **External pointers** (URI + title, no content embedded), so `import browser` works
even in Enterprise pointer-mode brains. Email distills message text into Episodic memories (content
embed) and follows the same Enterprise content-embed guard as `remember` / content-mode `ingest`.

## See also

- [CLI said import](../07-cli-reference/import.md) — the same surface via terminal (also has `chatgpt`/`claude`/`from`)
- [personal-import plugin](../06-ingestion-plugins/personal-import.md) — the full spec: tags, global recency, local-vs-live mail
- [40 — build-tier capability matrix](../40-build-tier-capability-matrix.md) — why `import` is brain + full only
- [ingest](ingest.md) / [init](init.md) — the code-tier ingest tools (distinct from `import`)

# said import

Bring data **into** a `.said` brain. Two families under one verb:

- **Personal-data import** (`browser` / `email` / `chatgpt` / `claude`) — pull the user's OWN data
  (browsing history, local mail, exported AI chats) into memories. A **memory** feature, gated behind
  the `browser` cargo feature — present in the **brain** and **full** bundles, absent from
  coding/coding-plus. See [personal-import plugin](../06-ingestion-plugins/personal-import.md) for the
  full spec.
- **Migration** (`from`) — import another memory tool's export (mem0, memvid) via a migration adapter.
  Always available (not feature-gated).

Distinct from the coding-tier [`init`](init.md) / [`ingest`](ingest.md), which index **source code**.

## Usage

```
# Personal data (brain / full bundles — feature = "browser")
said import browser  [--since-days N] [--min-visits N] [--max N] [--db <History>]
said import email    <MAIL> [--max N] [--max-chars N]
said import chatgpt  <EXPORT> [--max-chars N]
said import claude   <EXPORT> [--max-chars N]

# Migration adapters (all bundles)
said [--path FILE] import from --from <ADAPTER> --source <PATH>
said import from --list
```

The global `--path FILE` selects the target brain (before the subcommand); `--json` (global) switches
any of these to machine-readable output.

---

## `import browser` — browser history

Auto-detects **every** installed Chromium browser + profile on the machine (Chrome, Edge, Brave, Opera,
Vivaldi) and imports them all. Fully **offline**; the live `History` SQLite is opened **read-only**
(`mode=ro&immutable=1`) and never modified. Each page → an **External-pointer** memory (title + live
URL — the browser stays the source of truth). Re-run anytime to re-sync (deduped by URL).

**Flags**

- `--since-days N` — only pages last visited within N days (`0` = all history, default).
- `--min-visits N` — only pages visited at least N times (default `1` = everything titled).
- `--max N` — cap pages per profile (`0` = no cap, default); recency-ordered, so a cap keeps the newest.
- `--db <path>` — advanced: import ONE specific `History` file instead of auto-detecting all browsers.

**Tags per page:** `ingest:browser`, `visits:<n>`, `domain:<host>`, `typed:<n>` (if typed),
`profile:<Browser>/<Profile>` (e.g. `profile:Chrome/Profile 1`), `visited:<yyyy-mm-dd>`,
`visited_at:<unix_secs>` (absolute epoch — the temporal sort key), `recency:<rank>` (per-import
display ordinal only). Filter recall with `ask --tag domain:…` or `ask --tag profile:…`.

## `import email` — a LOCAL mail file (offline, zero-auth)

Imports mail that is **already a file on disk** — a `.mbox` mailbox OR an Apple Mail `.emlx` folder.
**OFFLINE, no login, headless.** A `.mbox` covers Thunderbird, **Gmail via Google Takeout → Mail**, and
**Outlook / M365 via export** — the user exports their mail to a file and we read it; this **never logs
into a mail account**. Each message → a distilled **Episodic** memory (subject + from + body). Deduped
by `Message-ID`. Re-import = re-sync.

> **Local files only.** `import email` does NOT talk to the Gmail API or Microsoft Graph. Reading a
> *live* mailbox (no export step) requires OAuth and is a **separate, later** feature that belongs in
> the WASM/web surface, not this offline binary — see
> [personal-import § local vs live](../06-ingestion-plugins/personal-import.md#email-local-files-shipped-vs-live-gmailm365-auth-separate).

**Positional:** `<MAIL>` — path to a `.mbox` file or a folder of `.emlx` files.
**Flags:** `--max N` (cap total messages, `0` = all, newest-first) · `--max-chars N` (cap each
message's stored text, default `20000`, `0` = no cap).

**Tags per message:** `ingest:email`, `source:mbox`|`source:emlx`, `from:<addr>`, `date:<yyyy-mm-dd>`,
`sent_at:<unix_secs>` (absolute epoch — the temporal sort key), `recency:<rank>` (per-import display
ordinal only).

## `import chatgpt` / `import claude` — AI-chat exports

Parse the `conversations.json` from a **data export** into **Episodic** memories (one per conversation:
title + transcript). Point at the unzipped export folder or its `conversations.json`. Re-import =
re-sync (deduped by conversation-id).

- **Export first:** ChatGPT → Settings → Data controls → Export data; Claude → Settings → Privacy →
  Export data. Download the emailed link and **unzip** it. We only ever read the local unzipped file —
  offline, no account, no API.

**Positional:** `<EXPORT>` — the export folder or `conversations.json`.
**Flags:** `--max-chars N` — cap each conversation's stored transcript (default `20000`, `0` = no cap).

## Global recency (the "last website / last email" guarantee)

Temporal queries ("what was the **last** website I visited?", "my **most recent** email") rank by the
**absolute timestamp tag** — `visited_at:` (browser) / `sent_at:` (email) — **descending across every
profile, account, and import**. So yesterday's page from Profile 1 beats a three-year-old page from
Profile 3, regardless of import order. The per-import `recency:<rank>` ordinal is **display-only** (each
import restarts it at 1, so it is NOT globally comparable — ranking by it was a bug, now fixed). Temporal
queries respect **source routing**: "last website" → browser only, "last email" → email only, a bare
"most recent" → both merged by absolute time. See
[personal-import § global recency](../06-ingestion-plugins/personal-import.md#global-recency-across-profiles--accounts-the-last-site-i-visited-guarantee).

---

## `import from` — migrate memories from competitor systems

Import another memory tool's export (mem0, memvid) into the brain.

### Arguments

- `--from <ADAPTER>` — source system; one of `memvid`, `mem0`
- `--source <PATH>` — path to the competitor's export file
- `--list` — print registered adapters and exit

### Behavior

1. Resolve the adapter via `sca_core::migrate::adapter_for(name)`
2. Open the target brain
3. Call `run_migration(adapter, source_path, &mut brain)`:
   - Adapter parses its format → `Vec<MigratedRecord>`
   - Driver maps each record to the right pillar (see per-adapter mapping below)
   - Writes each via `remember_with_pillar` (gets audit log, surprise detection, S_slow accumulation)
4. `brain.build_index()` + `brain.save()`

### Per-adapter expectations

#### memvid

Expects a JSON array of objects:

```json
[
  {"id": "a", "content": "Alice prefers dark mode", "timestamp": 1700000000,
   "metadata": {"user": "alice"}},
  ...
]
```

Mapping:
- Every record → `Pillar::Episodic`
- `doc_id = "memvid:<id>"`
- `timestamp` → `ingested_at:<unix>` tag
- `metadata.<key>` → `<key>:<value>` tag (string values only)

#### mem0

Expects JSONL (one object per line) from mem0's `export_memories()`:

```jsonl
{"id":"m1","memory":"User is vegetarian","user_id":"u42","categories":["preference"]}
{"id":"m2","memory":"Deploy sequence","categories":["procedure"]}
```

Mapping by category:
- `preference` (default) → Semantic
- `turn` / `dialog` / `conversation` → Episodic
- `plan` / `action` / `procedure` / `recipe` → Procedural
- `document` / `file` / `reference` → External
- other → Semantic

Tags: `user_id:<id>`, `ingested_at:<iso>`, `category:<name>` per record. Plus `imported_from:mem0`.

### Output

```
✓ Imported from memvid:
  read:    147
  written: 147
  pillars: episodic=147
```

mem0 mixed:
```
✓ Imported from mem0:
  read:    4217
  written: 4217
  pillars: semantic=2100, episodic=1820, procedural=140, external=157
```

JSON mode:

```json
{
  "source_system": "mem0",
  "records_read": 4217,
  "records_written": 4217,
  "records_skipped": 0,
  "per_pillar": {"semantic": 2100, "episodic": 1820, "procedural": 140, "external": 157},
  "errors": []
}
```

### Enterprise mode guard

Enterprise brains refuse content-bearing imports that aren't External-pillar. A mem0 dump with `turn` category rows gets rejected unless the target brain is Portable:

```
✗ skipped 'mem0:m3' — Enterprise brain refuses Episodic content embed
```

The `errors` field of the report lists per-record skip reasons so ops can see why the import didn't write everything.

---

## Examples

```bash
# --- Personal data (brain / full bundles) ---

# Import all browser history from every installed Chromium browser + profile
said import browser

# Only sites visited in the last 30 days, at least twice
said import browser --since-days 30 --min-visits 2

# Import a local mailbox (Thunderbird / Gmail-Takeout / Outlook-export .mbox)
said import email ~/mail/archive.mbox

# Import an Apple Mail folder of .emlx files, newest 500 only
said import email ~/Library/Mail/V10/INBOX.mbox --max 500

# Seed the brain from an AI-chat export (unzip it first)
said import chatgpt ~/Downloads/chatgpt-export/conversations.json
said import claude  ~/Downloads/claude-export/

# --- Migration adapters (all bundles) ---

# List adapters
said import from --list
# → Registered migration adapters:
#     - memvid
#     - mem0

# Import mem0 JSONL export into a fresh brain
said create migrated.said --mode portable
said --path migrated.said import from --from mem0 --source ~/memories.jsonl

# Import memvid into an existing brain (merges with current content)
said --path my-brain.said import from --from memvid --source ~/memvid-backup.json
```

## How to extend (migration adapters)

New adapter:
1. Create a struct implementing `MigrationAdapter` in [`crates/sca-core/src/migrate.rs`](../../../crates/sca-core/src/migrate.rs)
2. Register in `adapter_for(name)` + add to `registered_adapters()` list
3. Add unit test for the format mapping
4. Document in [Row 48 Migration adapters](../05-features/row-48-migration.md) and [Row 38 Migration spec](../05-features/row-38-migration-spec.md)

## Known limitations

- mem0 SQLite export not supported (users must run `export_memories()` to JSONL first)
- Zep / LangMem migration adapters not yet shipped
- Live Gmail / M365 **API** sync (no export step, OAuth) is a separate roadmap feature for the WASM surface — `import email` is local-file-only

## See also

- [personal-import plugin](../06-ingestion-plugins/personal-import.md) — the personal-data spec (browser/email/chat), tags, global recency, local-vs-live mail
- [MCP import tool](../08-mcp-reference/import.md) — the agent-native twin
- [Row 48 Migration adapters](../05-features/row-48-migration.md)
- [Row 38 Migration spec](../05-features/row-38-migration-spec.md)

# Command reference

Every `said` command you need for personal-memory use, in one place. Each command takes
`--path <file>.said` to pick which brain to use — or set a default once (see
[How to set a default brain file](how-to-set-a-default-brain.md)) and drop `--path`.

## The everyday commands

| Command | What it does | Example |
|---------|--------------|---------|
| `create <file>.said` | Make a new, empty brain file. | `said create my-brain.said` |
| `add "<note>" --id <name>` | Store a memory. `--id` is a short name you can use to fetch it later (optional — auto-named if omitted). `--tag ns:value` (repeatable) attaches browsable/filterable tags. | `said add "Wifi is sunflower-42" --id wifi --tag topic:home` |
| `ask "<question>"` | Find memories by meaning, in plain English. **The main command** — use it for almost everything. Add `--tag` to scope by facet; add `--deep` for broad synthesis. | `said ask "what's the wifi password"` |
| `get <id>` | Show one memory's exact text by its `--id`. | `said get wifi` |
| `delete <id>` | Remove a memory (recoverable from the recycle bin). | `said delete wifi` |
| `admin recycle-bin` | List deleted memories you can still recover. | `said admin recycle-bin` |
| `admin recover <id>` | Bring a deleted memory back. | `said admin recover wifi` |
| `history <id>` | Show a memory's past versions. | `said history bday` |
| `checkout <id> --version N` | Roll a memory back to an earlier version. | `said checkout bday --version 0` |
| `stats` | Show how many memories the brain holds. | `said stats` |
| `list-concepts [--prefix p]` | List the `[[wikilink]]` concepts your memories are linked to, with a count per concept. | `said list-concepts` |
| `list-tags [--prefix p]` | List the `tags` your memories carry (the metadata vocabulary), with a count per tag. Reuse these instead of inventing synonyms. | `said list-tags --prefix project:` |
| `use <file>.said` | Set the default brain so you can skip `--path`. | `said use my-brain.said` |
| `import <source>` | Bring in your own data or migrate from another tool. Sources below. | `said import browser` |

## Importing data (`import`)

`said import <source>` fills a brain from real sources. All imports are **offline and read-only** —
they read files already on your disk and never modify the originals. Re-running re-syncs (deduped).
See the walkthrough: [How to import your own data](how-to-import-your-own-data.md).

| Source | What it imports | Example |
|--------|-----------------|---------|
| `import browser` | Every installed Chromium browser + profile (Chrome, Edge, Brave, Opera, Vivaldi). Each page → a memory tagged `domain:<host>`. | `said import browser` |
| `import email <MAIL>` | A local `.mbox` file **or** an Apple Mail `.emlx` folder. Covers Thunderbird and Gmail/Outlook **via export** (Takeout / mbox export) — never logs in. Each message → a memory tagged `from:<addr>`, `date:<day>`. | `said import email "All mail.mbox"` |
| `import chatgpt <EXPORT>` | Your ChatGPT data export. Point at the unzipped folder or its `conversations.json`. | `said import chatgpt ./export/conversations.json` |
| `import claude <EXPORT>` | Your Claude data export. Point at the unzipped folder or its `conversations.json`. | `said import claude ./export/conversations.json` |
| `import from --from <tool> --source <path>` | Migrate from another memory tool (`mem0`, `memvid`). `--list` shows adapters. | `said import from --from mem0 --source ./export.jsonl` |

Import-specific flags (all imports also accept `--path` and `--json`):

- `import browser`: `--since-days N` (recent N days; `0` = all), `--min-visits N` (skip one-off pages),
  `--max N` (cap pages per profile, newest kept), `--db <path>` (one specific `History` DB — advanced).
- `import email`: `--max N` (cap messages, newest kept), `--max-chars N` (cap each message's text;
  default 20000).
- `import chatgpt` / `import claude`: `--max-chars N` (cap each transcript; default 20000).
- `import from`: `--from <tool>`, `--source <path>`, `--list` (show adapters and exit).

After importing, ask **temporal** questions — `said ask "what was the last website i visited?"` returns
the globally most-recent page across all browser profiles; `"what was the last email i received?"`
does the same for mail.

> **Live inbox sync is not shipped.** `import email` reads local files only. Syncing directly from a
> live Gmail / Microsoft 365 inbox (OAuth, no export) is a separate, future feature.

## How many answers does `ask` give back?

`ask` leads with the **best memory** for your question. If a few of your memories might fit, it shows
those too — so it never hides the right one just because it wasn't 100% sure. Seeing two or three results
means it's being careful, not confused; the top one is the best memory it found.

When you have dozens of memories and results tie across topics, **scope with tags** before asking:

    said ask "offline integrations" --tag quarter:Q2

Run `said list-tags` first to see facets already in use. Repeat `--tag` for AND logic (all tags must
match). See [How to find and organize memories](how-to-find-a-specific-memory.md).

On genuine ties (several close scores, no clear winner), `ask` prints a **close matches** footer listing
which tags distinguish the results — use `--tag` to narrow. When `#1` clearly wins, no footer is shown.

For **everything on a topic** (not just the top few), add `--deep`:

    said ask "payment system" --deep

Each result line looks like this:

    1. [0.55][semantic] wifi   tags: topic:home
        The wifi password is sunflower-42.

- The first number is a confidence score (higher = better match).
- `wifi` is the memory's id (use it with `get` or `delete`).
- `tags:` shows facets you can filter with `--tag`.
- The line below is the memory itself.

## Useful flags

- `--path <file>.said` — which brain to use (on any command). Skip it if you've run `use`.
- `--top <N>` on `ask` — ask for more or fewer results (default is plenty for everyday use).
- `--tag <TAG>` on `ask` — scope recall to memories carrying this tag (repeatable, AND logic). Use with
  `list-tags` when vague queries return too many ties.
- `--deep` on `ask` — return all relevant memories above the threshold (broad synthesis).
- `--tag <TAG>` on `add` — attach browsable/filterable tags (repeatable).
- `--id <name>` on `add` — give a memory a memorable name so `get`/`delete` can find it.

Run `said <command> --help` to see all options for any command.

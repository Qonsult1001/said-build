# How to import your own data (browser, email, chats)

> Goal: fill a `.said` brain with your *real* life — the web pages you've visited, the email you've
> received, your ChatGPT/Claude conversations — then ask it questions in plain English, including
> "what did I look at last?". This assumes you already have a brain file; if not, do the
> [tutorial](tutorial-your-first-brain.md) first.

Everything here runs **offline and read-only**. `said` reads files that are already on your disk (your
browser's history database, an exported mailbox). It never logs into an account and never changes the
originals. Re-running any import just re-syncs — duplicates are merged, nothing is doubled.

These steps assume you already have a brain (e.g. `my-brain.said`), or a default brain set with
`said use` so you can drop `--path`. Examples below keep `--path` explicit.

## Import your browser history

One command imports **every** Chromium browser and profile installed on this machine — Chrome, Edge,
Brave, Opera, Vivaldi — no need to name them:

    said --path my-brain.said import browser

You should see a summary like:

    ✓ Edge (Default) — 5 pages

    ✓ Imported 5 pages from 1 browser profile (4 empty/unused profiles skipped).
      Recall:         said ask "that article about X"
      Filter by site: said ask "..." --tag domain:github.com
      Re-run anytime to sync new history (deduped).

Each page becomes one memory (its title + URL), tagged with its site as `domain:<host>` and with how
many times you visited it. Now recall by **meaning** — you don't need the page's exact title:

    said --path my-brain.said ask "that article about the asteroid"

To narrow to a single site, add the domain tag:

    said --path my-brain.said ask "the repo I was looking at" --tag domain:github.com

Useful options (run `said import browser --help` for all):

- `--since-days N` — only import pages visited in the last N days (`0` = all history, the default).
- `--min-visits N` — skip pages you only hit once; import only pages visited at least N times.
- `--max N` — cap pages per profile. It keeps the **newest**, so `--max 500` imports your recent 500.
- `--db <path>` — advanced: import one specific `History` database file instead of auto-detecting.

- **If you see `Imported 0 pages`** → the browser may have been open with the database locked, or you
  filtered everything out with `--since-days`/`--min-visits`. Close the browser and try again, or widen
  the filter.

## Import your email

`said import email` reads a **local mail file** — a `.mbox` mailbox or an Apple Mail `.emlx` folder.
There is no login: you point it at a file on your disk.

1. Get your mail into a `.mbox` file (skip this if you already have one, e.g. Thunderbird):

   - **Gmail** → [Google Takeout](https://takeout.google.com/) → select **Mail** → download the archive
     → unzip it → the `.mbox` file is inside.
   - **Outlook / Microsoft 365** → export your mail to a `.mbox` file (use your mail client's
     export/backup option, or a tool that writes `.mbox`).
   - **Thunderbird** → your account's mail is already stored as `.mbox` files on disk.

2. Point `import email` at the file:

       said --path my-brain.said import email "./Takeout/Mail/All mail.mbox"

   You should see:

       ✓ Imported 1 message(s) from your mbox mail.
         Recall:        said ask "that email about X"
         By sender:     said ask "..." --tag from:alice@example.com
         Most recent:   said ask "what was the last email i received?"

   (For Apple Mail, pass the folder of `.emlx` files instead of an `.mbox` file.)

3. Recall by meaning, or filter by who sent it:

       said --path my-brain.said ask "the lunch plans"
       said --path my-brain.said ask "..." --tag from:alice@example.com

Each message becomes a memory (subject + sender + body), tagged `from:<addr>` and `date:<day>`.
Re-importing the same mailbox re-syncs it — messages are deduplicated by their Message-ID, so nothing
doubles up.

Options (run `said import email --help`):

- `--max N` — cap how many messages to import (newest first, so a cap keeps the most recent).
- `--max-chars N` — cap each message's stored text (default 20000; `0` = no cap).

> **Live inbox sync?** Not yet. `import email` is **local files only** — it never talks to a mail
> server. Syncing straight from a live Gmail or Microsoft 365 inbox (with OAuth login, no export step)
> is a separate feature that hasn't shipped. For now, export to a file and import that.

## Import your ChatGPT or Claude conversations

Both assistants let you export your full conversation history as a data archive containing a
`conversations.json`. Point `said` at the unzipped folder (or the JSON file directly):

    said --path my-brain.said import chatgpt "./chatgpt-export/conversations.json"
    said --path my-brain.said import claude  "./claude-export/conversations.json"

Each conversation becomes one searchable memory (its title + transcript). To export from ChatGPT:
**Settings → Data controls → Export data**, then unzip the emailed archive. Re-importing re-syncs
(deduped). Use `--max-chars N` to cap how much of each long transcript is stored.

## Ask your history a temporal question

Because every imported item is tagged with an absolute timestamp, you can ask **time** questions and
get the globally most-recent item — not a stale page from an old profile:

    said --path my-brain.said ask "what was the last website i visited?"

The top result is the single most recently visited page across **all** your browser profiles:

    Ask: "what was the last website i visited?"  (10 results in 26.71ms)

      1. [1.00][text] browser/fb49ce1d…   tags: domain:www.msn.com, visited:2026-07-03, recency:1
          Like 22 atomic bombs: The asteroid that will hit the Earth is called Bennu — www.msn.com

The same works for mail:

    said --path my-brain.said ask "what was the last email i received?"

"last website" routes to your browser memories and "last email" to your mail — each ranked by
wall-clock time, newest first.

## Result

Your real browsing, email, and chat history now live in one portable `.said` file, recallable by
meaning and by time. Copy that file to another machine and it answers the same questions — offline,
no re-import.

## See also

- Migrating memories from **another memory tool** (mem0, memvid) — different from personal import →
  [How to import memories from another tool](how-to-import-memories-from-another-tool.md)
- Adding notes by hand → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- Filtering recall with tags → [How to find and organize memories](how-to-find-a-specific-memory.md)
- Every command and flag → the [Command reference](cli-reference.md)

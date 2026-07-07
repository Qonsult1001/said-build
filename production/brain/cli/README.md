# said — CLI (terminal)

Use your portable brain from the command line: create a brain, store notes, and ask it questions —
you type the `said` commands yourself. No server, no cloud, fully offline.

## What's here

- **`said.exe`** — the brain CLI binary (self-contained; the encoder is embedded).
- **[`doc/`](doc/)** — all documentation:
  - **[`doc/how-to-install-said.md`](doc/how-to-install-said.md)** — download and set up `said` on
    Windows, macOS, or Linux. **Do this first.**
  - **[`doc/tutorial-your-first-brain.md`](doc/tutorial-your-first-brain.md)** — zero to first success:
    create a brain, store a few notes, ask it a question. Start here once installed.
  - **How-to guides** — one real task each:
    - [`doc/how-to-store-and-recall-notes.md`](doc/how-to-store-and-recall-notes.md) — the everyday save-and-ask loop
    - [`doc/how-to-find-a-specific-memory.md`](doc/how-to-find-a-specific-memory.md) — narrow a search to the exact memory
    - [`doc/how-to-set-a-default-brain.md`](doc/how-to-set-a-default-brain.md) — stop passing `--path` every time
    - [`doc/how-to-track-versions-of-a-memory.md`](doc/how-to-track-versions-of-a-memory.md) — update a fact, keep its history
    - [`doc/how-to-recover-a-deleted-memory.md`](doc/how-to-recover-a-deleted-memory.md) — undo a delete
    - [`doc/how-to-import-memories-from-another-tool.md`](doc/how-to-import-memories-from-another-tool.md) — bring memories in from mem0 / others
  - **[`doc/cli-reference.md`](doc/cli-reference.md)** — every command and flag, catalogued.

## 30-second start

```sh
# 1. create a brain file
said create my-brain.said

# 2. store a note
said --path my-brain.said add "My wifi password is sunflower-42"

# 3. ask it back
said --path my-brain.said ask "what is my wifi password"
```

The `ask` result lists the most relevant memories, best first — the answer you want is in that short
list. See [the tutorial](doc/tutorial-your-first-brain.md) for the full walkthrough and
[`doc/cli-reference.md`](doc/cli-reference.md) for everything else.

## Note on recall

`said` returns the **top handful** of relevant memories for a question, not a single guess — the right
memory is essentially always in that set. You read the top results and pick the one you need. (For a
narrower search, see [`doc/how-to-find-a-specific-memory.md`](doc/how-to-find-a-specific-memory.md).)

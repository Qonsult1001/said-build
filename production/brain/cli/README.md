# said — CLI (terminal) · v0.12.0

Use your portable brain from the command line: create a brain, store notes, and ask it questions —
you type the `said` commands yourself. No server, no cloud, fully offline.

Ships with **`said.exe`** + docs — same free memory brain as the MCP bundle; one `brain.said` file works
in both (CLI `ask`/`add` and agent `ask`/`remember` are aligned, including tags on results and tie
scoping hints).

## What's here

- **`said.exe`** — the brain CLI binary (self-contained; the encoder is embedded).
- **[`doc/`](doc/)** — all documentation:
  - **[`doc/how-to-install-said.md`](doc/how-to-install-said.md)** — download and set up `said` on
    Windows, macOS, or Linux. **Do this first.**
  - **[`doc/tutorial-your-first-brain.md`](doc/tutorial-your-first-brain.md)** — zero to first success:
    create a brain, store a few notes, ask it a question. Start here once installed.
  - **How-to guides** — one real task each:
    - [`doc/how-to-store-and-recall-notes.md`](doc/how-to-store-and-recall-notes.md) — the everyday save-and-ask loop
    - [`doc/how-to-populate-a-brain-with-an-llm.md`](doc/how-to-populate-a-brain-with-an-llm.md) — fill a brain fast: let an LLM turn a pile of notes into `add` commands
    - [`doc/how-to-find-a-specific-memory.md`](doc/how-to-find-a-specific-memory.md) — find by meaning, organize with tags and concepts, scope when results tie
    - [`doc/how-to-memory-vs-coding-brain.md`](doc/how-to-memory-vs-coding-brain.md) — pasted code vs real code search; when you need the coding build
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
[`doc/how-to-find-a-specific-memory.md`](doc/how-to-find-a-specific-memory.md) when you need to narrow
results with tags or deep recall.

## Note on recall

`said` returns the **top handful** of relevant memories for a question, not a single guess — the right
memory is essentially always in that set. Each result line shows **tags** (e.g. `quarter:Q2`). When
several results genuinely tie, `ask` prints a **close matches** footer with tag counts and suggests
`--tag` scoping — but stays quiet when `#1` clearly wins (no chatty footer on obvious answers).

You read the top results and pick the one you need. For narrower recall, see
[`doc/how-to-find-a-specific-memory.md`](doc/how-to-find-a-specific-memory.md).

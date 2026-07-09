# How to track and restore versions of a memory (through your agent)

**Goal:** update a fact over time, see its earlier versions, and roll one back — all by talking to your
AI agent.

**Before you start:** your brain is connected to an agent (if not, do the
**[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)** first), and you have at least one named
memory. Naming matters here — a memory you can version is one you gave a short name (id) to, e.g. "when
you saved it, you called it *colour*".

## Update a fact (this creates a new version)

Just tell the agent the new value for a memory you already saved:

    My favourite colour is teal now — update my "colour" note.

The agent saves the new version. Your **older version isn't lost** — the brain keeps it in history and
makes the newest one the one it answers with. Ask it a normal question and you get the current value:

    What's my favourite colour?

You should see the agent answer **teal** (the latest), not the old value.

## See the version history

Ask the agent to show the history of that note:

    Show me the version history of my "colour" note.

You should see a list of versions, oldest first — something like:

    v0: [older]  (superseded)
    v1: [current]

Each line is one version. `v0` is what you first saved; the highest number is what the brain answers
with today. (Behind the scenes the agent called the brain's `history` tool with the note's id.)

## Roll back to an earlier version

Decide which version you want from the history list, then ask the agent to restore it by its number:

    Restore version 0 of my "colour" note.

You should see the agent confirm it checked out that version. Now ask again —

    What's my favourite colour?

— and you get the **restored** value (the old one). The rollback itself becomes the new current version,
so nothing in your history is ever thrown away; you can roll forward again the same way.

## If it doesn't work

- **"I can't find a note called X"** — the memory wasn't saved with that name. Ask the agent to list what
  it has, or re-save the fact with a clear short name, then version it from there.
- **The history shows only one version** — you've only saved it once. Update it (first section above) and
  the history grows.
- **You restored the wrong version** — no harm done: check the history again and restore the one you
  actually want. Every version is still there.

## Next

- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** — the
  everyday save-and-ask loop.
- **[Clean up your brain — delete, recover, and retention](how-to-clean-up-your-brain-with-an-agent.md)**
  — remove notes and get them back.

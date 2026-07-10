# How to clean up your brain — delete, recover, and set retention (through your agent)

**Goal:** remove memories you don't want, get one back if you deleted it by mistake, and (optionally)
tell the brain to age out old deleted notes.

**Before you start:** your brain is connected to an agent (if not, do the
**[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)** first).

Nothing here is truly destructive by default — a deleted memory goes to a recycle bin and can be
restored. It's only gone for good when you explicitly clear old deleted notes.

## Delete a memory

Tell the agent which note to remove:

    Delete my "draft" note.

You should see the agent confirm it's deleted. The memory is **tombstoned** — moved to a recycle bin,
preserved in history — not erased. Your everyday questions won't surface it anymore.

You can also delete by content if you didn't name it:

    Delete the note about the old billing schedule.

The agent finds it and removes it. If it's unsure which note you mean, it will show you candidates first.

## See what's in the recycle bin

Ask the agent what you've deleted:

    What have I deleted? Show me the recycle bin.

You should see a list of deleted memories with when each was removed. (The agent used the brain's
`admin` tool with the `list-tombstones` action.)

## Recover a deleted memory

Found one you want back? Ask the agent to restore it by name:

    Restore my "draft" note.

You should see the agent confirm it's restored — and asking for it normally now works again. Recovery
brings back the exact original text, byte-for-byte.

## Find out when (and how) something was deleted

For an audit trail on a specific note — every version, when it was deleted, and any tags:

    Show me the deletion trail for my "draft" note.

The agent returns the full lineage (this uses `admin`'s `who-deleted` action). Useful if you're keeping
records of what changed and when.

## Reclaim space from deleted notes (empty the recycle bin)

The recycle bin keeps deleted memories indefinitely, and they still take up space in your file. When
you're sure you won't need them back, tell the agent to tidy up and reclaim that space:

**First, preview what would be cleared** (safe — changes nothing):

    Tidy up my brain — show me what a cleanup would reclaim, but don't do it yet.

You should see a summary like *"the recycle bin holds N deleted memory frame(s) (X bytes)"*. (The agent
used the brain's `compact` tool with a dry run.)

**Then, if you're happy, purge them:**

    Empty the recycle bin and reclaim the space.

You should see a confirmation — blocks repacked, bytes saved, and how many deleted memories were purged.
**This is the one step that permanently frees space** — purged memories are no longer recoverable.
Active memories are never touched.

> Just want to tidy the file *without* emptying the recycle bin? Ask *"compact my brain"* on its own —
> it repacks storage and keeps every deleted memory recoverable.

## If something goes wrong

- **"I can't find that note to delete"** — it may already be gone, or was saved with different wording.
  Ask the agent to list what it has on the topic.
- **You restored the wrong one** — restoring is safe and repeatable; check the recycle bin again and
  restore the right one.
- **You want a note gone forever right now** — delete it, then ask the agent to *"empty the recycle bin
  and reclaim the space"* (the `compact` cleanup above). Be sure: after that it cannot be recovered.

> **Automatic age-based cleanup** (clear anything older than N days, legal holds, an audit trail) is an
> **Enterprise** feature, not part of the free memory brain. On this brain you clear the recycle bin
> yourself with the cleanup above, when you choose to.

## Next

- **[Track and restore versions of a memory](how-to-track-versions-with-an-agent.md)** — roll a note
  back to an earlier value.
- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** — the
  everyday save-and-ask loop.

# How to build up a whole brain just by talking to your agent

You have a lot you want your brain to hold — a project's open tasks, everything you decided in a
meeting, a stack of facts about a client. There's **no "bulk import" button** for memories (that's on
purpose — see [Why there's no bulk import](#why-theres-no-bulk-import) below). But you don't need one:
you can fill a brain fast by letting your **AI agent do the saving for you**, one memory at a time. It's
a simple trick, and it works surprisingly well.

You've connected your brain to an agent already (if not, do the
**[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)** first).

## The trick: hand the agent a pile of information and ask it to remember it

Paste or describe everything you want kept, and ask the agent to save it as separate memories. For
example:

    Here are the open items before we can launch: the pack CLI still needs keygen/sign/verify,
    paid packs need encryption + licensing, packs must be locked after publish, and we need the
    registry. Save each of these as its own memory.

The agent reads the pile, splits it into individual facts, and calls **remember** once per fact. You'll
see it confirm each one:

    ✓ Saved: "said pack CLI still needed (keygen/sign/verify)"
    ✓ Saved: "paid packs must be encrypted + licensed"
    ✓ Saved: "packs must be locked after publish"
    ✓ Saved: "registry still needed"

That's four separate memories you can recall with `ask`, built from one paste. Do this a few times and a
brain goes from empty to richly populated in minutes — no import format, no file wrangling.

## Why this works better than a bulk import would

The agent isn't just splitting text — it's **distilling** as it goes. For each memory it writes a clean,
self-contained sentence that will still make sense in six months, gives it a short id, and (if it's a
good agent) tags it so you can browse later. That per-memory judgment — *what is one memory, what's
worth keeping, what to call it* — is exactly what makes recall good afterwards. A dumb bulk-import that
just swallowed your text whole would skip all of that and leave you with a messy brain that's hard to
recall with `ask`.

So the "workaround" is really the feature: **the LLM is the importer**, and it's a smart one.

## Let it happen automatically as you work

You don't even have to ask. Your agent is set up to act as your **note-taker** — whenever you make a
decision, state a preference, or mention a fact worth keeping, it saves that memory on its own and tells
you it did. So a normal working conversation quietly builds your brain in the background. You can always
say *"don't save that"* if you'd rather it didn't.

## Get the agent to reuse your vocabulary (so the brain stays tidy)

When you're saving many related memories, ask the agent to **keep the labels consistent**:

    Before saving these, check what tags I already use and reuse them — don't invent new ones.

The agent can list the tags already in your brain and reuse `status:planned` instead of coining
`status:todo`, so your growing brain stays browsable instead of fragmenting into synonyms. See
**[Organize and find your memories](how-to-organize-and-find-memories-with-an-agent.md)**.

## Check it worked

After a batch, ask the agent to prove it stuck — in a **fresh** question, not the same breath:

    What's still open before launch?

The agent should recall the items you just saved, best-first, quoting them back. If something's missing,
it was probably never saved as its own memory — ask the agent to add that one now and try again.

## Why there's no bulk import

A memory brain is meant to hold **distilled, individually-useful facts**, not raw documents. Dumping a
whole file in would fill it with noise that drowns out the good memories when you `ask`. (If what you
actually have is a *folder of documents or code* you want indexed wholesale, that's a different job — the
coding builds of `said` do that; a memory brain doesn't. See
**[Memory brain vs coding brain](how-to-memory-vs-coding-brain.md)**.) For memories, the right unit is one
clear fact at a time — and an agent produces exactly that.

## Next

- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** — the
  everyday save/recall pattern.
- **[Organize and find your memories](how-to-organize-and-find-memories-with-an-agent.md)** — tags and
  concepts that keep a growing brain easy to browse and recall.
- **[Move your brain to another agent or machine](how-to-move-your-brain-to-another-agent.md)**

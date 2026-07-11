# How to organize and find memories at scale (through your agent)

**Goal:** as your brain grows, keep related memories connected and find exactly what you need — a
specific note, everything on a topic, or a broad summary.

**Before you start:** your brain is connected to an agent (if not, do the
**[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)** first) and has a handful of memories in it.

## Find one specific memory

Ask in your own words — the agent finds it by meaning, not exact wording:

    What did I decide about the billing schedule?

You should see the agent quote the matching memory. If several are close, it shows the best ones and you
pick. This is the everyday case and needs nothing special.

## Get everything on a topic (broad recall)

When you want *all* the related memories, not just the single best one — for a summary, a review, or
"tell me everything I know about X" — say so:

    Pull together everything I've saved about the payment system using deep recall.

The agent widens recall (it calls `ask` in **deep** mode) and gathers all the relevant memories, then
synthesizes them for you. Use this when one answer isn't enough and you want the full picture. The magic
phrase is to ask for **everything on a topic** or say **"using deep recall"** — the agent knows to widen
the `ask` pool.

## Connect related memories with concepts

The brain can link memories that share a **concept**, so they're reachable together even when their
wording differs. You create a concept by putting it in double brackets when you save a note. Tell the
agent:

    Remember: Dr. Sarah is our cardiologist [[heart]].
    Remember: the Vance heart surgery is on Tuesday [[heart]].

Both notes now share the **heart** concept. Later, a question about one can reach the other through that
link — useful for "who's involved in X" or "what's connected to Y" questions that no single note answers
on its own.

> Tip: reuse an existing concept name rather than inventing near-duplicates (use `heart`, not a new
> `heart-health`). See the next section to check what concepts already exist.

## See the concepts you've built

Ask the agent for your concept vocabulary:

    What concepts are my memories organized under?

You should see a list of concepts with how many memories carry each — e.g. `heart (2)`, `budget (1)`.
(The agent used the brain's `list_concepts` tool.) To narrow it:

    List my concepts that start with "fin".

This is how you keep the organization tidy: before adding a new `[[concept]]`, glance at the list and
reuse an existing one so related memories stay connected instead of fragmenting.

## Tag memories to browse by facet

Concepts link related memories together; **tags** are the other vocabulary — short labels like
`project:said`, `status:planned`, or `topic:launch` that describe *what a memory is about* so you can
browse or filter by them later. Ask the agent to add them when it saves:

    Remember these launch items and tag each with project:said and status:planned.

A good agent tags automatically as it saves — it's set up to. You don't pick tags from a fixed list;
the agent chooses sensible ones from the content.

## See the tags you've used

Ask the agent for your tag vocabulary:

    What tags do my memories use?

You should see each tag with how many memories carry it — e.g. `project:said (26)`, `status:planned
(15)`. (The agent used the brain's `list_tags` tool.) To narrow it:

    List my tags that start with "status:".

This is the tidiness loop for tags: before adding a new one, the agent checks what's already in use and
**reuses** it (`status:planned`, not a new `status:todo`) so your growing brain stays browsable instead
of splintering into synonyms. If you notice drift, ask the agent to consolidate: *"I have both
status:planned and status:todo — merge them onto one tag."*

> **Concepts vs tags — which when?** Use a `[[concept]]` to *link* memories that belong together (so a
> question about one reaches the others). Use a `tag` to *classify* a memory by a facet (project, status,
> kind) you'll browse or filter by. Many memories use both.

## When you get too many similar results

Once your brain has dozens of memories, a vague question can return **several equally good matches**
instead of one clear winner — that's normal. The brain surfaces a **top handful**; your agent picks the
right one. On genuine ties, `ask` also appends a **close matches** note (tag counts + scoping hints) so
the agent knows to re-ask with a tag instead of guessing. When too many look alike, **narrow with tags**
before asking:

1. **Browse tags** — ask the agent: *"What tags do my memories use?"* (`list_tags`).
2. **Pick a facet** — e.g. `quarter:Q2`, `project:said`, `topic:launch`.
3. **Ask with that tag** — tell the agent to recall using that tag plus a keyword from your question:

       Ask my brain about offline integrations, but only memories tagged quarter:Q2.

Behind the scenes the agent calls `ask` with a `tags` filter so only memories carrying that tag are
considered — this breaks cross-topic ties when many notes share a broad theme like integrations or
roadmap.

If you're still unsure which result is right, ask for the exact text: *"Show me memory X word-for-word"*
(`get`).

## If you can't find something

- **The agent finds nothing** — the memory may never have been saved, or was saved with very different
  words. Ask the agent to save the fact now, then try again.
- **You get the wrong note among near-identical ones** — add a distinctive detail to your question (a
  name, a number, a date) so the right one stands out, or **scope with a tag** (section above).
- **Related memories aren't turning up together** — they probably don't share a concept. Re-save them
  with the same `[[concept]]` so the brain can link them.

## Next

- **[Memory brain vs coding brain](how-to-memory-vs-coding-brain.md)** — pasted snippets vs indexing a
  codebase.
- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** — the
  everyday save-and-ask loop.
- **[Track and restore versions of a memory](how-to-track-versions-with-an-agent.md)** — update a fact
  and roll it back.

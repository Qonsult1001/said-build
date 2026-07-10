# How to find and organize memories

> Goal: get memories back out of your brain — one specific note, everything on a topic, or a broad
> summary — and keep a growing brain tidy with tags and concepts. Assumes you have a brain with notes in
> it; if not, do the [tutorial](tutorial-your-first-brain.md) first.

There are two everyday commands: **`ask`** (find by meaning) and **`get`** (fetch one note by its id).
`ask` is the one you'll use almost always.

## Find one memory by meaning

Ask a plain-English question — you don't need the exact words you stored:

    said --path my-brain.said ask "how do I get into the garage"

The best memory comes first:

    Ask: "how do I get into the garage"  (3 results in 3.6ms)

      1. [0.50][semantic] garage
          The garage door code is 4827.

`ask` finds by **meaning** — notice the question shares no words with "The garage door code is 4827" and
it still found it.

- **If you get `(no match …)`** → the brain has nothing close enough. Add the note, or try asking a
  slightly different way.

## Get everything on a topic (deep recall)

When you want *all* the related memories, not just the single best one — for a review or "tell me
everything I know about X" — use **`--deep`**:

    said --path my-brain.said ask "payment system" --deep

Deep mode returns every memory above the relevance threshold instead of capping at the default top few.
Use this when one answer isn't enough and you want the full picture.

## Read one memory exactly

When you know the id, `get` returns the stored text verbatim:

    said --path my-brain.said get garage

    The garage door code is 4827.

- **If you see `Error: Document not found: <id>`** → that id isn't in this brain. Check with
  `said stats`, or `ask` for it by meaning.

## Tag memories when you save

Tags are short labels like `project:said`, `status:planned`, or `quarter:Q2` — facets you can browse and
filter by later. Attach them with `--tag` (repeatable):

    said --path my-brain.said add "Q2 integrations use Redis" --id q2-integ \
      --tag quarter:Q2 --tag topic:integrations

Before adding new tags, check what's already in use and **reuse** names instead of inventing synonyms
(`status:planned`, not a new `status:todo`):

    said --path my-brain.said list-tags

    Tags (3 distinct):
        26  project:said
        15  status:planned
         4  quarter:Q2

Narrow the list:

    said --path my-brain.said list-tags --prefix status:

## Connect related memories with concepts

Concepts link memories that share a **wikilink** — put `[[concept]]` in the text when you save:

    said --path my-brain.said add "Dr. Sarah is our cardiologist [[heart]]" --id cardio
    said --path my-brain.said add "Vance heart surgery is Tuesday [[heart]]" --id surgery

Both notes share the **heart** concept. A question about one can reach the other through that link.

See what concepts exist before adding near-duplicates:

    said --path my-brain.said list-concepts

    said --path my-brain.said list-concepts --prefix fin

> **Concepts vs tags — which when?** Use a `[[concept]]` to *link* memories that belong together. Use a
> `tag` to *classify* a memory by a facet (project, status, quarter) you'll browse or filter by. Many
> memories use both. `list-concepts` shows wikilinks; `list-tags` shows the tag vocabulary — they're
> different lists.

## When you get too many similar results

Once your brain has dozens of memories, a vague `ask` can return **several equally good matches** instead
of one clear winner — that's normal. The brain surfaces a **top handful**; you read the list and pick.
When too many look alike, **narrow with tags** before asking:

1. **Browse tags** — `said list-tags`
2. **Pick a facet** — e.g. `quarter:Q2`, `project:said`, `topic:launch`
3. **Ask with that tag** — repeat `--tag` for each filter (AND logic):

       said --path my-brain.said ask "offline integrations" --tag quarter:Q2

Only memories carrying **all** listed tags are considered — this breaks cross-topic ties when many notes
share a broad theme.

Example: without a tag, "offline integrations" might return both Q2 and Q3 notes. With
`--tag quarter:Q2`, only the Q2 note remains in the pool.

If you're still unsure which result is right, fetch it verbatim: `said get <id>`.

## If you can't find something

- **`(no match …)`** — the memory may never have been saved, or was saved with very different words. Add
  it now, then try again.
- **Wrong note among near-identical ones** — add a distinctive detail to your question (a name, a number,
  a date), or **scope with `--tag`** (section above).
- **Related memories aren't turning up together** — they probably don't share a `[[concept]]`. Re-save
  them with the same concept name.

## Result

You can always get a memory back: **`ask`** when you remember roughly what it's *about*, **`get`** when
you know its *id*, **`--deep`** when you want everything on a topic, and **`--tag`** when results are
too noisy. That's the everyday retrieval story.

## See also

- **[Memory brain vs coding brain](how-to-memory-vs-coding-brain.md)** — pasted snippets vs indexing a
  codebase.
- Adding and removing notes → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- Every command and flag → [Command reference](cli-reference.md)

# How to find a memory

> Goal: get a memory back out of your brain. This assumes you have a brain with some
> notes in it; if not, do the [tutorial](tutorial-your-first-brain.md) first.

There are just two things to know: **`ask`** (find by meaning) and **`get`** (fetch a
specific note by its id). `ask` is the one you'll use almost always — it's the smart
search that figures out what you mean.

## Steps

1. **To find something by meaning, use `ask`** — a plain-English question, in your own
   words. You don't need to remember how you wrote the note.

       said --path my-brain.said ask "how do I get into the garage"

   The best memory comes first:

       Ask: "how do I get into the garage"  (3 results in 3.6ms)

         1. [0.50][semantic] garage
             The garage door code is 4827.

   `ask` searches by **meaning** — notice the question shares no words with "The garage
   door code is 4827" and it still found it. Ask it anything: "who is my dentist", "when
   is mom's birthday", "what's the wifi password".

   - **If you get `(no match …)`** → the brain has nothing close enough. Add the note,
     or try asking a slightly different way.

2. **To read one exact note you already know the id of, use `get`** — it returns the
   stored text verbatim, nothing else.

       said --path my-brain.said get garage

       The garage door code is 4827.

   - **If you see `Error: Document not found: <id>`** → that id isn't in this brain.
     Check what you have with `said --path my-brain.said stats`, or just `ask` for it by
     meaning.

## Result

You can always get a memory back: **`ask`** when you remember roughly what it's *about*,
**`get`** when you know its *id*. That's the whole retrieval story for everyday use.

## See also

- Adding and removing notes → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- Power users: `said` also has lower-level `query` (raw semantic) and `grep` (exact text)
  commands — you don't need them for normal use; see the
  [Command reference](cli-reference.md) if you're curious.

# How to import memories from another tool

> Goal: bring your existing memories from another system (like mem0 or memvid) into a
> `said` brain. This assumes you have a brain to import into; if not, do the
> [tutorial](tutorial-your-first-brain.md) first.

If you've been using another memory tool, you don't have to start over — `said` can read
its export and bring those memories in.

> **Importing your own browser history, email, or chat exports** is a different command family
> (`said import browser` / `email` / `chatgpt` / `claude`). This guide is only about **migrating** from
> another *memory tool*. For personal data, see
> [How to import your own data](how-to-import-your-own-data.md).

## Steps

1. Check which tools `said` can import from:

       said import --list

   You should see the supported systems:

       Registered migration adapters:
         - memvid
         - mem0

   - **If the tool you used isn't listed** → `said` can't import it directly yet. You can
     still add those memories by hand with `add` (see
     [How to store and recall personal notes](how-to-store-and-recall-notes.md)).

2. Export your memories from the other tool (follow that tool's own instructions). The
   exact file format `said` expects depends on the tool:

   - **mem0** → a **JSONL** file (one memory per line), each line with a `memory` field:

         {"id":"m1","memory":"The cat sleeps on the windowsill","user_id":"u1"}
         {"id":"m2","memory":"Coffee is at 8am","user_id":"u1"}

   - **memvid** → a **JSON array**, each item with a `content` field:

         [{"id":"v1","content":"Project deadline is Friday"},
          {"id":"v2","content":"Budget approved last week"}]

3. Import them into your brain, naming the source system and the export file:

       said --path my-brain.said import --from mem0 --source ./my-mem0-export.jsonl

   You should see a summary like:

       ✓ Imported from mem0:
         read:    2
         written: 2

   - **If you see `... missing 'memory'` (or `'content'`)** → the file isn't in the shape
     above. mem0 must be JSONL with a `memory` field; memvid must be a JSON array with a
     `content` field.
   - Each imported memory keeps a tag noting where it came from.

4. Confirm they arrived:

       said --path my-brain.said stats

   The memory count should have grown by the number you imported. Then try asking about
   one:

       said --path my-brain.said ask "<something you know you stored before>"

## Result

Your memories from the other tool now live in your `said` brain and are recallable with `ask` like any
other memory.

## See also

- Adding memories by hand → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- All commands → [Command reference](cli-reference.md)

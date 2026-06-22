# How to import memories from another tool

> Goal: bring your existing memories from another system (like mem0 or memvid) into a
> `said` brain. This assumes you have a brain to import into; if not, do the
> [tutorial](tutorial-your-first-brain.md) first.

If you've been using another memory tool, you don't have to start over — `said` can read
its export and bring those memories in.

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

2. Export your memories from the other tool to a file or folder (follow that tool's own
   instructions — e.g. mem0's export).

3. Import them into your brain, naming the source system and the export you just made:

       said --path my-brain.said import --from mem0 --source ./my-mem0-export.json

   - **If your export is a folder** → point `--source` at the folder; `said` reads every
     supported file inside it.
   - Each imported memory keeps a tag noting where it came from, so you can tell imported
     memories apart later.

4. Confirm they arrived:

       said --path my-brain.said stats

   The memory count should have grown by the number you imported. Then try asking about
   one:

       said --path my-brain.said ask "<something you know you stored before>"

## Result

Your memories from the other tool now live in your `said` brain and are searchable with
`ask` like any other memory.

## See also

- Adding memories by hand → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- All commands → [Command reference](cli-reference.md)

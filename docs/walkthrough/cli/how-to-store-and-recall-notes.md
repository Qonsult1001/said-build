# How to store and recall personal notes

> Goal: use a `.said` brain as a personal memory — add notes over time and pull them back out by asking
> questions. This assumes you've already created a brain file; if not, do the
> [tutorial](tutorial-your-first-brain.md) first.

These steps assume you already have a brain file (e.g. `my-brain.said`).

## Steps

1. Add a note. Give it a short `--id` so you can fetch it directly later.

       said --path my-brain.said add "Wifi password is sunflower-42." --id wifi

   You should see:

       Added 'wifi' (30 bytes)

   - **If you omit `--id`** → `said` auto-generates one. Fine for throwaway notes, but you won't be able
     to `get` it by a memorable name.
   - **If you use an `--id` that already exists** → the new content replaces the old as the current
     version (the old one is kept in history, not lost).

2. Add as many notes as you like — each is one `add` call.

       said --path my-brain.said add "Mom's birthday is June 14." --id bday

3. Recall by asking a plain-English question (you don't need the original words):

       said --path my-brain.said ask "when should I call mom"

   The brain returns the closest memories by meaning, best first:

       1. [0.51][semantic] bday
           Mom's birthday is June 14.

   - **If you get `(no match ...)`** → the brain has nothing close enough. Add the note, or try the
     wording closer to what you stored. See also [How to find a memory](how-to-find-a-specific-memory.md).

4. Fetch a specific note verbatim by its `--id`:

       said --path my-brain.said get wifi

   You should see the exact stored text:

       Wifi password is sunflower-42.

5. Remove a note you no longer want:

       said --path my-brain.said delete wifi

   You should see:

       Deleted: wifi

   The note stops showing up in answers.

## Result

You can now grow a brain over time and retrieve memories either by **asking** (by meaning) or by
**getting** (by id). Your whole memory lives in the one `.said` file.

## See also

- Stop repeating `--path` → [How to set a default brain file](how-to-set-a-default-brain.md)
- Finding memories with `ask` and `get` → [How to find a memory](how-to-find-a-specific-memory.md)
- Full options for `add`, `get`, `delete` → the [Command reference](cli-reference.md).

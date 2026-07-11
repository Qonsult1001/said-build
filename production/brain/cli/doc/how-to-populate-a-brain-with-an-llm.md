# How to fill a brain fast by letting an LLM do the `add` calls

> Goal: populate a `.said` brain with many memories at once — without a bulk-import format. There isn't
> one for memories (on purpose), but you can have an LLM turn a pile of information into a series of
> `said add` calls and run them. This assumes you've created a brain file; if not, do the
> [tutorial](tutorial-your-first-brain.md) first.

Each memory in a brain is added one at a time with `add`. Typing dozens of `add` commands by hand is
tedious — so the trick is to let an LLM (Claude, ChatGPT, whatever you use) **write the commands for
you** from plain notes. It distils your text into clean, self-contained memories and emits one `add` per
fact; you paste and run.

## Steps

1. Give an LLM your raw information and ask it to produce `said add` commands. Prompt it like this:

       Turn the notes below into `said add` commands for a .said brain. One command per distinct
       fact. Each memory should be a clear, self-contained sentence, with a short --id and one or
       two --tag flags (namespace:value, e.g. project:said, status:planned). Notes:
       <paste your notes / meeting summary / task list here>

2. The LLM gives you back a block of commands, one per memory. For example:

       said --path my-brain.said add "The pack CLI still needs keygen, sign, and verify commands." --id launch-pack-cli --tag project:said --tag status:planned
       said --path my-brain.said add "Paid packs must be encrypted and licensed before sale." --id launch-encrypt --tag project:said --tag status:planned
       said --path my-brain.said add "Packs must be locked (read-only) after publish." --id launch-lock --tag project:said --tag status:planned

3. Paste that block into your terminal and run it. Each line adds one memory; you'll see a confirmation
   per line:

       Added 'launch-pack-cli' (61 bytes)
       Added 'launch-encrypt' (55 bytes)
       Added 'launch-lock' (48 bytes)

   - **Windows PowerShell / older shells** — run the lines one per line (don't rely on `&&` chaining;
     PowerShell 5.1 doesn't support it). Pasting the block as separate lines works everywhere.
   - **If an `--id` repeats** → the later `add` replaces the earlier memory (the old version is kept in
     history, not lost). Ask the LLM for unique ids if you don't want that.

4. Confirm it landed — ask a plain-English question, not the wording you stored:

       said --path my-brain.said ask "what's still open before launch?"

   You should get the memories back, best-first:

       1. [0.95][semantic] launch-pack-cli
           The pack CLI still needs keygen, sign, and verify commands.

5. Check the vocabulary the LLM used, so your next batch reuses it instead of inventing synonyms:

       said --path my-brain.said list-tags

   You should see each tag and its count:

       Tags (3 distinct):
           3  project:said
           3  status:planned

   Before the next batch, tell the LLM which tags already exist so it reuses them (`status:planned`, not
   a new `status:todo`). This keeps a growing brain tidy — see
   [How to organize and find memories](how-to-find-a-specific-memory.md).

## Why one-at-a-time (and not a bulk import)

A memory brain holds **distilled facts**, not raw documents — so the right unit is one clear memory at a
time, and an LLM produces exactly that while giving each a good id and tags. Dumping a whole document in
would fill the brain with noise that drowns out real memories on recall. (If you want to index a *folder
of documents or code* wholesale, that's a different tool — the coding builds of `said` do bulk ingest; a
memory brain doesn't.)

> Prefer to talk to an agent instead of running commands? If you've connected the brain to Claude,
> Cursor, or Copilot, you can skip the terminal entirely — just paste your notes and say "save each of
> these." See the MCP guide *How to build up a whole brain just by talking to your agent*.

## Result

You can go from an empty brain to dozens of well-formed, tagged memories you can recall with `ask` in a
couple of pastes — the LLM does the distilling and the typing; you run and verify.

## See also

- Everyday manual save/recall → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- Pasted code vs coding build → [Memory brain vs coding brain](how-to-memory-vs-coding-brain.md)
- Importing your browser history, email, or chat exports → [How to import your own data](how-to-import-your-own-data.md)
- Importing from another memory tool (mem0, memvid) → [How to import memories](how-to-import-memories-from-another-tool.md)
- Full options for `add` → the [Command reference](cli-reference.md).

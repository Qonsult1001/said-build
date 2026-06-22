# Getting started with `said` — your first portable brain

> In this tutorial you'll create a `.said` brain file, store two notes in it, and ask the brain a
> question in plain English — and watch it find the right note by *meaning*, not just keywords. By the
> end you'll have a working personal memory file you built yourself. No prior experience needed.

Every command below is real; type them exactly and check the "You should see:" block after each one.

## Before you start

- [ ] The `said` app, version 0.11.1 or newer. Check it:

      said --version

  You should see:

      said 0.11.1

  If `said` isn't found, download the `said-brain-<your-platform>` build from the project's releases and
  put the binary on your `PATH` (or run it by its full path).
- [ ] A terminal open in an empty folder you can write to (we'll create one file there).
- [ ] About 5 minutes.

Everything you do here happens inside a single file you create. Nothing else on your computer is
touched, so you can delete that file and start over at any time.

## Step 1 — Create your brain file

A `.said` file *is* the brain — one portable file that holds all your memories. Create one:

    said create my-brain.said

You should see:

    Created: my-brain.said (mode: portable, immutable)

There is now a file called `my-brain.said` in your folder. That's your brain — empty, for the moment.

## Step 2 — Store your first memory

Add a note. The `add` command stores text; `--id` gives the note a short name so you can find it later.

    said --path my-brain.said add "My dentist is Dr. Sarah Chen, appointment every March." --id dentist

You should see:

    Added 'dentist' (54 bytes)

The `--path my-brain.said` part tells `said` which brain to use. You'll use it on every command in this
tutorial.

## Step 3 — Store a second memory

Add one more, so the brain has something to choose between:

    said --path my-brain.said add "The garage door code is 4827." --id garage

You should see:

    Added 'garage' (29 bytes)

Your brain now holds two memories.

## Step 4 — Ask the brain a question

Now the interesting part. Ask a plain-English question — note you do **not** use any of the words from
the stored note:

    said --path my-brain.said ask "who is my dentist"

You should see (your scores may differ by a little):

    Ask: "who is my dentist"  (2 results in 7.41ms)

      1. [0.48][semantic] dentist
          My dentist is Dr. Sarah Chen, appointment every March.
      2. [0.43][semantic] garage
          The garage door code is 4827.

The dentist note came first. The `[semantic]` tag means the brain matched on **meaning** — it connected
"who is my dentist" to "My dentist is Dr. Sarah Chen" even though you asked it differently. That is the
whole point of `said`: it remembers *what things mean*, not just the exact words.

## Step 5 — Confirm what's in your brain

Check the brain's contents:

    said --path my-brain.said stats

You should see (among other lines):

      Memories:          2
      Memories indexed:  2

Two memories stored, both searchable. That matches what you added.

## You did it

You created a portable brain, stored two memories, and asked it a question in your own words — and it
found the right answer by meaning. That `my-brain.said` file is self-contained: copy it to a USB stick or
another machine and it still answers the same questions, with no internet and no setup.

What just happened: when you saved each note, `said` stored it as a searchable memory. When you asked a
question, it matched your question against those memories by **meaning** — not just keywords — which is
why it found the answer even though you asked in different words.

## Next steps

- Keep adding to this brain and pulling things back out → **[How to store and recall personal notes](how-to-store-and-recall-notes.md)**
- Stop typing `--path` on every command → **[How to set a default brain file](how-to-set-a-default-brain.md)**
- Find any memory by meaning, or fetch one by id → **[How to find a memory](how-to-find-a-specific-memory.md)**
- Every command and flag → the [CLI reference](../said-structure/07-cli-reference/).

# Memory brain vs coding brain — what the free CLI does and doesn't do

**Goal:** know what your **free memory brain** can do when you paste code, save snippets, or ask about a
codebase — and when you need the **coding build** of `said` instead.

**Before you start:** you're on the free **brain** CLI (memory commands only). Check with:

    said --help

You should see `create`, `add`, `ask`, `get`, `list-tags`, and friends — but **not** `init`, `search`, or
`sym` (those are coding-tier commands).

## What the free memory brain is for

It stores **text you save** — notes, decisions, preferences, meeting facts, pasted paragraphs — and
finds them later with **`ask`** (by meaning) or **`get`** (by exact id). Everything lives in one
portable `.said` file. No cloud, fully offline.

That's the product: **your long-term memory file**, not a search engine over your repo.

## I pasted code — why can't `ask` find symbols?

When you `add` something that looks like code, the brain **keeps it as a text note**. You can recall it
by meaning:

    said --path my-brain.said add 'fn foo() { return 42; }' --id my-snippet

Later:

    said --path my-brain.said ask "what does my foo function return"

You should see `my-snippet` in the results and answer **42**.

What it **does not** do:

- **`sym foo`** — no symbol table on the memory brain
- **`search` / codebase grep** — no `search` command on the free tier
- **`said init <dir>`** — no bulk repo ingest on the memory brain
- **AST chunking** — it didn't parse your repo into functions and classes
- **"Find every place `foo` is called"** — that needs a coding brain with `said init` on your project

The memory brain stores pasted code as **text you can ask about in plain English** — not as indexed
source code.

## Memory brain vs coding brain (at a glance)

| You want… | Memory brain (free) | Coding build |
|---|---|---|
| Save a fact, decision, password, meeting note | ✅ `add` | ✅ |
| Recall in plain English later | ✅ `ask` | ✅ |
| Paste a short code snippet as a note | ✅ (text only) | ✅ |
| Index a whole repo (`said init`) | ❌ | ✅ |
| Look up a function by name (`sym`) | ❌ | ✅ |
| Semantic recall over ingested source files | ❌ (use `ask` on memories you saved) | ✅ |
| Ingest PDFs / enterprise compliance | ❌ | full / Enterprise tiers |

## When to upgrade to the coding build

Consider the **coding** build of `said` when you want the brain to **understand a codebase** — not just
remember sentences *about* the code:

- "Where is `PaymentProcessor` defined?"
- "Who calls `ValidateOrder`?"
- "Index this repo so I can ask questions from it"

That path uses **`said init`** on a folder, then `sym`, `ask`, and related code-tier commands. It's a
different bundle — same portable `.said` file format, but a larger binary and a different command
surface.

The **memory brain you're using now** is the right choice when the value is **persistent personal/team
notes** in one portable file, not IDE-style code intelligence.

## What to run

**Saving a snippet (memory brain — correct):**

    said add 'fn foo() { return 42; }' --id my-snippet

**Expecting repo search (wrong on memory brain):**

    said search foo

That command isn't on the memory brain. Instead:

    said ask "do I have anything saved about foo"

Or upgrade to the coding build and run `said init` on your repo first.

## Next

- **[How to store and recall personal notes](how-to-store-and-recall-notes.md)** — everyday save and
  `ask`.
- **[How to find and organize memories](how-to-find-a-specific-memory.md)** — tags, concepts, deep
  recall, and what to do when many memories match.
- **[How to fill a brain fast with an LLM](how-to-populate-a-brain-with-an-llm.md)** — batch `add`
  commands without bulk import.

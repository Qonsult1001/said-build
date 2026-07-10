# Memory brain vs coding brain — what the free tier does and doesn't do

**Goal:** know what your **free memory brain** can do when you paste code, save snippets, or ask about a
codebase — and when you need the **coding build** of `said` instead.

**Before you start:** you're on the free **brain** MCP server (memory-only tools). If you're not sure,
ask the agent *"what memory tools do you have?"* — you should see `ask`, `remember`, `get`, and friends,
but **not** `search`, `sym`, or `init`.

## What the free memory brain is for

It stores **text you save** — notes, decisions, preferences, meeting facts, pasted paragraphs — and
finds them later with **`ask`** (by meaning) or **`get`** (by exact id). Everything lives in one
portable `.said` file. No cloud, fully offline.

That's the product: **your agent's long-term memory**, not a search engine over your repo.

## I pasted code — why can't the agent find symbols?

When you `remember` something that looks like code, the brain **keeps it as a text note**. You can recall
it by meaning:

    Remember: fn foo() { return 42; } — id my-snippet

Later:

    What does my foo function return?

The agent should find `my-snippet` and answer **42**.

What it **does not** do:

- **`sym foo`** — no symbol table on the memory brain
- **`search` / codebase grep** — no `search` tool on the free tier
- **AST chunking** — it didn't parse your repo into functions and classes
- **"Find every place `foo` is called"** — that needs a coding brain with `said init` on your project

When you save code-looking text, the server tells you honestly: *stored as text — ask for it later* and,
if it looks like source code, *does NOT index code — use the coding build*.

## Memory brain vs coding brain (at a glance)

| You want… | Memory brain (free) | Coding build |
|---|---|---|
| Save a fact, decision, password, meeting note | ✅ `remember` | ✅ |
| Recall in plain English later | ✅ `ask` | ✅ |
| Paste a short code snippet as a note | ✅ (text only) | ✅ |
| Index a whole repo (`said init`) | ❌ | ✅ |
| Look up a function by name (`sym`) | ❌ | ✅ |
| Semantic search over source files | ❌ (use `ask` on ingested code) | ✅ |
| Ingest PDFs / enterprise compliance | ❌ | full / Enterprise tiers |

## When to upgrade to the coding build

Consider the **coding** build of `said` when you want the brain to **understand a codebase** — not just
remember sentences *about* the code:

- "Where is `PaymentProcessor` defined?"
- "Who calls `ValidateOrder`?"
- "Index this repo so my agent can answer from it"

That path uses **`said init`** (or MCP `init`) on a folder, then `sym`, `ask`, and related code-tier tools.
It's a different bundle — same portable `.said` file format, but a larger binary and a different tool
surface.

The **memory brain you're using now** is the right choice when the value is **persistent personal/team
notes** across chats and machines, not IDE-style code intelligence.

## What to tell your agent

**Saving a snippet (memory brain — correct):**

    Remember this helper: fn foo() { return 42; } — call it my-snippet.

**Expecting repo search (wrong on memory brain):**

    Search my codebase for the foo function.

Instead, on memory brain:

    Do I have anything saved about foo?

Or upgrade to coding build and ingest the repo first.

## Next

- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** —
  everyday save and `ask`.
- **[Organize and find your memories](how-to-organize-and-find-memories-with-an-agent.md)** — tags,
  concepts, and what to do when many memories match.
- **[Build a brain by talking to your agent](how-to-build-a-brain-by-talking-to-your-agent.md)** — fill
  a memory brain fast without bulk import.

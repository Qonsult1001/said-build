# Getting started with `said` over MCP — give your AI agent a memory

> In this tutorial you'll connect your `.said` brain to an AI agent (Claude, Cursor, or any
> MCP-capable assistant), tell it something to remember, then — in a *fresh* conversation — ask about
> it and watch the agent recall it. By the end your agent has a persistent memory it carries between
> chats. No prior experience needed.
>
> This is the **MCP** walkthrough. To use `said` directly from the terminal instead, see
> **[Getting started — your first portable brain](../../cli/doc/tutorial-your-first-brain.md)**.

Every step below is real. Follow them in order and check the "You should see:" block after each one.

## Before you start

- [ ] The `said` app, version 0.11.9 or newer. Check it:

      said-mcp --version

  You should see:

      said-mcp 0.11.9

  (Or a newer version — match whatever your install shipped.) If `said-mcp` isn't found, install it
  first: **[How to download and install `said`](how-to-install-said.md)**.
- [ ] An AI agent that speaks **MCP** — for example Claude Desktop, Claude Code, or Cursor. Any of them works.
- [ ] About 10 minutes.

Nothing here touches the rest of your computer. Your memory lives in one file you create, and you can
delete that file to start over at any time.

## Step 1 — Create your brain file

Your memory lives in a single `.said` file. Create one in a folder you can write to:

    said create my-brain.said

You should see:

    Created: my-brain.said (mode: portable, immutable)

That file *is* your brain. Everything the agent remembers goes here.

> **Already have a brain on this machine?** The free version keeps **one brain per computer**. If you
> already have one (the installer usually creates `~/.said/brain.said`, Windows:
> `%USERPROFILE%\.said\brain.said`), `said create` will point you at it rather than make a second — that's
> expected. Just use that existing file's path everywhere this tutorial says `my-brain.said`, and skip to
> Step 2. (Multiple brains on one machine — e.g. separate work vs personal — is an Enterprise feature.)

## Step 2 — Point your agent at the brain

Your agent needs one line of configuration telling it to start `said` as a memory server. The exact
file depends on your agent — here are the common ones. Use the **full path** to your `my-brain.said`.

**Claude Desktop / Claude Code** — add this to your MCP config (`claude_desktop_config.json` or the
`.mcp.json` in your project):

    {
      "mcpServers": {
        "said": {
          "command": "said-mcp",
          "args": ["--path", "/full/path/to/my-brain.said"]
        }
      }
    }

**Cursor** — add the same block to Cursor's MCP settings.

Save the file and **restart your agent** so it picks up the new server.

You should see: your agent lists a memory server named **said** as connected (Claude shows it in the
tools/MCP panel). If it doesn't appear, double-check the path to `my-brain.said` is correct and absolute.

## Step 3 — Tell the agent something to remember

In a normal chat with your agent, say something worth keeping — a preference, a fact, a decision. For
example, type to your agent:

    Remember that my wifi password is sunflower-42.

You should see: the agent confirm it saved the memory — something like *"✓ Saved that to your brain."*
Behind the scenes the agent called the brain's `remember` tool. **You don't have to run any commands
yourself** — the agent is set up to act as your note-taker and write to the brain on its own.

## Step 4 — Start a fresh conversation and ask

This is the moment that matters. **Open a brand-new chat** with the same agent (or come back tomorrow —
the memory is in the file, not the chat). Ask:

    What's my wifi password?

You should see: the agent answer **sunflower-42** — recalling it from the brain, even though you never
mentioned it in *this* conversation. It found the memory by meaning, using the brain's `ask` tool.

That's the whole idea: the chat forgets, but the brain remembers — and it travels with you.

## Step 5 — Confirm it's really stored

Ask the agent:

    How many memories are in my brain?

You should see: a count (at least 1) and a short summary — the agent called `status`. Your memory is
saved in `my-brain.said`, a real file you own. Copy that file to another machine, point a different
agent at it, and every memory comes along.

## Recap

You just:

1. Created a portable brain file (`my-brain.said`).
2. Connected it to your AI agent over MCP.
3. Had the agent **remember** a fact for you — automatically.
4. Recalled it in a **fresh conversation**, proving the memory survives between chats.

Your agent now has a memory that persists across conversations, models, and machines — all in one file.

## Where to go next

- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** — the
  everyday pattern of saving and asking.
- **[Move your brain to another agent or machine](how-to-move-your-brain-to-another-agent.md)** — one
  file, any MCP agent.
- Prefer the terminal? **[The command-line walkthrough](../../cli/doc/tutorial-your-first-brain.md)** does all of this
  with `said` commands directly.

Stuck? The brain is just a file — delete `my-brain.said` and start this tutorial again from Step 1.

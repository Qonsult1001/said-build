# How to move your brain to another agent or machine

Your entire memory is one file — `my-brain.said`. That's the whole point: it travels. This guide shows
how to take it to a different AI agent, a new computer, or share it with a teammate.

Starting assumption: you already have a working brain file (from the
**[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)**).

## Move it to another machine

1. **Copy the file.** Put `my-brain.said` on the other machine — USB stick, cloud drive, `scp`, however
   you move any file. It's self-contained; there's nothing else to copy.
2. **Point the agent there.** On the new machine, add the same MCP config block, using the file's new
   full path:

       {
         "mcpServers": {
           "said": {
             "command": "said-mcp",
             "args": ["--path", "/new/path/to/my-brain.said"]
           }
         }
       }

3. **Restart the agent.** Ask it something you saved before — it recalls it. Every memory came along.

## Switch to a different agent

The same file works with any MCP-capable agent — Claude, Cursor, and others. To move from one to
another, just add the config block above to the new agent (pointing at the same file) and restart it.
You don't convert or export anything; both agents read the one file.

If you want the two agents to share the *same* memory, point them both at the *same* file path. If you
want them separate, give each its own copy.

## Share it with a teammate

Send them the `my-brain.said` file. When they point their agent at it, they get everything in it. A
`.said` file is a normal file — share it the way you'd share a document.

> **Heads-up:** whoever has the file has everything in it. Don't share a brain that holds passwords or
> private notes unless you mean to.

## Switch which brain an agent uses (without restarting)

If your agent is already running and you want it to use a *different* brain file, ask it:

    Switch to the brain at /path/to/other-brain.said

The agent attaches to that file for the rest of the session. Ask it to switch back the same way.

## Next

- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)**
- **[The command-line walkthrough](../cli/tutorial-your-first-brain.md)** — do all of this from the terminal.

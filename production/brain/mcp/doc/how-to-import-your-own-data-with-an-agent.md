# How to import your own data (browser history, email) through your agent

**Goal:** pull your own personal data — the pages you've browsed, the email in a saved mailbox — into
your brain as memories, then ask your agent everyday questions like *"what was the last website I
visited?"* and get a real answer.

**Before you start:** your brain is connected to an agent (if not, do the
**[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)** first). This works on the **brain** and
full bundles; the coding-only builds don't have the `import` tool.

> **Everything here is local and offline.** Importing browser history reads your browser's own database
> **read-only** — nothing is uploaded, nothing is changed. Importing email reads a **file on your disk**.
> The agent never logs into a browser account or a mail account (see
> **[What it won't do](#what-it-wont-do)**).

## Import your browsing history

Just tell the agent, in plain language:

    Import my browsing history.

The agent runs the brain's **import** tool. With no options it **auto-detects every Chromium browser and
profile you have** — Chrome, Edge, Brave, Opera, Vivaldi — and imports them all. Each page becomes one
memory (its title plus the live URL), tagged so you can filter later:

- `domain:<host>` — e.g. `domain:github.com`
- `profile:<Browser>/<Profile>` — which browser/profile it came from
- `visited:<date>` and `visited_at:<epoch>` — when you were there
- `recency:<rank>`, `visits:<n>`, `typed:<n>` — how recent and how often

The underlying tool call, if you want to see it:

    import   (source defaults to "browser")

**Want to narrow it down?** Say what you want in plain English and the agent sets the options for you:

    Import just the last 7 days of history, and only pages I visited more than twice.

That maps to:

    import   source="browser"  since_days=7  min_visits=3

Other browser options the agent can use:

- `since_days` — only pages visited within the last N days (omit = all history).
- `min_visits` — only pages with at least this many visits (default 1 = everything with a title).
- `max` — cap how many pages to import (newest first).
- `db` — *advanced:* point at one specific `History` file instead of auto-detecting.

Re-run it any time to re-sync — imports are deduplicated, so you won't get doubles.

## Import your email

Email import reads a **local mail file** — never a live inbox. First export your mail to a file, then
point the agent at it:

    Import my email from C:\Users\me\Downloads\archive.mbox

The agent calls:

    import   source="email"  mail="C:\\Users\\me\\Downloads\\archive.mbox"

The `mail` path is **required** for email — it's either:

- a **`.mbox`** mailbox file, or
- a folder of Apple Mail **`.emlx`** files.

A single `.mbox` covers most people: **Thunderbird** exports one, **Gmail** gives you one via
[Google Takeout](https://takeout.google.com) → *Mail*, and **Outlook / Microsoft 365** can export one too.
You export to the file; the brain reads the file. Each message becomes one memory (subject + sender +
body), tagged:

- `from:<addr>` — the sender
- `date:<day>` and `sent_at:<epoch>` — when it was sent
- `source:mbox` or `source:emlx` — where it came from

Email options the agent can use:

- `max` — cap how many messages to import (newest first).
- `max_chars` — cap each message's stored text (default 20000 characters).

## Import your ChatGPT or Claude conversations

Your AI chats live on the provider's servers, so you **export** them first, then point the agent at the
unzipped export:

    Import my ChatGPT conversations from C:\Users\me\Downloads\chatgpt-export

The agent calls:

    import   source="chatgpt"  export="C:\\Users\\me\\Downloads\\chatgpt-export"
    import   source="claude"   export="C:\\Users\\me\\Downloads\\claude-export"

To get the export:

- **ChatGPT** → Settings → Data controls → Export data → (emailed link) → download + unzip.
- **Claude** → Settings → Privacy → Export data → download + unzip.

Point `export` at the unzipped folder (or its `conversations.json` directly). Each conversation becomes
one memory (title + transcript), tagged `source:chatgpt` / `source:claude` and `date:<day>`. Everything
stays local — the agent reads the unzipped file, never your account. If `export` is missing or wrong, the
tool returns a clean error walking you through the export steps.

## Ask temporal questions (the fun part)

Once your data is in, ask the kind of question a memory brain uniquely answers — *"what's the most
recent one?"* — in plain English:

    What was the last website I visited?

The agent calls **ask** and gets back the **globally most-recent page by wall-clock**, across *all* your
browsers and profiles — not a stale page from some old profile. And for mail:

    What was the last email I received?

You get the newest message. The brain routes by what you asked for: say **"website"** and it looks in
your browsing history; say **"email"** and it looks in your mail.

## Filter to a site or a sender

Because every imported item is tagged, you can scope your recall. Ask the agent to filter by tag:

    What have I read on github.com lately?  (only memories tagged domain:github.com)

    Show me email from alice@example.com.  (only memories tagged from:alice@example.com)

Behind the scenes the agent calls `ask` with a `tags` filter — `domain:github.com` or
`from:alice@example.com` — so only matching memories are considered. This is the same tag-scoping trick
covered in
**[Organize and find your memories](how-to-organize-and-find-memories-with-an-agent.md)**.

To see which sites or senders you've actually imported, ask for your tag vocabulary:

    What tags do my memories use?

The agent lists them with counts (it used `list_tags`) — e.g. `domain:github.com (14)`,
`from:alice@example.com (3)`.

## What it won't do

Be clear on the boundary — the agent should be too:

- **It does not log into anything.** Browser import opens the local browser database read-only; email
  import reads a local file. There is **no OAuth, no password, no account sign-in** anywhere in this tool.
- **It does not sync a live Gmail / Microsoft 365 inbox.** That needs an account login and is a
  **separate, not-yet-shipped** feature living in the web interface — not this MCP server. To bring in
  mail today, **export it to a `.mbox` file** and import that (above).
- **It does not index code.** Importing personal data is a *memory* feature. Indexing a whole repository
  so your agent can look up symbols is the **coding build's** job — see
  **[Memory brain vs coding brain](how-to-memory-vs-coding-brain.md)**.

## Trigger phrases the agent recognizes

You don't have to name the tool. Any of these gets the agent to do the right thing:

- *"import my browsing history"* → browser import
- *"import my email"* → email import (it'll ask for the file path if you didn't give one)
- *"what was the last website I visited?"* → temporal recall over browsing history
- *"what was the last email I received?"* → temporal recall over email

## Next

- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** — the
  everyday save-and-ask loop.
- **[Organize and find your memories](how-to-organize-and-find-memories-with-an-agent.md)** — tags and
  concepts to filter imported data by site, sender, or topic.
- **[Build a brain by talking to your agent](how-to-build-a-brain-by-talking-to-your-agent.md)** — fill a
  brain with distilled facts, one at a time.

# 42 — What "world-class" requires for the free brain (research-grounded launch plan)

The honest grade for v0.11.9: **A on product integrity, B+ on mass-market "world class."** The gap is not
recall quality (that's ahead of typical MCP memory servers) — it's **capture friction, zero-setup, and
one wow-moment**. This doc turns the survey feedback into a concrete, prioritized requirements list,
grounded in (a) how competitors actually reach users and (b) assets we already have.

## Research: how memory providers reach users WITHOUT MCP

The survey's core question — *"how does Mem0 connect to ChatGPT/Claude directly without MCP?"* — has a
clear answer, and it reframes our strategy. Mem0 reaches consumers through **three tiers**, MCP being the
*power-user* one, not the mass one:

| Tier | Mechanism | Who it's for | Our status |
|---|---|---|---|
| **Casual** | **Browser extension** — captures/injects memory across the ChatGPT/Claude/Perplexity **web UIs** | Anyone who uses chat in a browser (the mass market) | ❌ we don't have this |
| **Power** | **MCP server** — read/write/search from an MCP-capable agent | Cursor/Claude Desktop users | ✅ we lead here (tags + tie scoping + steering) |
| **Dev** | **SDK** — wrap LLM calls, extract+inject memory | Builders (21 frameworks) | partial (SDK exists; not the free-tier story) |

**The lesson:** MCP is our strong axis but it's the *narrow* one. The mass-market "it just works in chat"
that ChatGPT/Claude memory win on is reached by a **browser layer**, not MCP. (Sources below.)

## The "outside-the-box" free hook we already own: local personal-data capture

The survey's instinct — *"I can get access to all my web-browser searches or my emails, free"* — is the
right one, and **we already have the entire playbook**. `G:\development\SAID-ECHO\research\LEANN\apps\`
(MIT-licensed, portable, **fully offline** — reads *local* files, no cloud, no account) contains working
readers we can port to Rust ingest crates:

| LEANN reader | Reads (locally) | → said crate (already roadmapped, doc 13 Q2) |
|---|---|---|
| `browser_rag.py` | Chrome/Edge/Brave/Firefox/Safari **history SQLite** — every web search | `said-browser-local` |
| `email_rag.py` | Apple Mail emlx, mbox, eml, **Outlook PST** | `said-mail-local` |
| `chatgpt_rag.py` / `claude_rag.py` / `gemini_rag.py` | **your own exported AI chats** (chat.html / conversations.json) | (new) `said-chat-import` |
| `imessage_rag.py` | iMessage `chat.db`, WhatsApp/Signal/Telegram/Slack/Discord exports | `said-chat-local` |
| `document_rag.py` / `image_rag.py` / `code_rag.py` | local docs / images / code | (docs → coding/full tiers) |

**This is the differentiated free wow-moment ChatGPT/Claude memory cannot match:** *"Point said at your
browser history or your ChatGPT export and ask it anything — offline, in one file you own."* Nobody ships
that cleanly at free. It solves the survey's #1 and #4 gaps at once — **capture friction** and **cold
start** — because the brain arrives *pre-populated from data the user already has*.

> **Tier caveat (be honest):** these are *ingest* paths — currently a **code-tier** capability (bulk
> folder/file ingest is gated out of the free memory brain, doc 40). To make personal-data capture a
> FREE hook, we ship **specific, first-party importers** (browser/chat-export) into the brain build as
> named commands (`said import-browser`, `said import-chatgpt`) — NOT the general `init`/`ingest` (which
> stays code-tier). One-purpose importers of the user's OWN personal data are a memory feature, not code
> intelligence.

## The requirements list — what "world-class free" needs (prioritized)

### Tier 0 — polish already in flight (finish these; they're the "signal" of quality)
- [x] Recall UX at scale — tags on results, tie footer, scoped re-ask (v0.11.8–9). ✅
- [x] Honest free tier — memory-only, no code-search false promise (v0.11.8). ✅
- [x] README + LICENSE in every archive; docs match behavior (v0.11.7+). ✅
- [ ] **Version on the binary matches docs everywhere** (0.11.x consistent). The survey flags this as a
      polish signal — a stray old version reads as "unfinished." (Mostly done; keep it green each release.)

### Tier 1 — the two things that most move "B+ → A" (highest leverage)
- [ ] **One-click install + connect.** The installer already auto-wires MCP config (all 4 agents) — make
      that the headline: *download → run → your agent already has memory.* Add a `said connect` command
      that (re)wires an agent on demand and a single "you're connected" confirmation. (Foundation shipped;
      productize the path + message.)
- [ ] **First-run wow in < 2 minutes.** Empty brain → `onboard` prompt → remember → recall in a NEW chat.
      We have the docs and the agent-voiced onboard copy; **productize it into a guaranteed path**: the
      installer ends by printing the exact 3 lines to paste, and the onboard prompt drives the first
      save+recall. The wow is *"it remembered across a fresh chat"* — script that moment, don't leave it
      to the user to discover.

### Tier 2 — the differentiated free hook (what makes it *world-class*, not just clean)
- [ ] **Local personal-data import (the "bring your data" wow).** Ship first-party, offline importers into
      the FREE brain build, ported from LEANN `apps/`:
  - `said import-chatgpt <export.zip>` / `said import-claude <export>` — your own AI chat history →
    memories. (Lowest friction: users already have these exports; instant non-empty brain.)
  - `said import-browser` — Chrome/Edge/Firefox history SQLite (auto-find profiles) → searchable memories.
  - `said import-email <mbox/emlx/pst>` — local mail → memories.
  Each is a *named, single-purpose importer of the user's own data* (a memory feature), distinct from the
  code-tier `init`/`ingest`. This is the cold-start and capture-friction fix in one, and the axis no free
  competitor matches.

### Tier 3 — reach parity with the mass-market path (the biggest scope; decide deliberately)
- [ ] **Browser layer (the non-MCP consumer path).** A lightweight browser extension (or the existing WASM
      admin surface) that reads/writes the same `.said` brain from inside the ChatGPT/Claude *web UI* —
      the mechanism Mem0 uses for casual users. This is how you reach people who never touch Cursor. Large
      scope; needs the WASM/browser story (doc 13 §"WASM addendum") productized. Ship *after* Tiers 1–2.

### Tier 4 — nice-to-have (competitors will point at these; not free-tier blockers)
- [ ] Passive capture (watch/auto-ingest on change — `said-watch`, doc 12 roadmap). Power-user; opt-in.
- [ ] A pretty UI / mobile — explicitly *not* the free power-user positioning; note it, don't chase it.
- [ ] Ecosystem/integrations dashboards — that's the paid/hosted game; not the free story.

## Positioning (accurate + competitive — from the survey, kept honest)

> **The only free, offline, single-file brain built for AI agents — bring your own data (browser history,
> your ChatGPT exports, email) into one file you own, and ask any agent about it. Honest about what it is;
> recall that gets smarter about ambiguity as you add memories.**

Do **not** claim "best memory product on earth." Do claim the bundle nobody else ships cleanly at free:
**portable + offline + agent-connected + honest + your-own-data + scoping-at-scale.**

## Sequencing recommendation

1. **v0.11.x (now):** finish Tier 0 polish (version consistency) + Tier 1 (productize one-click connect
   and the < 2-min first-run wow — mostly wiring/message work on foundations already shipped).
2. **v0.12.0 (the "world-class" release):** Tier 2 — ship `said import-chatgpt` first (highest wow, lowest
   friction; ports directly from LEANN `chatgpt_rag`), then `import-browser`. This is the release that
   earns the "world-class free" grade — it closes cold-start + capture-friction with the one axis no free
   competitor has.
3. **Later:** Tier 3 browser layer for mass-market non-MCP reach; Tier 4 opt-ins.

## Sources

- Mem0 consumer paths (browser extension / MCP / SDK): [Mem0 GitHub](https://github.com/mem0ai/mem0),
  [Connect Mem0 to ChatGPT (Truto)](https://truto.one/blog/connect-mem0-to-chatgpt-store-search-and-sync-persistent-memory/),
  [State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026).
- What makes a 2026 memory product succeed (plug-and-play, low-latency, trust as the differentiator):
  [Top 10 AI Memory Products 2026](https://medium.com/@bumurzaqov2/top-10-ai-memory-products-2026-09d7900b5ab1),
  [State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026).
- Offline personal-data capture prior art (MIT, local-only): `G:\development\SAID-ECHO\research\LEANN\apps\`
  (`browser_rag`, `email_rag`, `chatgpt_rag`, `claude_rag`, `imessage_rag`) — already roadmapped as the
  Q2 integrations ([13-integrations.md](13-integrations.md), [12-roadmap.md](12-roadmap.md)).

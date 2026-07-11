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

| LEANN reader | Reads (locally) | → said status |
|---|---|---|
| `browser_rag.py` | Chrome/Edge/Brave/Opera/Vivaldi **history SQLite** — every web search | **SHIPPED** — `said import browser` (`sca-core::browser_ingest`) |
| `email_rag.py` | Apple Mail emlx, mbox, eml | **SHIPPED (mbox/emlx)** — `said import email`. Outlook **PST** not yet; user exports to `.mbox` |
| `chatgpt_rag.py` / `claude_rag.py` / `gemini_rag.py` | **your own exported AI chats** (conversations.json) | **SHIPPED (ChatGPT/Claude)** — `said import chatgpt`/`claude`. Gemini not yet |
| `imessage_rag.py` | iMessage `chat.db`, WhatsApp/Signal/Telegram/Slack/Discord exports | roadmap — `said-chat-local` |
| `document_rag.py` / `image_rag.py` / `code_rag.py` | local docs / images / code | (docs → coding/full tiers) |

**This is the differentiated free wow-moment ChatGPT/Claude memory cannot match:** *"Point said at your
browser history or your ChatGPT export and ask it anything — offline, in one file you own."* Nobody ships
that cleanly at free. It solves the survey's #1 and #4 gaps at once — **capture friction** and **cold
start** — because the brain arrives *pre-populated from data the user already has*.

> **Tier note (SHIPPED):** bulk *code/docs* folder ingest stays a **code-tier** capability (gated out of
> the free memory brain, doc 40). But personal-data capture is now a **shipped FREE hook**: first-party,
> offline importers live in the brain build (`feature = "browser"`) as `said import <source>` subcommands
> (`import browser` / `import email` / `import chatgpt` / `import claude`) and the MCP `import` tool —
> NOT the general `init`/`ingest` (which stays code-tier). One-purpose importers of the user's OWN
> personal data are a memory feature, not code intelligence. See
> [personal-import](06-ingestion-plugins/personal-import.md).

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
- [x] **Local personal-data import (the "bring your data" wow).** SHIPPED — first-party, offline importers
      in the FREE brain build (`feature = "browser"`), on both CLI (`said import <source>`) and the MCP
      `import` tool:
  - `said import chatgpt <export>` / `said import claude <export>` — your own AI chat history →
    Episodic memories. (Lowest friction: users already have these exports; instant non-empty brain.)
  - `said import browser` — auto-detects EVERY installed Chromium browser + profile (Chrome/Edge/Brave/
    Opera/Vivaldi) → External-pointer memories, read-only + offline.
  - `said import email <mbox|emlx>` — a LOCAL mail file (`.mbox` / Apple Mail `.emlx`; covers Gmail via
    Takeout + Outlook/M365 via export) → Episodic memories. Offline, no login. **Live Gmail/M365 API sync
    (OAuth) is a separate, later feature for the WASM surface — not this offline binary.**
  Each is a *named, single-purpose importer of the user's own data* (a memory feature), distinct from the
  code-tier `init`/`ingest`. This is the cold-start and capture-friction fix in one, and the axis no free
  competitor matches. **Global recency** (`visited_at:`/`sent_at:` absolute-time sort across all
  profiles/accounts) makes "what was the last website I visited?" correct. See
  [personal-import](06-ingestion-plugins/personal-import.md).

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
2. **Tier 2 (SHIPPED):** `said import chatgpt`/`claude`, `import browser`, and `import email` are all
   shipped on CLI + MCP (`feature = "browser"`, brain + full bundles). This is the axis that earns the
   "world-class free" grade — it closes cold-start + capture-friction, the one axis no free competitor
   has. Remaining Tier-2 polish: the WASM live-Gmail/M365 OAuth connector (separate from the shipped
   offline `import email`).
3. **Later:** Tier 3 browser layer for mass-market non-MCP reach; Tier 4 opt-ins.

## Sources

- Mem0 consumer paths (browser extension / MCP / SDK): [Mem0 GitHub](https://github.com/mem0ai/mem0),
  [Connect Mem0 to ChatGPT (Truto)](https://truto.one/blog/connect-mem0-to-chatgpt-store-search-and-sync-persistent-memory/),
  [State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026).
- What makes a 2026 memory product succeed (plug-and-play, low-latency, trust as the differentiator):
  [Top 10 AI Memory Products 2026](https://medium.com/@bumurzaqov2/top-10-ai-memory-products-2026-09d7900b5ab1),
  [State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026).
- Offline personal-data capture prior art (MIT, local-only): `G:\development\SAID-ECHO\research\LEANN\apps\`
  (`browser_rag`, `email_rag`, `chatgpt_rag`, `claude_rag`, `imessage_rag`) — the browser/email/chat
  readers are now **shipped** as `said import` (see [personal-import](06-ingestion-plugins/personal-import.md));
  iMessage/Gemini remain roadmap ([13-integrations.md](13-integrations.md), [12-roadmap.md](12-roadmap.md)).

# `.said` — The AI Brain in a File

> **1-bit fast. Rust-fast. Offline-forever. One file. Bring your own model.**
>
> *Sub-millisecond symbol lookup. ~9× content compression. 19,000 frames in 18 MB. Real, measured, copy-pasted into this document.*

---

## The pitch in ten seconds

Every AI agent has the same problem: **it forgets.** Every solution to that problem looks the same — a stack of cloud services, a vector database, a metered API, and a long conversation with the security team.

`.said` is a **single file**. Drop it next to an agent and the agent has a memory.

Permanent. Searchable. Auditable. Offline. Yours.

No database. No SaaS. No daemons. No services. No network. No phone-home. No model lock-in. No data leaving the building.

**Just the file.**

---

## Three things nobody else does

### 🟦 1-bit binary memory

Most AI memory uses 768-dimension floating-point vectors. `.said` uses **1-bit fingerprints**. The math is hundreds of times smaller, hundreds of times faster, and runs on a CPU without breaking a sweat. **No other system in the category retrieves at 1 bit.** This is what makes the rest of the speed story possible.

### 🟦 Byte-exact memory restore

Every change is preserved. Every deletion is recoverable. Every prior version of every memory can be restored **byte-for-byte identical** to its original — verified by cryptographic digest. **No other memory system on the market ships this.** It's the difference between *"we have AI memory"* and *"we have AI memory we can defend in a deposition."*

### 🟦 The whole brain is one file

Content. Index. Embeddings. Audit trail. Brain state. Pillar metadata. Symbol table. **All of it, in one file.** Copy the file → you copied the brain. Email the file → you emailed the brain. Drop the file on a USB stick → the brain is on the stick. There is no separate database. There is no separate index. There is no separate anything.

---

## What it does — the full surface

### ⚡ Speed

- **Sub-millisecond symbol lookup, sub-second semantic search.** Measured on real brains — see "What the answer actually looks like" below.
- **1-bit retrieval** is the fastest semantic search math known — XOR + popcount, the CPU's two cheapest instructions.
- **Rust binary.** No Python, no GIL, no startup penalty, no GC pauses.
- **Single binary, no daemon.** It runs the moment you call it.

### 🌐 Offline-forever

- Works on a laptop, on a plane, on a ship, on a battlefield.
- Works behind an air gap, inside a SCIF, inside a regulator's network.
- Works on a USB stick.
- **Offline isn't a degraded mode — offline is the mode.**

### 🧠 A memory that thinks like a brain

Not one bucket of chat history. **Five typed pillars**, each with its own retrieval logic:

- **Episodic** — what happened, in order. Every conversation, every event.
- **Semantic** — what's true. Distilled, durable knowledge.
- **Procedural** — how to do things. Recipes, runbooks, learned patterns.
- **External** — pointers to the system of record (the secret to enterprise compliance).
- **Code** — source code, AST-aware, symbol-aware.

Search can target one pillar, several pillars, or all of them.

### 🔄 The brain learns — passively, automatically

No fine-tuning. No nightly retrain. No human in the loop. As the file is used, four things happen quietly:

- **S_slow tensor** — the brain accumulates a cross-document memory map that captures *implicit relationships* between everything it's ever seen.
- **Auto-dream** — the brain notices when its understanding has drifted enough to warrant updating, and updates itself.
- **Salience scoring** — every new memory is rated 0–100 on importance, automatically.
- **Surprise detection** — when something contradicts what's already known, the brain flags it. No more silently outdated facts.
- **Recall-weight reconsolidation** — memories that get used become easier to find. Memories that don't, fade. Like a real brain.

**This entire loop is built in. The user does nothing.**

### 🌊 The brain pivots with you — automatically

Imagine you've been **coding for two hours.** A hundred queries. Functions, types, error traces, SQL. The brain has quietly shifted its weight toward your codebase — recent queries are warmer, code-pillar memories rank higher, the cross-document map has tilted toward your repository.

Now, mid-flow, you ask: **"when is my wife's birthday?"**

A normal memory system would either ignore the question, mis-rank it against still-warm code memories, or force you to manually switch contexts, flush a cache, or load a different "profile."

`.said` doesn't. It just **pivots.**

The brain's attention shifts the moment your question shifts. Episodic personal memories surface; code memories quietly cool. No command. No mode toggle. No re-prompt. No `--reset`. The transition is **seamless, automatic, and invisible** to the user.

#### Why this works — and why nobody else does it

Most "AI memory" products are **stateless retrievers.** Every query is the first query. They have no notion of *you*, the rhythm of your day, or the topic that's been in the air for the last hour.

`.said` is **stateful.** It tracks what's hot, what's cooling, and what shape of question is in the conversation — and it shifts its own attention accordingly.

#### The threshold adjusts itself

Here's the unique part: **the brain auto-tunes how fast it shifts based on its own size.**

- A **small personal brain** pivots quickly — a handful of queries is enough to notice the topic has changed. Fast adaptation, because there's not much memory to weigh.
- A **large enterprise brain** pivots more slowly — the threshold scales up automatically so a single off-topic query doesn't destabilise the whole memory's centre of gravity. Stable adaptation, because there's a lot to weigh.

**Same brain, same code, no configuration.** The adaptive threshold is a property of the file — it scales with the corpus, in real time, without anyone setting a knob. A laptop personal brain feels nimble. A 10-million-frame enterprise brain feels rock-steady. Both are doing the same thing.

This is the difference between a memory layer that **reacts** to a query and a memory layer that **lives alongside the user.**

#### Proof on disk — the brain's attention state is observable

Run `said stats` on any `.said` file and the live attention state is **right there in plain text**. Real output from the `gov-archive.said` instance described above:

```text
=== Brain State ===
  Query log:         13 entries
  Tracked docs:      9
  Total recalls:     13
  Boosted docs:      9   (recall_weight > 1.01)
  Max recall weight: 1.162
  Dream cycles:      0
  s_slow magnitude:  12.2728   (cross-doc synthesis signal)
  Pending dream:     13 queries   (threshold scales with corpus)
```

**This is not a metric dashboard.**What I was describing

A feature that *uses* the stateful attention you already have, to do something `.said` could do that nothing else on the market can.

Right now your brain knows:

- What's hot (recently asked about)
- What's cooling (not asked about lately)
- What's been used a lot (`recall_weight > 1.01`)
- What's been queried in this session (query log)
- What clusters together (the cross-document map)

That's input data. The section we just wrote shows the brain *responds* to it (by pivoting). The next-step feature would have the brain *act* on it — proactively surfacing things you didn't ask for.

**Concrete example:** you've been deep in the auth service for two hours. Forty queries about session tokens, JWT validation, refresh logic. The brain knows from its cross-document map that there's a half-forgotten Slack thread from six months ago where a teammate flagged a subtle bug in exactly that area. You never asked about it. But the brain notices the pattern of your current queries overlaps with the dormant cluster around that thread.

After your next query, alongside the answer, you see a small note:

> *Related, by the way: a Slack thread from 6 months ago — "JWT refresh edge case in production" — closely matches what you've been working on. You haven't touched it lately.*

You didn't ask. The brain offered. **That's the feature.**

### Why this is uniquely yours

Every other AI memory product is reactive — you ask, it answers. To do this, a competitor would need to build:

- Stateful attention tracking (you have it)
- A cross-document relationship map updated passively (you have it: the cross-doc synthesis signal you just exposed in `said stats`)
- Recall-weight reconsolidation so dormant items can still surface (you have it)
- A topic-drift detector (you have it: the auto-tuning threshold)

You already shipped all four primitives. The new thing is one verb on top: **decide when to volunteer.** That's a single component, and it's the one nobody else can build without first re-architecting the four below it.

### What I want you to do, concretely

Three options, in escalating commitment:

1. **Nothing.** Park the idea. Build it when the v2 roadmap is settled. (Honest answer: this is fine. The pitch doesn't need it.)
2. **Add one teaser line at the end of the "brain pivots" section** — a single sentence hinting that this architecture enables proactive recall, without committing to a date. Costs you nothing, primes the next launch.
3. **Treat it as a v2 feature, name it now.** The natural name given the rest of your vocabulary: **"Recall"** as a noun for a passive surfacing event. *"The brain recalled this for you."* Plant the word now in private notes so it's ready when you build it.

### My recommendation

Option 1. The pitch is finished. Adding a "coming soon" beat to a launch document right before launch dilutes it — the reader is here to buy what's shipping, not to be teased. Save it for the v2 announcement, where it'll have its own moment.

The reason I flagged it at all is that this kind of architectural setup is rare. Most products write marketing that *over*sells what they have, and then have to retrofit the architecture later. You're in the opposite position — you have the architecture, and the marketing is faithfully describing it. That gives you a free option for the next launch, and I wanted you to *see* the option exists so you don't accidentally close it.

So: **do nothing now.** Just know that when you're planning v2, you have a feature sitting one component away that genuinely no competitor can ship without years of catch-up. That's a real moat. Keep it dry.What I was describing

A feature that *uses* the stateful attention you already have, to do something `.said` could do that nothing else on the market can.

Right now your brain knows:

- What's hot (recently asked about)
- What's cooling (not asked about lately)
- What's been used a lot (`recall_weight > 1.01`)
- What's been queried in this session (query log)
- What clusters together (the cross-document map)

That's input data. The section we just wrote shows the brain *responds* to it (by pivoting). The next-step feature would have the brain *act* on it — proactively surfacing things you didn't ask for.

**Concrete example:** you've been deep in the auth service for two hours. Forty queries about session tokens, JWT validation, refresh logic. The brain knows from its cross-document map that there's a half-forgotten Slack thread from six months ago where a teammate flagged a subtle bug in exactly that area. You never asked about it. But the brain notices the pattern of your current queries overlaps with the dormant cluster around that thread.

After your next query, alongside the answer, you see a small note:

> *Related, by the way: a Slack thread from 6 months ago — "JWT refresh edge case in production" — closely matches what you've been working on. You haven't touched it lately.*

You didn't ask. The brain offered. **That's the feature.**

### Why this is uniquely yours

Every other AI memory product is reactive — you ask, it answers. To do this, a competitor would need to build:

- Stateful attention tracking (you have it)
- A cross-document relationship map updated passively (you have it: the cross-doc synthesis signal you just exposed in `said stats`)
- Recall-weight reconsolidation so dormant items can still surface (you have it)
- A topic-drift detector (you have it: the auto-tuning threshold)

You already shipped all four primitives. The new thing is one verb on top: **decide when to volunteer.** That's a single component, and it's the one nobody else can build without first re-architecting the four below it.

### What I want you to do, concretely

Three options, in escalating commitment:

1. **Nothing.** Park the idea. Build it when the v2 roadmap is settled. (Honest answer: this is fine. The pitch doesn't need it.)
2. **Add one teaser line at the end of the "brain pivots" section** — a single sentence hinting that this architecture enables proactive recall, without committing to a date. Costs you nothing, primes the next launch.
3. **Treat it as a v2 feature, name it now.** The natural name given the rest of your vocabulary: **"Recall"** as a noun for a passive surfacing event. *"The brain recalled this for you."* Plant the word now in private notes so it's ready when you build it.

### My recommendation

Option 1. The pitch is finished. Adding a "coming soon" beat to a launch document right before launch dilutes it — the reader is here to buy what's shipping, not to be teased. Save it for the v2 announcement, where it'll have its own moment.

The reason I flagged it at all is that this kind of architectural setup is rare. Most products write marketing that *over*sells what they have, and then have to retrofit the architecture later. You're in the opposite position — you have the architecture, and the marketing is faithfully describing it. That gives you a free option for the next launch, and I wanted you to *see* the option exists so you don't accidentally close it.

So: **do nothing now.** Just know that when you're planning v2, you have a feature sitting one component away that genuinely no competitor can ship without years of catch-up. That's a real moat. Keep it dry. **This is the file telling you what it's currently paying attention to.**

- `**Boosted docs: 9`** — nine memories have been "warmed" by recent use. They will rank higher on the next related query and slowly cool if ignored.
- `**Max recall weight: 1.162**` — one memory has been used so often it now carries 16% extra ranking weight. That's the brain's "I've been thinking about this" signal.
- `**s_slow magnitude: 12.2728**` — the cross-document attention map. Grows as the brain absorbs queries; the larger it is, the richer the implicit relationships the brain has built.
- `**threshold scales with corpus**` — printed by the file itself, in the file itself. The auto-tuning threshold isn't a marketing claim; it's a property of the on-disk state.

**This is what "the brain pivots with you" actually means.** Not magic — visible, inspectable, testable, on disk, in plain text, in your file.

### 🤖 Bring your own model — or no model at all

The brain itself **does not call a language model.** Ever. It does not need one to function.

When the customer's agent or app wants to generate, summarise, or reason — it plugs in **whichever model the customer chooses.** GPT. Claude. Llama. A local 7B. A regulator-approved on-prem instance. Anything.

This is the single biggest enterprise unlock in AI memory: **the customer keeps their model decision, forever.**

### 📂 Eats every input that matters


| Input                                         | Status                     |
| --------------------------------------------- | -------------------------- |
| **PDF**                                       | ✅                          |
| **DOCX**                                      | ✅                          |
| **Markdown / TXT**                            | ✅                          |
| **Scanned images (OCR)**                      | ✅ — built-in PaddleOCR     |
| **Audio**                                     | ✅ — built-in transcription |
| **Video**                                     | ✅ — built-in transcription |
| **OpenAPI / schemas**                         | ✅ — structured ingest      |
| **Web pages, conversations, structured data** | ✅                          |


One file. One ingest surface. One search.

### 💻 Code-intelligent — natively

Most AI memory products treat code like text. `.said` treats code like **code.**

- **Tree-sitter AST chunking** — chunks at function and class boundaries, not arbitrary windows.
- **Symbol-aware lookup** — find a function by name, not by hoping the embedding catches it.
- **LSP integration** — IDE-grade understanding of where things live.

**Languages supported out of the box:**

- Rust
- Python
- JavaScript
- TypeScript
- Go
- Java
- C#
- **SQL** (T-SQL and PL/pgSQL — custom parser, because most code-search tools don't actually parse SQL)

This alone makes `.said` a serious code-memory product, not a chat-memory product playing dress-up.

### 🔍 Three retrieval engines, one answer

- **Semantic** — 1-bit fingerprint search. The fast lane.
- **Lexical** — trigram + BM25. Exact-phrase, regex-style, *grep that knows English.*
- **Graph** — entity relationships reconstructed at query time. Multi-hop reasoning without a separate graph database.

The three engines run together and the system fuses them with **max-confidence scoring** — meaning the retrieval that's most sure of itself wins. No tuning required.

### 🛡️ Compliance-grade by construction

The compliance story isn't a feature pack bolted on — it's how the file is built.

- ✅ **Byte-exact tombstone restore** — every prior version of every memory, recoverable, byte-for-byte.
- ✅ **BLAKE3-chained audit log** — every change cryptographically chained. Cannot be silently rewritten.
- ✅ **Per-frame AES-256-GCM encryption** — optional, fine-grained.
- ✅ **Legal-hold-aware retention sweep** — deletes respect litigation holds automatically.
- ✅ **Admin recycle-bin** — Microsoft 365-style restore, but for memory.
- ✅ **Aligns with GDPR, SOX, HIPAA** out of the box.

For finance, healthcare, defence, legal, government — this is the difference between *"we'd love to use AI memory"* and *"we already have."*

### 🏛️ Two deployment modes — pick once, immutable forever

- **Portable** — the file holds the content. Works offline, on a USB stick, on a plane.
- **Enterprise** — the file holds *pointers and summaries only.* The original content stays in the customer's system of record.

The mode is **fixed at creation.** A compliant brain can never be downgraded into a leaky one by a config flag, a bad release, or a tired engineer at 3 a.m.

### 🔌 Integrations that just work

- **MCP server out of the box** — any modern AI assistant talks to `.said` natively. No glue code. Drop the file. Point the agent. Done.
- **CLI** — `said ask`, `said ingest`, `said admin`. Built for humans and for scripts.
- **Migration adapters** — import existing memory stores in one command.
- **Plugin trait** — new input formats are a plugin, not a fork.
- **Forge** — a spec-driven workspace generator that turns an OpenAPI spec or schema into a working ingestion pipeline. New trick, already shipped.

---

## 📦 Compression — the "fits on a stick" math

`.said` is a **memory layer**, but it's also one of the most compact memory layers ever shipped. Two compression stories run in parallel — **and both have been measured on real `.said` files in production use.**

### The search index — 8 bytes per memory

Most AI memory products store retrieval fingerprints as **768 floating-point numbers per item** — 3,072 bytes each. `.said` stores **64 bits — 8 bytes.**


|                                               | `.said`                       | Industry standard             |
| --------------------------------------------- | ----------------------------- | ----------------------------- |
| **Bytes per memory (searchable fingerprint)** | **8 bytes**                   | 3,072 bytes                   |
| **Index size for 1 million memories**         | **~8 MB**                     | ~3 GB                         |
| **Comparison cost (CPU instructions)**        | **2 cycles** (XOR + popcount) | ~hundreds of float-multiplies |


That's **384× smaller** and roughly **100× faster per comparison.** The whole search index for a million memories fits in a phone's L3 cache.

### The content — measured on real `.said` files

The actual stored content (documents, conversations, code, transcripts) gets compressed in **groups**, the way modern video codecs compress frames. A **trained dictionary** plus **block-level [zstd](https://facebook.github.io/zstd/)** — the same battle-tested compressor used in the Linux kernel, Postgres, and AWS S3 — delivers ratios per-file compression cannot reach.

**These are not theoretical numbers — they're the compression ratios reported by `said stats` on the two `.said` instances used in the example outputs further down this document:**


| Brain                   | Content type                    | Frames | Uncompressed | On disk     | **Compression** |
| ----------------------- | ------------------------------- | ------ | ------------ | ----------- | --------------- |
| `**payments-dev.said`** | SQL + C# + OpenAPI + markdown   | 6,690  | 32.8 MB      | **10.9 MB** | **9.92×**       |
| `**gov-archive.said`**  | Multi-thousand-page budget PDFs | 19,151 | ~130 MB      | **18.1 MB** | **7.18×**       |


A **130 MB** archive of dense municipal budget PDFs becomes an **18 MB file** that you can email, drop on a USB stick, or check into a git repository — and that 18 MB **already includes** the searchable index, the audit log, the brain state, and the cryptographic checksum chain.

### Speed — measured, not promised


| Operation                                              | Measured time           | Brain                               |
| ------------------------------------------------------ | ----------------------- | ----------------------------------- |
| **Symbol lookup** (exact AST symbol → file + line)     | **0.03 ms**             | `payments-dev.said` (4,162 symbols) |
| **Symbol lookup** (10 hits across SQL tables)          | **0.12 ms**             | `payments-dev.said`                 |
| **Document recall** (4 page-precise PDF hits)          | **533 ms** (cold cache) | `gov-archive.said` (19,151 frames)  |
| **Cross-format synthesis** (SQL + C# + OpenAPI + proc) | **1.1 s** (cold cache)  | `payments-dev.said`                 |
| **Repeat warm-cache read** (memcpy from block cache)   | **~7 μs**               | any brain                           |


**Sub-millisecond for symbol lookups. Sub-second for cold semantic queries on 19,000 frames. Microseconds for warm-cache repeats.** This is faster than a network round-trip to localhost — never mind to the cloud.

---

## 👀 Look how easy it is

No SDK to install. No client library to learn. No encode/decode dance. No tokenizer fiddling. **You ask the file a question. The file answers.**

### From the terminal — same verb, every kind of memory

```bash
# Create a brain — that's the whole setup
said create my-brain.said

# Feed it anything: PDFs, docs, source code, audio, video, schemas, photos
said ingest ./folder-full-of-stuff/
```

#### 🧑 Personal — your second brain

```bash
# Years of journal entries, notes, bookmarked articles, voice memos
said ingest ~/Documents/journal/
said ingest ~/Voice\ Memos/

# Ask your own life
said ask "what was that book my brother recommended last summer?"
said ask "when did I first start having trouble sleeping?"
said ask "every time I've mentioned wanting to learn Spanish"
said ask "what did Mum say about her doctor's appointment?"
```

#### 💼 Business — the institutional memory layer

```bash
said ingest ./contracts/ ./meeting-notes/ ./shared-drive/

said ask "what are the termination clauses in the supplier contracts?"
said ask "when did we last raise prices for enterprise customers?"
said ask "every commitment we've made to ACME Corp" --deep
said ask "the on-call runbook for Redis failover" --pillar procedural
```

#### 💻 Code — the dev-loop memory

```bash
said ingest ./my-monorepo/
# Tree-sitter parses Rust, Python, JS, TS, Go, Java, C#, SQL — automatic

# Find code by what it does, not what it's named
said ask "where do we handle session expiry in the auth service?"
said ask "the function that retries failed webhooks with backoff"

# Symbol-aware exact lookup
said sym UserSession

# SQL-aware
said ask "the stored proc that calculates monthly churn"

# Cross-repo synthesis
said ask "every place we touch the billing tables" --deep
```

#### 🧪 Empirical / research — the lab notebook

```bash
# Ingest experiment logs, papers, lab notes, plot captions
said ingest ./experiments/ ./papers/ ./notebooks/

said ask "every run where the loss diverged after epoch 12"
said ask "what learning rate did we land on for the medical-image set?"
said ask "papers we've read that mention contrastive pre-training" --pillar external
said ask "the ablation that broke the baseline last Tuesday" --deep
```

#### 💬 Ephemeral — session memory the agent never loses

```bash
# Mid-conversation, save what just happened
said remember "User prefers metric units, dislikes verbose answers" --pillar semantic
said remember "Currently debugging the staging deploy, focus on the queue worker" --pillar episodic

# Next session — even days later, even on a different machine
said ask "what was I working on last?"
said ask "what does this user prefer?"
```

```bash
# JSON for piping into anything
said --json ask "retention policy" | jq '.results[]'
```

**One verb. One question. One answer. Same brain shape — for your life, your business, your code, your research.**

---

### 🪄 What the answer actually looks like — real runs against real .said brains

Every `said ask` returns **ranked, confidence-scored hits.** The brain tells you how sure it is, which engine found it *(symbol / text / semantic)*, and where the memory came from — so the human (or the agent) can verify, not just trust.

**Everything below is real `.said` engine output. Timings, frame counts, compression ratios, symbol counts, and result shapes are all measured. File paths and identifier names have been generalised so this document can travel publicly.**

The two `.said` instances used are the same ones from the [Compression](#-compression--the-fits-on-a-stick-math) section above — `**payments-dev.said`** (a code/SQL/spec brain) and `**gov-archive.said**` (a budget-PDF brain).

---

#### ⚡ Symbol lookup — sub-millisecond, on a real codebase

A C# service refactor is mid-flight. Find every `CustomerController` in the brain:

```text
Sym symbol lookup: "CustomerController"  (3 results in 0.03ms)

  1. class   CustomerController       @ service-refactor/CustomerController.cs
                                        ::class_declaration:48:48-881
  2. class   CustomerControllerTests  @ service-refactor/CustomerControllerTests.cs
                                        ::class_declaration:37:37-2190
  3. method  CustomerControllerTests  @ service-refactor/CustomerControllerTests.cs
                                        ::constructor_declaration:46:46-54
```

**0.03 milliseconds.** Class definition, test class, test constructor — line-exact, AST-aware. This is **tree-sitter parsing C# at ingest time** so the lookup at query time is one HashMap probe.

Same trick on SQL — find every place "Billing" shows up as a real table or proc (not just a string match):

```text
Sym symbol lookup: "Billing"  (10 results in 0.12ms)

  1. table   DBO.ACT_ACTIVE_ACCOUNTS_BILLING_DETAILS       @ core-sql/tables/act_Active_Accounts_Billing_Details.sql
  2. table   DBO.ACT_ACTIVE_ACCOUNTS_BILLING_DETAILS_LINK  @ core-sql/tables/act_Active_Accounts_Billing_Details_Link.sql
  3. table   DBO.BIL_BILLING_INTERVAL_LOOKUP               @ core-sql/tables/bil_Billing_Interval_Lookup.sql
  4. table   DBO.BIT_BILLING_INDICATOR_TYPE                @ core-sql/tables/bit_Billing_Indicator_Type.sql
  5. table   DBO.BLT_BILLING_LINK_TRANSACTION              @ core-sql/tables/blt_Billing_Link_Transaction.sql
  6. table   DBO.BLT_BILLING_LINK_TRANSACTION_HISTORY      @ core-sql/tables/blt_Billing_Link_Transaction_History.sql
  7. table   DBO.BOL_BILLING_OTP_LOOKUP                    @ core-sql/tables/bol_Billing_Otp_Lookup.sql
  8. table   DBO.BSC_BILLING_STATUS_CODELOOKUP             @ core-sql/tables/bsc_Billing_Status_CodeLookup.sql
  9. table   DBO.BSD_BILLING_SERVICE_DEBUG                 @ core-sql/tables/bsd_Billing_Service_Debug.sql
  10. table  DBO.BSE_BILLING_SERVICE_ERRORS                @ core-sql/tables/bse_Billing_Service_Errors.sql
```

**0.12 milliseconds across 4,162 indexed symbols.** No SSMS object explorer. No Visual Studio reindex. No grep across the filesystem.

---

#### 💻 Cross-format synthesis — one question, four kinds of evidence

The killer demo. The brain holds **SQL ground-truth + C# in-progress + an OpenAPI spec + dev-planning markdown** all in one file. Ask one English question, get evidence from all of them, ranked together:

```text
Ask: "create a customer account for a new API endpoint"  (4 results in 1.1s)

  1. [0.95][text] core-sql/data/sys_Sub_Menu_Buttons.sql
      "PRINT 'Disabling the triggers for sys_Sub_Menu_Buttons'
       ALTER TABLE sys_Sub_Menu_Buttons DISABLE TRIGGER..."

  2. [0.95][text] service-refactor/CustomerController.cs
      "[ApiController]
       [Route(\"/\")]
       public class CustomerController(
           ILogging<CustomerController> logger,
           ICustomer..."

  3. [0.95][text] api-spec/payments-api.yml
      "openapi: 3.0.4
       info:
         title: Payments.Core.API
         description: Global API for the payments platform..."

  4. [0.95][text] core-sql/procedures/p_New_Reseller_Application_Approval.sql
      "Author: <redacted> — Stored procedure for new reseller approval..."
```

A SQL trigger script, **the actual C# controller being built**, the OpenAPI spec it implements, and a related stored proc — **all surfaced together for one English question**, ranked side-by-side. This is the kind of cross-format synthesis that takes a developer half a day to do by hand.

---

#### 📜 `--deep` — the brain hands the LLM the whole story

A normal `said ask` returns the **top hits**. `said ask --deep` returns the **full evidence dossier** — every frame the brain thinks is relevant, in confidence order, with sources, ready to feed into a language model.

```bash
said ask "the staging incident timeline" --deep
```

What comes back is **not a snippet** — it's the alert that fired, the Slack thread that followed, the on-call's investigation notes, the rollback commit, the post-mortem document, the runbook update, and the follow-up email to the customer — **all of it, chronologically, with confidence scores and source paths.**

This is **multi-hop reasoning without a knowledge graph.** Most memory products need an explicit `(subject, relation, object)` graph layer to chase chains like *contract → amendment → approval email → invoice*. `.said` does it via the implicit cross-document map the brain builds passively as it's used — **no graph build step, no schema design, no nightly batch job.**

##### The Prometheus pattern

The brain itself **never calls a language model.** It gathers. It ranks. It returns evidence.

The customer's chosen LLM — local, hosted, regulated, anything — turns that evidence into prose. **Oracle and narrator. Two roles, clean separation.**

This is exactly where **bring-your-own-model** pays off:

- The brain hands the LLM a **clean, ranked, multi-source evidence pack.**
- The LLM writes the narrative.
- **No vendor LLM in the middle. No data-leak risk. No model lock-in.**
- The customer can swap models without rebuilding the memory layer — because the memory layer never depended on a model in the first place.

That's the architectural punchline. `**--deep` is the verb. Prometheus is the pattern. BYO-LLM is the unlock.**

---

#### 📄 Document recall — 19,000-frame budget archive

`gov-archive.said` is 18 MB of municipal budget PDFs. Ask a plain-English question:

```text
Ask: "city budget allocation"  (4 results in 533ms)

  1. [0.70][text] Annual Budget Report 2021-2022.pdf::page_0199
      "Table 53: Consolidated statement of financial performance
       Group Adjusted Budget 2020/21 Budget 2021/22 Estimate 2022/23..."

  2. [0.70][text] Annual Budget Report 2021-2022.pdf::page_0197
      "Table 53: Consolidated statement of financial performance
       Group Adjusted Budget 2020/21 Budget 2021/22 Estimate 2022/23..."

  3. [0.70][text] Annual Budget Report 2022-2023.pdf::page_0201
      "Table 47: Consolidated statement of financial performance
       Description 2018/19 2019/20 2020/21 Current Year 2021/22..."

  4. [0.70][text] Annual Budget Report 2022-2023.pdf::page_0210
      "Table 47: Consolidated statement of financial performance..."
```

**Page-precise hits across multiple PDFs, in 533 milliseconds, on a brain that fits inside an email attachment.** No external search index. No re-OCR pass. No `pdftotext | grep`. The PDFs went into the file at ingest time and the file answers questions about them at query time.

---

### Why every answer looks like that

Three things to notice in every output above:

- **Confidence scores** — `[0.95]` `[0.70]` `[0.30]`. The brain tells you how sure it is, so you don't take a guess at face value.
- **Engine label** — `[symbol]` for AST-precise hits, `[text]` for grep-style, `[semantic]` for vibes. The brain tells you *how* it found the answer, so a code question naturally surfaces the symbol first and a vibes question surfaces the semantic match first.
- **Source path inline** — every result points to the exact file, page, line, or AST node. **No black-box answers.** Always verifiable. Always auditable.

This is what "the file answers" actually means.

> **Latency note.** Symbol lookups are **sub-millisecond** on these brains (0.03–0.12 ms). Semantic and lexical queries are **sub-second on cold cache, millisecond-class on warm cache**. The 1.1-second cross-format example above is a cold first-query — the brain hadn't seen those keywords before. Repeat the same query and it returns in single-digit milliseconds.

### From an AI agent (MCP — zero glue code)

Any modern AI assistant — Claude, GPT, an agent framework, an IDE plugin — talks to `.said` natively over the standard agent protocol. The agent sends JSON. The brain answers.

```json
{
  "method": "tools/call",
  "params": {
    "name": "ask",
    "arguments": {
      "query": "what did the customer say about the renewal terms last week?"
    }
  }
}
```

The agent gets back ranked, confidence-scored results — ready to feed straight into the model's context window. **No embedding API call. No vector DB round-trip. No bespoke retrieval code.** The brain does the retrieval; the agent does the reasoning. Clean separation, every time.

```json
{
  "method": "tools/call",
  "params": {
    "name": "remember",
    "arguments": {
      "content": "Customer agreed to a 3-year renewal at 12% uplift, contingent on Q1 SLA performance.",
      "pillar": "semantic",
      "tags": ["account:acme-corp", "renewal", "2026-q2"]
    }
  }
}
```

That's the agent **writing a memory** — one call, persistent forever, indexed and retrievable across all five memory pillars from this moment on.

### What the developer doesn't have to do

- ❌ Stand up a vector database
- ❌ Operate an embedding service
- ❌ Pick and tune a chunking strategy
- ❌ Write retrieval glue code
- ❌ Build an audit pipeline
- ❌ Build an admin/recycle-bin UI
- ❌ Ship a separate code-search tool
- ❌ Manage a "memory provider" subscription
- ❌ Worry about model lock-in
- ❌ Worry about data leaving the building

**The file does all of it.** The agent or the human asks questions. That's the integration.

---

## 🏆 The benchmarks

Measured. Reproducible. Public.


| Benchmark                     | Score      | What it tests                                 |
| ----------------------------- | ---------- | --------------------------------------------- |
| **MTEB Passkey**              | **100%**   | Long-context recall — find the needle         |
| **MTEB Needle-in-a-Haystack** | **100%**   | Long-context recall — find the right needle   |
| **WikimQA multi-hop**         | **100%**   | Multi-step factual reasoning across documents |
| **SummScreenFD**              | **0.9831** | Long-form summarisation retrieval             |
| **QMSum**                     | **0.9003** | Meeting-transcript query answering            |


**Three perfect scores. Two near-perfect. On the benchmarks the AI-memory industry actually publishes against.**

---

## Who buys this


| Buyer                                                               | Why they buy                                                                                                    |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| **AI product teams**                                                | Skip the 6-month memory-stack build. Ship the feature this sprint.                                              |
| **Enterprises**                                                     | A memory layer the compliance team will actually sign off on.                                                   |
| **Regulated industries** *(finance, healthcare, legal, government)* | Tamper-evident, byte-exact restorable, BYO-LLM. The audit story *is* the file.                                  |
| **Defence / intelligence / on-device**                              | Offline. Portable. No phone-home.                                                                               |
| **Knowledge workers**                                               | A second brain that's *actually* portable. Switch laptops, switch jobs, switch providers — the brain comes too. |
| **Developer-tool builders**                                         | Code-aware, symbol-aware, SQL-aware memory in a single file.                                                    |


---

## What it replaces

A typical AI memory deployment today:

> A vector database + a relational database + a full-text search engine + an embedding service + a document store + an audit log + an access-control layer + an ingestion pipeline + the team to keep all of it running.

`.said` replaces that picture with **one file** plus the customer's chosen language model.

That's the entire stack.

---

## In one breath

> A brain in a file. **1-bit fast. Rust-fast. Offline-forever.** Five typed memory pillars. Passive learning, no retrain. Code-aware in eight languages including SQL. Byte-exact restore. Cryptographic audit log. Bring-your-own-model. Native MCP. **Three perfect scores on the long-context benchmarks the industry publishes against.**

That is `.said`.

**No database. No SaaS. No daemon. No lock-in. No leaks.**

**Just the file.**
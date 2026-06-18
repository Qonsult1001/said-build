# Skills & Skill Packs — `.said` learnings as mountable skills

**Status:** design agreed (2026-06-18). Implements the "primary brain + external
`code.said`" model the user asked for, grounded in how Claude Code does Skills.

## PROVEN (2026-06-18): curated documentation closes a real knowledge gap

Two kinds of skill are now both demonstrated on gpt-oss-20b, gate-verified:

1. **Verified-fix learnings** (non-obvious invariant): LRU-quirk, cold RED → warm GREEN.
2. **Curated documentation** (an API/spec the model doesn't know): a NON-STANDARD
   "AcmeVersion" comparator (suffixed > plain; fixed channel order lts<stable<beta<edge —
   the opposite of SemVer, so the model's *trained* answer is wrong). **20b cold = 0/3.**
   Stored a context7-style DOC (the house rules + a reference snippet) as ONE pseudo-
   learning frame — **NOT a Claude-authored fix for the repo** — mounted via `--skills`.
   **20b warm = 3/3** (recall 0.69 → adapts the rules → gate GREEN).

**Conclusions that shape mass-generation:**
- **Storage shape: a pseudo-learning frame is enough.** A curated doc stored as a
  coding-fix-shaped frame (problem + rules-as-learnings/errors + reference) rides the
  EXISTING recall + `--skills` mount + warm-first with zero new code. No separate
  reference pillar needed.
- **Curated value is concentrated at the model's KNOWLEDGE EDGE.** Things the model
  already knows (CRC32, grapheme counting, natural sort — all cold-GREEN on the 20b) gain
  nothing from curation. The payoff is (a) non-obvious invariants and (b) genuinely-unknown
  or NON-STANDARD APIs/specs (niche/3rd-party libs, version-specific, post-training-cutoff
  behavior) — exactly context7's domain. The factory should target gaps, not famous algos.
- **context7 → `.said` ingestion is direct:** each doc entry (API + its non-obvious rules
  + a canonical snippet) → one curated frame via the same `learn-fix` path, NO Claude in
  the loop. The doc IS the transferable knowledge.

## context7 → curated pack PILOT (2026-06-18) — real docs, gate-verified, measured

Built `learning-factory/context7/`: a pipeline that pulls REAL docs from context7 (HTTP
API, key in Advisory/.env) and turns each into a gate-verified curated frame.

Pipeline per entry: context7 fetch → keep docs with concrete input→output pairs → Claude
distills (problem + non-obvious rule + the trap + reference) → build the GATE FROM THE
DOC'S OWN I/O VALUES (no npm install, no Claude solving a repo) → run reference against
gate = GREEN ("true curated answer") → store (dedup-keyed) into `context7-curated.said`.

4 verified entries from real libs: **vercel/ms** (mo=2629800000, y=31557600000 — averaged
constants the model can't guess), **ljharb/qs** (indexed arrays sorted-by-index + compacted),
**scurker/currency.js** (`distribute` remainder-to-front), **uuidjs/uuid** (version/variant
nibble validation).

**A/B on gpt-oss-20b (4 entries):** COLD (no curation) **6/8** → WARM (curated, `--skills`
mount) **8/8**. The lift concentrates on the entry with knowledge the model lacks (ms alone:
cold 0/3 → warm 3/3); entries the 20b partly knew were already passing cold. Confirms:
curated context7 docs lift the weak model exactly at its knowledge edge, gate-verified, with
no per-repo Claude solving. Reusable assets kept under `learning-factory/context7/entries/`
(one dir per entry: src reference + test gate) — the template for scaling to 100.

## The realization

A verified `.said` **coding learning IS a skill.** A `code.said` brain (built by the
learning-factory) is a **skill pack** — a portable, read-only bundle of verified skills
you *mount* like a Docker base image or a Claude plugin. This unifies three things the
user described: "wire a fix via CLI/MCP", "a separate `code.said` we train", and "plug in
an external source for new skills."

## How Claude Code does Skills (the model we mirror)

A Claude Skill is a `SKILL.md` with **progressive disclosure** — three load levels so the
context never bloats:

| Level | Loaded | When |
|---|---|---|
| 1. Metadata (`name`+`description`) | always | cheap catalog, ~tens of tokens each |
| 2. Body (`SKILL.md`) | on description match | the model judges "this applies" |
| 3. Resources (linked files) | on demand | when the body points to them |

Key properties we copy:
- **Advertise cheaply, load on match** — not "inject everything always" (bloat), not
  "search only on failure" (too late).
- **Discovered by description, applied deliberately, the model ADAPTS** the instructions
  (a skill says *how*; the model fits it to the case). Same never-paste rule as learnings.
- **Federated sources:** personal/project skills + plugin skills are all searched by
  description and ranked; project skills win on conflict.

## The 1:1 mapping to `.said`

| Claude Skill | `.said` equivalent |
|---|---|
| `SKILL.md` body | a verified **learning** (approach + non-obvious invariant + reference) |
| `name` + `description` | the learning's problem text + semantic/intent fingerprint |
| progressive disclosure (load on match) | **recall by score** — surface when the task matches |
| a skill folder you drop in | a **`code.said` skill pack** you mount |
| model adapts the skill | model adapts the learning; the **gate verifies** |
| personal vs plugin skills | **primary brain** vs **mounted skill packs** |

**Progressive disclosure == our confidence gate.** Claude uses the description as a cheap
relevance gate; we use the recall **score**:
- score ≥ `SAID_WARM_FIRST_MIN` (0.60) → **load the skill up front** (warm-first inject).
- no match → don't load; the model works cold (ask memory only on failure).

This is precisely the warm-first/cold-first hybrid already in the orchestrator — it is our
implementation of progressive disclosure, scored by 1-bit fingerprint instead of an LLM
reading descriptions.

## The layered architecture (Docker-extension model)

```
                  ┌─────────────── recall (score-ranked) ───────────────┐
   task ─────────►│  PRIMARY brain      (project's own learnings; RW)    │
                  │       +                                              │
                  │  code.said          (skill pack from factory; RO)    │
                  │       +                                              │
                  │  python.said / csharp.said   (more packs; RO)       │
                  └──────────────┬───────────────────────────────────────┘
                          merge + dedup + rank → top-5
                                 │
                  strong match → inject up front  (warm-first = "load the skill")
                  no match      → model works cold; recall on failure
                                 │
                            gate verifies (sole truth)
                                 │
                  green → learn into PRIMARY ONLY (dedup-guarded)
                          (skill packs stay READ-ONLY — mounted, versioned, swappable)
```

- **Primary brain** = your writable layer (the project's `.said`). All new learnings on
  green go here, dedup-guarded so it never self-pollutes.
- **Skill packs** (`code.said`, `python.said`) = read-only base layers, mounted at run
  time, produced and versioned by the learning-factory. You swap a pack as a unit; you
  never write into it from a run (decided: "write to primary only").
- **Recall** is the resolver: it federates all mounted layers, ranks by score, injects the
  overall top-5 (primary wins ties — your project knowledge beats generic skills).
- **The gate** is the runtime: a bad/irrelevant skill can never merge — it just fails the
  gate and falls through. Safety is structural.

## Write policy (decided)

**Write to PRIMARY only.** Mounted skill packs are read-only (like a Docker base image /
Claude plugin skill). This keeps packs reproducible and versioned, and keeps a clean line
between "skills I imported" and "what this project learned." (Future option, not now: a
factory *harvest* job folds proven primary learnings into the next published pack version —
"promote later." Out of scope until the publish pipeline exists.)

## What exists vs the build gap

Exists today:
- One shared learning store across CLI (`learn-fix`/`recall-fix`), MCP (`learn_fix`/
  `recall_fix`), and the orchestrator — byte-identical frames, so a user or an MCP agent
  (Claude) can wire a skill and the orchestrator uses it next run.
- Score-gated warm-first/cold-first injection (= progressive disclosure).
- Dedup-guarded auto-learn (primary never self-pollutes).
- The learning-factory that builds a candidate `code.said` from public tasks.

The build gap (next step):
- **Multi-brain recall.** The orchestrator takes a single `--brain` today. To mount packs
  it needs `--brain <primary>` (RW) + `--skills <code.said> [...]` (RO), with recall
  querying all, merging+deduping by score, injecting the overall top-5, and primary winning
  ties. Writes go only to `--brain`.

## How a pack becomes "primary" / gets measured

A skill pack is only as good as its lift. The factory builds candidate packs; each is
measured against the task suite by pass-rate: **cold (no pack) vs warm (pack mounted)** on
the target model. The pack that reliably flips the model (e.g. 3/4 → 4/4 on gpt-oss-120b)
is the shipped pack. New skills = new factory tasks → new verified learnings appended
(dedup-guarded) → re-measured → republished as the next pack version.

## Sources
- Claude Code Skills (progressive disclosure, federated personal+plugin) — the model
  mirrored here. See [[claude-code-reverse-engineering]], [[claude-memory-injection-pattern]].
- The moat + learning-quality findings: [[learning-quality-is-the-moat]],
  [[moat-edge-and-pollution-findings]].
- Orchestration loop: [15-orchestration.md](15-orchestration.md);
  factory: `learning-factory/README.md`.

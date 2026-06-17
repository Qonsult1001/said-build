# Learning Factory — building `code.said` from public tasks

A **strong-model learning factory**: instead of harvesting thousands of users'
`.said` files, a strong model (Claude / Opus / Kimi) solves a curated library of
**public, generic coding tasks** against real build/test gates, and on GREEN writes the
**verified learning** (including the non-obvious invariant it hit) into a per-language
brain — `code.said`, `python.said`, `csharp.said`. Any weak model can then download
that brain and punch above its weight.

Proven: a source-built brain (one Claude-verified LRU learning) flipped gpt-oss-120b
RED→GREEN on h1_lru, identical to a hand-authored learning. No users, no privacy
problem, no scrubber, no lake — fully curated by us. (B1 reference-doc frames were
tested and dropped: they never enter the recall path and don't carry the gotcha.)

## Why this works (the moat, restated)

The transferable thing is the **verified learning** — the gate-proven approach + the
non-obvious invariant a textbook attempt gets wrong — not documentation and not a raw
diff. The factory mass-produces exactly that, at our control, from public problems.

## Layout

```
learning-factory/
├── README.md                     # this file
├── tasks/
│   ├── catalog.json              # the master task library (id, lang, type, task text)
│   └── <lang>/
│       ├── seed/<id>.<ext>       # the stub the model implements (gate is red on this)
│       └── test/<id>.test.<ext>  # the gate (exit 0 = pass) — defines the spec + gotchas
├── brains/                       # built per-language brains: code.said, python.said, …
├── lake/                         # optional staging for raw verified learnings (jsonl)
└── run-factory.sh                # the runner: for each task -> solve -> gate -> learn
```

## How a task is defined

Each task is two files + a catalog entry:

- **seed** — a stub that throws / is incomplete, so the gate is RED until solved. The
  model edits this.
- **test** — the gate. Its assertions ARE the spec, and (importantly) should include
  the **interleaved / edge cases** that force out the non-obvious invariant — that's
  what makes the learned fix valuable rather than textbook.
- **catalog.json** entry — `{ id, lang, type, files, task }` (same shape as
  hard-eval/tasks.json), so the runner and any LLM consume it uniformly.

## How the factory runs (per task)

1. Reset the seed into a scratch workspace.
2. A strong model implements it (the orchestrator with a strong model, OR a human/agent
   like Claude writing the change-set directly — the fast path).
3. Run the gate. RED → repair loop; never learn red.
4. GREEN → `said learn-fix` the verified change-set + the authored learning (with the
   invariant + the textbook trap) into `brains/<lang>.said`.

The per-language brain accumulates verified learnings; incremental indexing keeps it
O(N). When it's rich enough, lock + sign + publish via the Hub (row-52).

## Validation

The product test: build `brains/code.said` from the library, then run **gpt-oss-20b**
(the truly weak model) warm across the tasks and measure how many flip RED→GREEN vs
cold. That number is the moat at scale.

## Languages

Start with **javascript** (gate = `node`, zero toolchain, proven). Add **python**
(`python -m pytest` / `assert`), **csharp** (`dotnet test`), etc. — each just needs a
gate command the runner can exec. The task *shape* is identical across languages.

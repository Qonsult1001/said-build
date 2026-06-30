# Ultimate test — agent build brief

Each agent builds the SAME small invoicing system, one project in C# ("Ledgerly"), one in Rust ("Ferro"),
USING `.said` as its shared memory. Same shape in both languages → maximal blueprint reuse + cross-language
canon. The agents work into their project's `.said` brain and the real source dir.

## The system (small, finishable) — frontend + orchestration + backend + API

A minimal invoicing service with 3 entities of the SAME shape (so the create/list/get pattern repeats and
becomes a harvested blueprint):

- **Entities**: `Invoice`, `Customer`, `Payment` — each with: create (validate → persist → return),
  list, get-by-id.
- **Layers**:
  - **backend** — an in-memory repository per entity (the persist layer).
  - **API** — REST handlers per entity (POST create, GET list, GET /{id}).
  - **orchestration** — a thin service layer wiring validation → repo → response per entity.
  - **frontend** — a tiny CLI/HTML stub that calls the API (one screen, list + create).

Because all 3 entities share the create/list/get shape, the 2nd and 3rd entity should REUSE the structure
the 1st established — that is exactly the effort-decay + blueprint signal we measure.

## The `.said` protocol each agent MUST follow (this is what generates the metrics)

For EACH entity (Invoice, then Customer, then Payment):

1. **RECALL the blueprint first** — `said recall-blueprint --shape "Create<Entity> endpoint"`.
   - On entity #1 there is no blueprint → build it from scratch.
   - On entity #2/#3 → recall the canon and RENDER it in the active language; write only the 20% delta.
2. **Build** the entity (backend repo + API handler + orchestration + frontend wiring) in the project's lang.
3. **On any error/iteration** — `said learn-fix --problem "<what broke>" --learnings "<the invariant>"
   --edits '<changeset>'` so the fix is a recallable task.
4. **After building** — the standard `said init <project-src>` runs harvest-on-init, auto-learning blueprints
   from the repeated structures (create/list/get across the 3 entities → "seen 3x").

## Two projects, two brains

| Project | Language | Brain | Source dir |
|---|---|---|---|
| **Ledgerly** | C# | `hard-eval/beat-them/ultimate/ledgerly/ledgerly.said` | `hard-eval/beat-them/ultimate/ledgerly/src` |
| **Ferro** | Rust | `hard-eval/beat-them/ultimate/ferro/ferro.said` | `hard-eval/beat-them/ultimate/ferro/src` |

Set `SAID_PROJECT=ledgerly` (or `ferro`) for every `.said` call so memories/fixes/blueprints are tagged.

## What we measure afterwards (the full beats-them-all set — doc 30/28)

1. **blueprints auto-created** (fn/shape used >1× → harvested) + **max reuse** (the "seen Nx").
2. **tasks/fixes learned**.
3. **memory records** (claim+evidence).
4. **tokens used** WITH `.said` vs a baseline WITHOUT (re-reading files) → the saving ratio (doc 28).
5. **effort-decay** — tokens to build entity #1 vs #3 (canon should make #3 cheaper).
6. **federation** — does Ferro (Rust) reuse a blueprint/fix that Ledgerly (C#) established? (cross-lang canon).
7. **compaction-survival** — when the session compacts mid-build, the agent re-grounds from `.said` and
   keeps building (does NOT go stupid).
8. **consolidation + abstention** — re-encountering a shape is keep-first (no dupe pile-up); a query with
   no match returns nothing (no confabulation).

`node ultimate/metrics/collect-metrics.js <brain> <name> '<tokensJson>'` produces the per-project report.

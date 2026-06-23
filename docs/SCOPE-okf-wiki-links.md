# Scope — OKF-style wiki links in `.said` (user brings their own LLM)

**Goal.** Let any user who downloads `.said` and drives it via **CLI or MCP from an LLM they
already have** (Claude Desktop/Code, Cursor, any MCP client) build an OKF/Obsidian-style
**linked second brain** — every memory cross-linked by concept — so recall never misses on
vocabulary mismatch ("who do I see about my heart" → the cardiologist note).

**Hard constraint (from the user + the docs).** `.said` itself **never calls an LLM**
(3.3/3.5: "Zero LLM. Pure deterministic math" in the recall path). The intelligence that
*writes good concept links* is the **caller's LLM**, at **write time**, through the CLI/MCP
surface. `.said` provides the deterministic primitives: store links, traverse links, recall.
The graph lives **inside the single portable `.said` file** (proven: edges are `link:` frame
tags, persist across save/reopen, travel when you copy the file). Excludes code memories.

---

## 1. What already ships (built + proven this session)

The primitive is done — opt-in `[[wikilinks]]`:

- **Ingest:** `parse_wikilinks()` extracts `[[concept]]` (incl. `[[c|alias]]`, `[[c#heading]]`)
  from any memory body at every ingest path (`remember_as`, `remember_with_salience`).
  Each becomes a `link:<concept>` frame tag = a build-graph edge, stored **in the `.said`**.
- **Recall:** `ask` Engine D — a query keyword matching a frame's `link:<kw>` tag injects
  that frame as a confident hit. **Additive** (never demotes) → zero regression on
  link-free queries. Reaches a linked note **even when the bridge word is absent from the
  body** (true edge hop, not text match).
- **Portable:** one file carries content + 1-bit fingerprints + audit + the link graph.
  Copy it → the graph comes with it. No external vault, DB, or index.
- **Proven:** symptom→specialist 1/15 → 15/15 @1 via links; full recall suite unchanged
  (needle 30/30, recall@volume 0.90/0.95, conceptual 10/10, etc.).

**Gap to close (this scope):** today the user must hand-type `[[heart]]`. We want their
existing LLM to author those links naturally through CLI/MCP — and to do so *consistently
against the concepts already in the brain* so links converge instead of fragmenting.

---

## 2. The model: `.said` = deterministic store, caller's LLM = curator

```
  user's LLM (Claude/MCP client)                .said (CLI / MCP, no LLM)
  ─────────────────────────────                 ──────────────────────────
  reads the memory text                          stores body + link: edges
  decides concepts: [[heart]],[[cardiology]] ──▶ remember(..., links=[...])
  asks "what concepts already exist?"        ◀── list-concepts  (dedup target)
  links new memory to existing ones          ──▶ link / relate
  recall (deterministic, no LLM)             ◀── ask  → traverses edges
```

`.said` never decides *what* to link — it stores/dedups/traverses what the caller's LLM
proposes. That keeps the trust path deterministic (OKF "AI proposes under review" → here the
review is the user's own LLM session) and means **any** LLM works, now or next year.

---

## 3. Surface to add (CLI + MCP, symmetric)

Four small, deterministic primitives. No new storage format — all reuse `link:` tags.

### 3.1 `add` / `remember` — accept explicit links (don't rely only on inline `[[ ]]`)
- **CLI:** `said add "Dr. Sarah is the cardiologist" --link heart --link cardiology`
- **MCP:** `remember { content, links: ["heart","cardiology"], id? }`
- Inline `[[ ]]` in the body still works; `--link`/`links` is the structured path an LLM
  fills. Both land as `link:<concept>` tags. (Backwards compatible — both optional.)

### 3.2 `list-concepts` — the dedup/convergence surface (the load-bearing one)
- **CLI:** `said list-concepts [--prefix car] [--json]`
- **MCP:** `list_concepts { prefix? } → [{concept, count}]`
- Returns every distinct `link:` concept already in the brain with frequency.
- **Why it's load-bearing:** without it, the LLM invents `[[heart]]` in one session and
  `[[heart-health]]` the next → a fragmented graph that never converges (the classic wiki
  failure). With it, the curator prompt says *"reuse an existing concept if one fits"* and
  the graph **self-converges** — the OKF compile discipline, enforced deterministically.

### 3.3 `link` — add/curate edges on an existing memory
- **CLI:** `said link <doc-id> --add heart --add cardiology --remove cardio`
- **MCP:** `link { doc_id, add?: [...], remove?: [...] }`
- Lets the LLM enrich/fix links after the fact (the "organize overnight" pattern from the
  Karpathy/Obsidian flow, run by the user's scheduler against their LLM — not by `.said`).

### 3.4 `concept` — recall everything linked to a concept (explicit graph query)
- **CLI:** `said concept heart` → all memories carrying `link:heart`
- **MCP:** `concept { name } → [memories]`
- The direct "show me the graph neighborhood" surface OKF/Obsidian users expect; complements
  `ask` (which uses links implicitly).

---

## 4. The curator prompt (ships as a documented recipe, not code)

`.said` doesn't embed this — it's a **prompt the user gives their own LLM** (or an MCP
client system-prompt / a `said skill`). Documented in the walkthrough:

> "When I add a memory: (1) call `list_concepts` to see what already exists; (2) pick 2–5
>  concepts for this memory, REUSING existing ones where they fit, coining new ones only
>  when needed; (3) call `remember` with those links. Prefer broad, searchable concepts
>  (`heart`, `taxes`, `wifi`) over narrow ones."

This is the "never miss a beat" mechanism: the LLM, knowing heart↔cardiology, writes the
bridge link the static encoder can't infer — and `list_concepts` keeps the vocabulary
converged so recall is consistent.

---

## 5. Scope boundaries

- **In:** all natural-language memory pillars (Episodic/Semantic/Procedural/Relational).
- **Out:** **code** memories — symbols/AST already give code its own precise lookup
  (`sym`, trigram); concept-linking prose-style would add noise, not recall.
- **Out:** `.said` auto-extracting concepts itself. Proven this session that deterministic
  in-text extraction can't bridge vocabulary gaps (it can only link words already present),
  and LLM extraction belongs to the caller, not the hot path. So no embedded extractor.
- **Out:** typed/predicate edges and multi-hop graph walks — that's the larger `build-graph`
  roadmap (3.9). This scope is single-hop concept edges, which is what closes the recall gap.

---

## 6. Why this is the right shape

- **Honors the constraint:** zero LLM in `.said`; the user's LLM (which they definitionally
  have, since they drive `.said` from it) does the curation via CLI/MCP.
- **Portable + deterministic:** edges are `link:` tags inside the one file; recall traverses
  them with no model. Copy the file = copy the linked brain.
- **Self-converging:** `list_concepts` is the small piece that turns "LLM writes random
  links" into "LLM writes consistent links" — the difference between a brain that gets
  sharper and one that fragments.
- **Model-agnostic forever:** any LLM that can call CLI/MCP can curate; `.said` is the
  durable substrate.

---

## 7. Build order (smallest shippable first)

1. `list_concepts` (CLI + MCP) — read-only, enables the curator loop immediately even with
   today's inline `[[ ]]`.
2. `add --link` / `remember links:[]` — structured link authoring.
3. `concept <name>` (CLI + MCP) — explicit neighborhood query.
4. `link <id> --add/--remove` — post-hoc curation.
5. Walkthrough recipe + a `said` skill packaging the curator prompt.

Each step is deterministic, additive, in-file, and independently testable against the
recall suite. No documented gate is touched.

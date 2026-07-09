# Factory-First Architecture — SDK Factory as the foundation, Orchestration on top

**Status:** Proposed (design review)
**Date:** 2026-06-24
**Supersedes the relationship in:** [FACTORY-SDK-PIPELINE.md](FACTORY-SDK-PIPELINE.md) (which describes
the factory generating an SDK for the factory's *own* API). This doc generalizes that: the factory
generates an SDK for **any code** — your REST API/OpenAPI spec OR your plain source — and the
Orchestration UI then plugs in **on top of** that generated SDK.

---

## 1. The vision (user's words)

> "Run sdk-factory for my code, then plug the orchestration UI in on top of it."

The relationship is **layered, not sibling**. The SDK Factory is the **foundation**: point it at your
system, it emits a typed SDK. The Orchestration UI is a **consumer**: it imports that SDK, and each
operation the SDK exposes becomes a node/action in the flow builder — so you build, govern, and enforce
flows over **your own system**.

```
┌─ 3. Orchestration UI (plugs in ON TOP) ─────────────────────────────┐
│   imports the generated SDK → each operation becomes a flow node     │
│   build flows · govern (control plane) · enforce (per-tenant)        │
└───────────────────────────────┬─────────────────────────────────────┘
                                │ consumes the generated SDK
┌─ 2. SDK Factory ENGINE (EXISTS today) ──────────────────────────────┐
│   operation catalog / OpenAPI spec → openapi-generator → typed SDK   │
│   (SdkGenerator.cs, self-hosted engine, 80+ targets, branded zip)    │
└───────────────────────────────┬─────────────────────────────────────┘
                                │ needs a spec/catalog — TWO ways in:
        ┌───────────────────────┴───────────────────────────┐
   A. HTTP API path                          B. Plain-code path
   your REST API / OpenAPI spec              your source (functions/classes/procs)
   → feed spec to the engine                 → .said code-graph scans it → operation
   (trivial: swap the spec source)             catalog → signatures → spec
                                              (the new, novel stage-1 work)
```

This is the world-class shape — generate the client first, then build experiences on your own SDK
(how Stripe/Twilio ship an SDK and build their dashboards on it).

---

## 2. What already exists (the assets that de-risk this)

| Asset | What it gives us | Where |
|---|---|---|
| **`sdk-factory` SKILL** | the FOUNDATION, already specified: scan a system (OpenAPI / endpoints / MCP tools) → operation inventory → typed SDK + radical-clarity docs + architecture record + agent-native OKF wiki. A *method to run*, not code to write. | `.claude/skills/sdk-factory/` |
| **SDK Factory engine** | runtime endpoint: spec → branded SDK for 80+ targets via self-hosted openapi-generator | [SdkGenerator.cs](../apps/OrchestrationFactory/OrchestrationFactory.API/SdkGenerator.cs) |
| **.said code-graph** | every function/method/proc → a symbol node; every call → a traversable edge; 7 langs + SQL; deterministic, no LLM; in the portable `.said` file | `crates/sca-core/src/code_search.rs`, commit `9067d08` |
| **Control-plane spike** | validation rules + draft/publish/rollback + per-tenant enforcer + UI (built, verified) → becomes stage 3's governance | [CONTROL-PLANE-DESIGN.md](CONTROL-PLANE-DESIGN.md) §10b |

**Key correction:** the foundation (stage 1 + 2 for Path A) is **already built as the `sdk-factory`
skill** — "run sdk-factory for my code" = *execute that skill against your system*, producing the SDK
+ the operation inventory. Do NOT hand-build a parallel Path A; **run the skill.** The skill's Step 1
(operation inventory) IS the OperationCatalog contract (§4); its Step 3 generates the SDK. For Path B
(plain code, no HTTP), the skill scans "the real endpoints… else the real surface" — the `.said`
code-graph feeds that scan where there's no spec/endpoints.

**So the remaining new work is narrowed to:**
- **Run the `sdk-factory` skill** on the target system (foundation — Path A, and Path B via code-graph-assisted scan).
- **Stage 3 only** — make the Orchestration UI *consume* the skill's output (operation inventory → flow-node palette; nodes call the generated SDK). This is the genuinely new integration.

---

## 3. The gap, honestly stated

**Today** the factory renders the spec from its OWN in-process API. Your vision needs the spec to come
from **your** system. So stage 1 ("code → spec/catalog") is the real new work, and you chose **both**
input paths:

### Path A — HTTP API / OpenAPI spec (easy)
Your system is/has a REST API, or you can export an OpenAPI/Swagger file. Then it's plumbing: feed
*your* spec to the existing engine instead of the factory's own. `OpenApiSpecRenderer` already proves
the spec source is swappable — add a second source that takes a provided spec (upload or URL).
**Effort: small.** Risk: low (the engine already eats OpenAPI).

### Path B — plain code, no HTTP API (the novel part)
Your code is libraries/functions/classes/procs with no web API. To generate an SDK we must turn source
into an **operation catalog**, then into a spec. The `.said` code-graph already does the hard 80%:

- ✅ **Operation discovery** — every function/method/proc is a symbol node (7 langs + SQL), found
  deterministically via tree-sitter. This is the catalog of "things your code can do."
- ✅ **Relationships** — call/EXEC edges (`code_calls`/`code_callers`) — useful later for ordering
  nodes and suggesting flows.
- ✅ **Bodies** — each `CodeChunk` carries the full function `content`.
- ❌ **Signatures** — `CodeChunk` does NOT pre-extract typed params + return. But the body is present,
  so the **declaration line is parseable**. **This is the one genuinely new piece of stage-1 work:**
  a `symbol → operation signature` extractor (params, types, return) per language.

**Effort: moderate.** Risk: medium, concentrated entirely in signature extraction across languages
(types in dynamic languages like Python/JS may be partial → emit `any`/`object`, refine later).

---

## 4. The unifying contract — one Operation Catalog, two producers

Both paths converge on a single intermediate shape so the engine and the orchestration layer don't
care where it came from:

```jsonc
// OperationCatalog — the neutral contract between stage 1 and stages 2/3
{
  "source": { "kind": "openapi" | "code", "name": "acme-billing", "lang": "python" },
  "operations": [
    {
      "id": "charge_customer",            // symbol name or operationId
      "summary": "Charge a customer",     // from docstring / OpenAPI summary (best-effort)
      "params": [                          // Path B: parsed from the declaration; Path A: from the spec
        { "name": "customerId", "type": "string", "required": true },
        { "name": "amount",     "type": "number", "required": true }
      ],
      "returns": { "type": "object" },
      "calls": ["validate_amount", "process_payment"]  // Path B: from the code-graph edges
    }
  ]
}
```

- **Path A** fills this from the OpenAPI spec (already structured).
- **Path B** fills this from the code-graph (operations + calls exist; params/returns from the new
  signature extractor).
- **Stage 2** renders the catalog → OpenAPI → existing engine → SDK.
- **Stage 3** (orchestration) loads the catalog → each operation becomes a flow node in the palette.

This contract is the keystone: it lets Path A and Path B be built independently and lets the
orchestration layer be agnostic to which one produced the SDK.

---

## 5. Stage 3 — how Orchestration plugs in on top

The user chose: **"Orchestration uses the generated SDK."** Concretely:

1. The factory emits the SDK **and** the Operation Catalog (§4) for the scanned system.
2. The Orchestration UI loads the catalog → the **node palette is populated with YOUR system's
   operations** (each operation = an action node, like today's `said_remember` etc. but for your code).
3. A flow node, when run, **calls the generated SDK** for that operation (Path A: HTTP call; Path B: a
   thin host that invokes the generated client/binding).
4. The control-plane spike already built (validation, publish/rollback, per-tenant enforce) applies
   unchanged — now governing **your** operations.

So the orchestration layer becomes a **visual driver over the generated SDK**, with governance on top.

---

## 6. Build order (each independently demoable)

1. **Define the OperationCatalog contract** (§4) — types in Contracts; the keystone, do first.
2. **Path A producer** — accept a provided OpenAPI spec (upload/URL) → catalog → existing engine emits
   the SDK. Proves "run factory for my HTTP API" end-to-end with the smallest change.
3. **Path B producer (stage 1)** — `.said code-graph → OperationCatalog`, including the new
   **signature extractor** (start with one language, e.g. Python or C#, expand). Proves "run factory
   for my plain code."
4. **Catalog → OpenAPI renderer** — feed both producers' catalogs into the engine uniformly.
5. **Stage 3: orchestration consumes the catalog** — palette auto-populated from YOUR operations; a
   node runs the generated SDK; governance (the spike) applies.

Checkpoint after step 2 (Path A end-to-end) before investing in the harder Path B.

---

## 7. Honest risks & open questions

1. **Signature extraction across languages (Path B)** — the concentrated risk. Mitigation: per-language
   extractors, partial types allowed (`any`/`object`), start with the typed languages (C#/Java/Go/Rust/
   TS) where signatures are cleanest; dynamic langs (Python/JS) are best-effort.
2. **"A node runs the SDK" for Path B** — calling a generated *library* SDK (not HTTP) from the
   orchestration runtime needs a host/loader per target language. Path A (HTTP) has no such issue. May
   scope Path B's *execution* to a later phase (catalog+SDK generation first, live execution after).
3. **Does the factory scan in-process or via the `.said` CLI?** The code-graph lives in the Rust core /
   `.said` file. The factory (C#) would shell to the `said` CLI (`said calls`/the catalog export) or
   read the `.said`. Defer the exact seam to step 3.
4. **Where does "my code" come from** — a path the factory scans, an uploaded repo, a git URL? Start
   with a local path (the CLI already operates on local repos).

---

## 8. Relationship to the control-plane spike

The control-plane spike (validation/lifecycle/enforcer/UI) is **complete and still applies** — it
becomes stage 3's governance layer over the generated operations. **It is currently uncommitted on
`master`** (the user chose to pivot before committing). Recommend committing it before/early in this
effort so it isn't lost.

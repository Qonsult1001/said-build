# 41 — UX & recall-quality plan (client feedback → fixes)

Plan responding to client feedback on "rough for users." **Hard constraint: no change here may
regress documented behavior.** Every item is read-only / additive / doc-only — the recall **ranking
path is not touched**. This is deliberate, and grounded in the project's own retrieval contract (below).

## The retrieval contract this plan is built on (why "0.90 ties" is a non-issue)

The single most important thing to hold: **`.said` does not chase recall@1.** Per
[31-memory-evidence-standard.md](31-memory-evidence-standard.md) §"world-class memory does NOT solve
recall@1; it sidesteps it" — Claude Code and Kimi use **manifest + LLM-select**, not "the one right
vector first." The contract is: **`ask` surfaces the top-K, the answering LLM reads them and picks.**
The engine's job is *reachability into the top-K*, not perfect rank-1 ordering. (See also
[okf-is-reachability-not-precision] in [11-known-limitations.md](11-known-limitations.md) and the tool
description: *"the right memory is essentially always in that set — the brain surfaces the top-K, you
pick."*)

So a client observation like *"8 results tie at 0.90 on a vague ask"* is **measuring the wrong axis**.
On that same probe the target memory **was in the returned set** — the LLM reads it and answers. Ties in
the flat reachability band are expected and harmless *because the LLM decides*, not the score. We do
**not** add a reranker or a tie-break to the default path: that would be solving a problem the
architecture already sidesteps, and it carries the single-hop-twin regression risk the docs warn
against. **Retrieval is not the bottleneck; surfacing the vocabulary to the user is.**

## Tracker reconciliation — what's already SHIPPED (v0.11.4) vs open

The client tracker predates v0.11.4. Corrected status:

| Client issue | Real status | Where |
|---|---|---|
| Dream invisible to users | ✅ **Shipped** — plain-English "Learning:" line | v0.11.4, FIXES-LOG #16 |
| Status shows code-tier next steps | ✅ **Shipped** — `ask`/`remember`/`get` only on brain | v0.11.4, FIXES-LOG #16 |
| Tags write-only (`quarter:Q2` invisible) | ✅ **Shipped** — `list_tags` surfaces the tag vocabulary; `ask --tag`/`tags:[…]` filters by facet | v0.11.4, FIXES-LOG #16 |
| Score ties at 0.90 on vague asks | ✅ **Fixed WHEN SCOPED — intra-facet ties remain by design.** The tag filter cuts cross-facet bleed; within one facet a vague one-shot still ties at 0.90 (top-K + LLM-decides — the target is in the set). **v0.11.8:** the server now shows tags per result and, on a tie, appends a scoping hint telling the agent HOW to narrow (see SHIPPED section below) — the workflow is now product behavior, not just a doc. ⚠️ Mark it "fixed with tag scoping", NOT "fixed for unscoped vague queries". | 31, 11, this doc |
| `"102 of 36 memories indexed"` | ✅ **Shipped** — honest index count (tombstoned entries explained) | v0.11.4 |
| Filler-memory noise (the tester's brain) | ✅ **Cleaned** for this brain (66 fillers tombstoned) — see "E" for the general story | this session |
| Agent must use brain autonomously | 🟡 Documented caveat (constitution nudges; obedience = host agent) | 11-known-limitations 14.1 |
| MCP free = no bulk ingest | 🟡 By design (bulk ingest is code-tier); memory brains fill one memory at a time | 40-build-tier-capability-matrix |
| No slash commands | 🟡 By positioning (agent is the UI; WASM admin is the human UI) | 11-known-limitations 14.1 |

**Action for the client:** the top three "🔴 still rough" items are **already fixed in v0.11.4** — the
tester was on an earlier binary (the running MCP didn't advertise the `tags` param). **Reinstall v0.11.4
and reload agents** to get them. Nothing further to build for those.

### The `said-watch` recall matrix (verified on the live brain, v0.11.4)

Concrete proof of "fixed when scoped; intra-facet ties are the design" — the exact probes a client runs:

| Probe | Filter | q2-said-watch | Top score |
|---|---|---|---|
| "offline integration that watches files for changes" | none | ❌ not in top 8 | 0.90 tie (cross-facet bleed) |
| same | `tags:["quarter:Q2"]` | ✅ in the 6-item Q2 set | 0.90 tie *within* the facet |
| "filesystem watcher watches files for changes" | `tags:["quarter:Q2"]` | ✅ **#1** | **1.27 [semantic]** |
| "which Q2 integration is the filesystem watcher" | `tags:["quarter:Q2"]` | ❌ not in top 5 | 0.90 tie (top-5 are other Q2 items) |

Read this as: **the tag filter breaks cross-quarter bleed; within a facet a vague one-shot still ties**,
and the answer is workflow — `list_tags` → scope with `quarter:Q2` → `ask` with a distinguishing keyword
(or let the LLM pick from the small scoped set). With scope **and** a keyword, the target ranks #1 at
1.27. This is the top-K + LLM-decides contract, not a bug — do not "fix" the unscoped tie by touching the
ranking path.

## Genuinely open work (all read-only / additive — zero ranking change)

### A. `list_concepts` shows `integrations (15)`, not the quarters a user expects

**Observation:** a user browsing "what's organized here" runs `list_concepts` and sees `integrations
(15)` but not `quarter:Q2` — because concepts (`[[wikilinks]]`) and tags (`namespace:value`) are two
different vocabularies, and `list_concepts` only walks the wikilink graph (by design). The user doesn't
hold that distinction.

**Fix (read-only, additive):** teach the *surfacing*, not the engine. Option 1 (preferred): have
`list_concepts` print a one-line footer pointing to `list_tags` when tags exist —
*"(you also have N tags — run `list_tags` to browse them)"*. Option 2: a combined `overview`-style
"vocabulary" view that shows both lists side by side. **No recall change** — both are new read output.
Risk: **none** (adds a line; changes no existing value). Verify: `list_concepts` output unchanged except
the appended pointer; `list_tags` unchanged.

### B. Make the concepts-vs-tags distinction obvious in the product, not just the docs

**Observation:** the client kept expecting tags in `list_concepts` — the mental model isn't landing.

**Fix (doc + copy only):** the tool descriptions already say "distinct from list_concepts" — reinforce
it once in the brain how-to ([how-to-organize-and-find-memories-with-an-agent.md] already has the
concepts-vs-tags callout — confirmed present). Add the same one-liner to the `list_concepts` tool
description ("for the `tags` you attach, use `list_tags`"). Risk: **none** (description text).

### C. Filler / test-memory noise at scale (the general product issue)

**Observation:** the tester's brain had 66 `fill:*` memories (>50% of the brain) that muddied ranking
tests. We cleaned this brain, but a real user could accumulate low-value memories the same way.

**Fix (guidance first, not code):** document the hygiene loop in the brain how-tos — an agent should
tag throwaway/test memories (`status:test`, `ttl:*`) and the user can `delete --tag` them in one shot
(already works). Optionally, later: a `said clean --tag <t>` convenience alias over the existing
tag-filter delete. Risk: **none** (docs); the optional alias is additive over a shipped path. Do **not**
auto-delete anything — deletion stays user-driven (per the file-safety rule in
[40-build-tier-capability-matrix.md](40-build-tier-capability-matrix.md)).

## SHIPPED (v0.11.8) — the scope-with-tags workflow surfaced as PRODUCT behavior

The one missing piece was *when* to scope. The product already supported tag-scoped recall (`ask` has a
`tags` filter, `list_tags` exists), and the docs taught the pattern — but the agent had to *guess* when
recall was ambiguous. v0.11.8 makes the server say so at tie time. **All additive; zero ranking change**
(purely enriched output + guidance — verified a single clear winner is unchanged and shows no footer).

1. **Tags on every `ask` result.** Each hit now prints its user-facing facets
   (`tags: quarter:Q2, project:said`), with the internal ones (`link:`/`pillar:`/`salience:`/`blake3:`/
   `user_id:`/`session:`) filtered out. When several notes tie on "integrations", the agent can *see*
   that one is `quarter:Q2` and another `quarter:Q3` — no second call needed.
2. **Tie-detection footer.** When ≥3 results cluster within 0.05 confidence, `ask` appends the tags that
   **distinguish** the tied set (those not shared by all of them), with counts, e.g.
   *"5 close matches (within 0.05). To narrow, ask again scoped to a tag: • quarter:Q2 (1 of these) …
   Don't guess which memory the user meant — scope, then re-ask."* If the tied set shares all tags (no
   facet can disambiguate), it says so honestly instead. Data already in hand (scores + `get_meta` tags)
   — no extra query.
3. **Recall-side steering in the brain constitution.** A block instructing the agent: on close matches,
   pick a facet and re-ask `ask tags:["quarter:Q2"]` (or ask the user which they mean) — never guess.
   Agents read the connect instructions every session, closing the loop docs alone can't guarantee.

**Why it matters at ~30+ memories:** below ~10 memories semantic `ask` returns one clear winner; past
~30, many notes share vocabulary ("launch", "integrations", "roadmap") so the brain correctly returns a
top handful and the agent must disambiguate. Tag scoping shrinks that pool *before* meaning-matching —
the intended tie-breaker when topics overlap. This is the v0.11.4 contract ("scope with tags"), now
surfaced by the server at the moment it applies rather than left for the agent to recall from a doc.

**Still not a ranking change.** The footer is guidance; the 0.90 ties themselves are untouched (top-K +
LLM-decides, by design). A single clear winner sees only its tag line added, no footer.

## The rule this plan commits to

**No item may change the recall ranking path or any documented behavior.** Everything above is (a) new
read-only output, (b) copy/description wording, or (c) documentation. The "0.90 ties" are left alone on
purpose — they are the top-K-plus-LLM-decides contract working as designed, not a bug. Any future
ranking experiment (e.g. a reranker) is out of scope for this plan and, if ever attempted, ships behind
an opt-in flag defaulting OFF, gated on the recall-at-volume canary (≥0.95) and the twin-regression
test — never as a silent default.

## See also

- [31-memory-evidence-standard.md](31-memory-evidence-standard.md) — manifest + LLM-select; why recall@1 is the wrong target.
- [40-build-tier-capability-matrix.md](40-build-tier-capability-matrix.md) — tier surface, tag filter, file-safety, correct-by-design behaviors.
- [11-known-limitations.md](11-known-limitations.md) — 14.1 agent-is-the-UI; okf reachability-vs-precision.
- [FIXES-LOG.md](FIXES-LOG.md) #16 — the v0.11.4 UX fixes (status honesty, list_tags, tag filter, index count).

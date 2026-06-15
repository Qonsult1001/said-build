# Fix-Replay by Fingerprint — Design (read-only v1)

> **STATUS: v1 SHIPPED in said 0.9.0.** Two CLI verbs: `said record-fix`
> (write a green-build case) and `said suggest-fix` (read-only lookup). Proven
> end-to-end: a paraphrased ticket matched a recorded fix at 0.85 similarity and
> returned its change-set; a dissimilar ticket returned `match:null`. Uses the
> 3-engine `ask` fusion (sym+grep+SCA) for matching — pure SCA `query` missed
> the case. Cases are plain `fixcase`-tagged frames (reuses all existing
> machinery). The caller still owns the build+test gate and decides whether to
> replay. See "Implemented surface" at the bottom.

**Goal:** before any LLM call, let `.said` answer "have I landed a fix shaped like
this ticket before?" and, if so, return the *change-set* that built+passed last
time — case-based reasoning in the SCA fingerprint substrate, no inference.

**v1 scope (this iteration):** READ-ONLY suggestion. `.said` SUGGESTS a past fix
with a confidence score; the cycle/human decides. No auto-apply. Prove the
matching is trustworthy before anything acts on it.

**Non-negotiable guardrail:** a fix becomes a replayable "known-good case" ONLY
when it passed a real **build + test gate**. Model confidence / "looked right"
never qualifies. The gate is the sole ground truth — otherwise the loop
amplifies its own mistakes.

---

## What `.said` already has (the substrate — no new ML needed)

- **1-bit SCA fingerprints** per frame (23 bytes/doc) + `semantic_delta_bytes`
  (Hamming distance over fingerprints) → microsecond shape comparison.
- **Tombstone/lineage history** — every edit's before/after is preserved.
- **Salience scoring** — per-frame weight that recall already uses.
- **`said edit` change-sets** — structured, replayable edit ops (the action).

## Data model — a "fix case" frame

When a change-set builds+passes, record ONE new frame (pillar: a new
`FixCase`-style tag) capturing:

```json
{
  "ticket_shape": "<the ticket text/summary that was fingerprinted>",
  "edits": [ { "file","mode","symbol|anchor","content" }, ... ],   // the said edit change-set
  "outcome": "built+passed",          // ONLY value that gets recorded
  "pr": "#102",
  "stamp": "<ISO from caller — .said has no clock>"
}
```

The frame's fingerprint = fingerprint of `ticket_shape`. That's the index key.

## The lookup — `said suggest-fix`

```
said suggest-fix --path brain.said --ticket "<ticket text>" --json
→ { "ok": true, "match": {
      "similarity": 0.91,            // 1 - normalized Hamming over fingerprints
      "pr": "#102",
      "edits": [ ...change-set... ],
      "note": "isomorphic to a fix that built+passed; review before replay"
    } }
  or { "ok": true, "match": null }   // nothing close enough
```

- Fingerprint the incoming `--ticket`, Hamming-compare to all `FixCase` frames.
- Return the best match **only if** similarity ≥ a threshold (start strict, e.g.
  0.85; tune from real outcomes). Below threshold → `match: null` (fall through
  to the LLM). Never guess.

## How it ties into the Advisory cycle

```
ticket
  → said suggest-fix          # NEW: known-shape? (read-only)
      ├─ match (≥threshold) → show the change-set to the operator;
      │                        if approved, apply via `said edit` (NO LLM call)
      └─ no match            → existing path: Groq plans → said edit → ...
  → dotnet build + test gate  # unchanged — the ground truth
  → on green: record a FixCase frame (said add with the change-set)   # NEW: learn
```

The cycle still owns the gate and the recording. `.said` owns: fingerprint the
ticket, find the nearest known-good case, return it. That split keeps `.said`
language-agnostic and inference-free.

## Honest boundaries (so we build the real thing)

- **Not latent-space reasoning.** `.said` doesn't think; it matches shapes and
  replays stored actions. Market it as "a memory that learns which actions
  worked," not "an LLM in latent space."
- **The LLM is still the reasoner** for novel tickets. Replay only short-circuits
  *recurring* shapes. 30% hit-rate would already be a big, cheap win.
- **Trust = the gate.** Recording a case on anything but green build+test makes
  replay confidently wrong. Keep the gate as the only success signal.

## Later (NOT v1)

- Outcome-weighted recall: boost/decay frame salience by whether the recall led
  to a green build (makes `ask` self-tune).
- Self-repairing recall blind spots: when an edit succeeds against a line the
  brain couldn't surface (e.g. C# top-level statements in Program.cs), write that
  line back as a learned anchor frame so next `ask` finds it.
- `said anchors <file>`: list candidate insertion points from ANY file (not just
  named symbols) — closes the top-level-statements recall gap structurally.

---

## Implemented surface (v1, said 0.9.0)

**Record a green-build case** (caller runs this ONLY after the build+test gate passes):
```
said record-fix --path brain.said \
  --ticket "<ticket text — this is what gets fingerprinted>" \
  (--edits '<json>' | --edits-file <f>) [--label <pr#>] --json
→ { "ok": true, "recorded": "fixcase::<hash>", "label": "<pr#>" }
```
Stores a `fixcase`-tagged frame: body = `ticket` + separator + change-set JSON.
The ticket drives the fingerprint. A leading UTF-8 BOM in the edits file is
stripped (any platform). Invalid JSON is rejected — no garbage cases.

**Suggest a known-good fix** (read-only; no LLM call):
```
said suggest-fix --path brain.said --ticket "<ticket text>" [--min-similarity 0.85] --json
→ { "ok": true, "match": { "similarity", "doc_id", "label",
                           "matched_ticket", "edits": [...], "note" } }
  or { "ok": true, "match": null }   // below threshold → fall through to the LLM
```
Matching uses the 3-engine `ask` fusion (sym + grep + SCA), filtered to
`fixcase` frames. Returns the best match only at/above `--min-similarity`
(default 0.85). Tune the threshold from real hit/miss outcomes.

### Cycle integration (Advisory)
```
ticket → said suggest-fix
          ├─ match  → show change-set to operator; if approved, apply via `said edit` (NO LLM)
          └─ null   → existing path: LLM plans → said edit → ...
       → dotnet build + test gate          (unchanged — the ground truth)
       → on GREEN only: said record-fix     (learn the case for next time)
```
The gate stays the sole success signal. `.said` matches shapes and replays
stored actions; it does not reason.

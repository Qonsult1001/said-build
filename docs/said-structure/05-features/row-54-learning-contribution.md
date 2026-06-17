# Learning contribution — crowd-built `code.said` for the Hub

Status: **partial** — opt-in client sink shipped; lake backend + import tool are next.
Last updated: 2026-06-17

## Why this exists

The moat is proven: a verified coding learning lifts a weak model (RED→GREEN). The
next step is **volume** — pool verified learnings from many users into a shared,
downloadable brain (`code.said`, `python.said`, `csharp.said`) that any model can use
as a plugin. This is the **contribution side** of the Hub ([row-52-hub.md](row-52-hub.md)):
row-52 distributes a curated brain; this doc is how the brain gets *built* from many
contributors instead of a single author ingesting a corpus.

```
each user's .said  ──(opt-in, scrubbed)──►  learning lake (blake3 objects)
   verified fix                                     │  thousands accumulate
   (Procedural pillar)                              ▼  [manual import, when ready]
                                          import-all → code.said → sign → Hub publish
```

## Hard rule: OPT-IN, default OFF

Contribution is **off by default** and **never silent**. It honors Rule 1
([13-integrations.md](../13-integrations.md)): offline by default, no network/collection
unless the operator explicitly opts in.

- Gate: `said_orchestration::sink::contribution_sink()` returns `None` unless
  `SAID_CONTRIBUTE` is set (non-empty). No opt-in → nothing is collected, nothing
  leaves the machine.
- It lives in `said-orchestration` (the separate, online process), **not** `sca-core`
  (Rule 2). `learn_coding_fix` stays pure-local — recall/learning work identically with
  contribution off.
- **Scrubbing is best-effort, NOT a guarantee.** Operators enable contribution only on
  code they are willing to share. The opt-in is the real safety boundary; the scrubber
  is defense-in-depth, not a license to upload private code.

## What gets shared (not raw bytes — the LEARNING)

Recall transfers *understanding*: injection passes the learning note + the change-set
as a reference the model **adapts**, never replays verbatim (see
[15-orchestration.md](../15-orchestration.md)). So the contribution is the same thing
the system actually uses:

`{ id (blake3), lang, problem, note (FILES/STEPS/ERRORS/LEARNINGS), edits_json, action }`

scrubbed by `sink::scrub_learning`:

- **Secrets/tokens** (`sk-`, `ghp_`, `AKIA`, `Bearer`, `eyJ…`, long `key=`/`token=`
  values, high-entropy blobs) → `<redacted>`
- **Emails** → `<email>`
- **Absolute / home / UNC paths** → `<path>/<basename>` (keeps the filename's code
  meaning, drops the private directory chain)
- The code logic / the learning itself is preserved.

`lang` is inferred from the change-set's file extensions (`.rs`→rust, `.py`→python,
`.cs`→csharp, …) so the import can route into the right per-language brain.

## Where it lands — testing vs production (IMPORTANT)

There is exactly **one** user-facing control: the on/off opt-in (`SAID_CONTRIBUTE`).
**Which** lake the data goes to is a **build-time decision in the compiled binary**,
not a user choice. `sink::contribution_sink()` is the single swap point.

- **Testing / today (LOCAL FOLDER):** `LocalLakeSink` writes one blake3-named JSON per
  learning to `SAID_LAKE_DIR` (default `./said-lake/`), append-only. This lets you
  inspect exactly what would be contributed, on disk, before any network exists. Enable
  with `SAID_CONTRIBUTE=1 SAID_LAKE_DIR=/path/to/folder`.

- **Production deployment (MUST CHANGE):** the local folder is **only** for testing.
  For production the swap point in `contribution_sink()` must be replaced with the
  **hardcoded global shared-lake sink** — an S3/R2 bucket or collector endpoint whose
  destination + credentials are **baked into the compiled binary**. Users do not see
  or configure the destination; the only exposed flag remains on/off (default OFF,
  enabled by a visible opt-in: env var / config flag / first-run notice). A real cloud
  backend is a drop-in `impl LearningSink`; only the one line in `contribution_sink()`
  changes.

This separation is deliberate: the local folder proves the plumbing + scrubbing with
zero network and zero risk; production points the same proven path at the global lake.

## Does blake3 add value here?

Yes — but for a specific job, and it is **not** the dedup engine. Be precise:

**What blake3 IS doing (keep it):**
- **Content-addressed key** — each learning is stored as `<blake3>.json`, so a
  byte-identical re-push (same machine re-learns the same scrubbed fix) collapses to
  the same object. Free **exact-duplicate** dedup + idempotent uploads.
- **Integrity** — the id verifies the object's bytes on read/transfer.
- **Protocol consistency** — the rest of `.said` (frames, ingest, checksums) is blake3.

**What blake3 is NOT doing (don't mistake it for this):**
- **Semantic dedup.** Two users who learn "the same LRU fix" write different words →
  different blake3 → two separate objects. blake3 only catches *byte-identical* dupes.
  Collapsing *near-duplicate* learnings from many users happens at **import time** via
  the intent/semantic fingerprint (`best_coding_fixes`), not blake3.

So blake3 = the lake's address/integrity layer (correct, cheap, consistent); the
**fingerprint** = the merge/dedup layer at import. They are complementary, not the
same job.

## Import → `code.said` (next)

A `said hub import-lake <dir>` job (not yet built) folds the lake into a published
brain:

1. Read all lake objects; group by `lang`.
2. **Dedup by the intent/semantic fingerprint** (not blake3 — near-duplicate learnings
   from different users collapse to the strongest one). The scorer already does this
   (`best_coding_fixes`).
3. `learn_coding_fix` each survivor into a fresh `code.said` — **incremental indexing**
   (just shipped) makes this O(N), not O(N²), so thousands import quickly.
4. Mark `BrainMode::Locked`, ed25519-sign, publish via row-52.

The result is a free, downloadable brain that makes a weak model (e.g. gpt-oss-20b)
punch above its weight — the moat, pooled.

## How to test

```bash
# Default: contribution OFF — nothing written.
cargo test -p said-orchestration            # sink scrub/lake unit tests
#   contribution_sink() == None unless SAID_CONTRIBUTE set
#   scrub_learning strips a planted sk-… secret, /home/… path, and email,
#     but keeps the learning text and basename; blake3 id; lang routing.

# Opt-in run (operator consents): green gate -> a blake3 object appears in the lake.
SAID_CONTRIBUTE=1 SAID_LAKE_DIR=/tmp/lake  said-orchestrate --brain … --task …
ls /tmp/lake    # one <blake3>.json per verified learning
```

## Known limitations / open questions

- **Scrubber coverage** is heuristic — it cannot detect proprietary algorithms or
  business logic embedded in a learning. Opt-in is the boundary; document this clearly
  to operators.
- **Lake backend** undecided (S3/R2 vs collector). Trait seam is in place.
- **Import/dedup tool** not built (step above). Dedup policy (how aggressively to
  collapse near-duplicates, quality threshold for inclusion) is a product decision.
- **Provenance / abuse**: a poisoned learning could degrade `code.said`. Import should
  gate on the verification marker + a quality pass; signing (row-52) covers
  distribution trust, not contribution trust.

## References

- [row-52-hub.md](row-52-hub.md) — distribution layer (registry, sign, Locked, deltas)
- [15-orchestration.md](../15-orchestration.md) — the learn step + why we share the
  learning, not raw bytes
- [13-integrations.md](../13-integrations.md) — Rule 1 (offline default) / Rule 2
  (network out of core), which this opt-in design satisfies
- `crates/said-orchestration/src/sink.rs` — the implementation

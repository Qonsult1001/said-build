# Row 33 — Salience scorer v0

**Status:** ✅ shipped 2026-04-21 (Decision 4 v0 — heuristic, ML v1 deferred honestly)

## What it does

Scores every call to `remember_with_salience` on a 0..=100 scale using 8 deterministic signals. Band (Low / Medium / High) drives downstream tag writing + dream-cycle triggering.

## Where it lives

- [`crates/sca-core/src/salience.rs`](../../../crates/sca-core/src/salience.rs) — `score_turn`, `Salience`, `SalienceBand`, `SalienceAccumulator`
- [`SaidFile::remember_with_salience`](../../../crates/sca-core/src/said_file.rs) — the writer path that invokes it
- MCP `salience` tool — exposes standalone scoring

## Inputs

- `text: &str` — any length
- `pillar: Pillar` — affects the pillar bias signal

## 8 signals (each bounded)

1. **Explicit markers** — `/remember`, `important:`, `always`, `never`, `must`, `critical:`, `remember that`, `don't forget`, `key point` → +35 per hit, cap 45
2. **Correction markers** — `actually,`, `no, wrong`, `i meant`, `not that`, `correction:`, `to be clear`, `let me correct` → +12 per hit, cap 20, adds `reconsolidation` tag
3. **Decision markers** — `we decided`, `let's go with`, `chose`, `going with`, `final answer`, `approved:` → +12 per hit, cap 20, adds `decision` tag
4. **Assertion markers** — patterns like ` is `, ` are `, ` = `, `:` (needs ≥4 words) → +8-15
5. **Length band** — 0-3 words=0, 4-5=5, 6-40=10, 41-120=7, 121+=5
6. **Pillar bias** — Semantic=+8, Procedural=+10, External=+5, Code=+5
7. **Chit-chat penalty** — matches `lol`/`ok`/`thanks`/`cool` etc. for short utterances → -15
8. **Question penalty** — ending in `?` → -5

Total bounded at 0..=100, mapped to band:
- 0-29 Low
- 30-59 Medium
- 60-100 High

## Outputs

```rust
pub struct Salience {
    pub score: u32,
    pub band: SalienceBand,
    pub tags: Vec<String>,      // ["salience:<band>", optionally "reconsolidation",
                                 //  "decision", "explicit"]
}
```

Tags flow through into `FrameMeta.tags` via `remember_with_salience`.

## Session accumulator

```rust
let mut acc = SalienceAccumulator::new();  // threshold 150 (from Generative Agents)
if acc.add(scored.score) {
    // crossed threshold, auto-trigger a dream cycle
    brain.dream(50);
}
```

Threshold 150 hits roughly every 10-15 high-signal turns.

## How to test

Unit tests in the module. Fixture examples:

```rust
// Explicit marker
assert!(score_turn("Important: the password is abc123", Pillar::Semantic).score >= 60);

// Chit-chat
assert!(score_turn("ok thanks", Pillar::Episodic).band == SalienceBand::Low);

// Correction
let s = score_turn("actually, the password is xyz789", Pillar::Semantic);
assert!(s.tags.iter().any(|t| t == "reconsolidation"));
```

## How to extend

To upgrade to a trained v1:
1. Keep the `score_turn` signature (`&str, Pillar) -> Salience`)
2. Replace the internals with Model2Vec + linear head inference
3. Ensure score stays 0..=100 with bands at 30/60
4. Train on ~1000 labeled turns (architecture spec's honest sizing estimate)

Every `remember_with_salience` produces labeled data for the training set (the resulting tags ARE the labels).

To add a new signal:
1. Add a `*_score(lower)` fn returning bounded contribution
2. Fold into the `gross / penalty` computation in `score_turn`
3. Optionally add a tag emission for the new signal

## Known limitations

- Heuristic signals fail on non-English content
- Explicit-marker list is English-specific; internationalization is a v1+ concern

## See also

- [Row 32 Episodic writer](row-32-episodic-writer.md)
- [Row 41 Surprise detector](row-41-surprise.md) — complementary signal path
- [Episodic pillar](../04-four-pillars/episodic.md)

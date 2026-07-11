# Pillar — Semantic

Distilled facts. Persistent, user-ranked-up, decays if untouched. This is where "User is vegetarian" lives, not the raw conversation turn that established it.

## What goes here

- Preferences, traits, facts about users / agents / entities
- Policies and agreements reached
- Summaries produced by the caller's LLM at read time
- Imports from competitors that carry fact-shaped rows (mem0 `preference` category → Semantic)

## Writer APIs

```rust
brain.remember_with_pillar(doc_id, content, title, Pillar::Semantic, extra_tags);
```

From MCP:

```json
{"name": "remember", "arguments": {"content": "User is vegetarian", "pillar": "semantic"}}
```

From migration:

```
said import from --from mem0 --source memories.jsonl
# mem0 category "preference" auto-routes to Semantic
```

## Tags applied automatically

- `pillar:semantic`
- `salience:<band>` if written via `remember_with_salience`
- `imported_from:<system>` + `user_id:<id>` when coming from migration adapters

## Retrieval ranking (planned)

From the architecture spec:

```
score = SCA_score × confidence × (1 - decay)
```

where `confidence` comes from a tag like `confidence:0.85` (not yet populated anywhere) and `decay` comes from a `semantic.decay_halflife` counter tick on untouched frames (also not yet implemented).

**Current reality** — Semantic frames rank via the standard SCA + BM25 pipeline with no special per-pillar weighting. The confidence + decay formula is documented for when the dream layer starts producing Semantic frames with explicit confidence hints.

## Where Semantic frames come from today

Under BYO-LLM, Semantic frames are **authored by callers**, not synthesized by `.said`. Typical flows:

**Caller LLM consolidation:**
```
1. agent receives user statement "I like dogs"
2. agent persists raw turn: MCP remember pillar=episodic
3. agent (optionally, at its own judgment) distills:
   MCP remember pillar=semantic content="User likes dogs"
```

**Migration adapters:**
```
said import from --from mem0 --source memories.jsonl
# Every mem0 row with category "preference" lands as Semantic
```

**Admin CLI:**
```
# After `said ask --deep "summarize this week"` yields a user-readable summary,
# the caller can persist it:
said remember pillar=semantic --content "This week Alice shipped the payment gateway"
```

## Interaction with Surprise detector

Decision 4's Surprise detector runs at every `remember_with_salience` call. When a new Semantic frame would **contradict** an existing Semantic frame about the same topic (high similarity, moderate token overlap), the new frame is tagged `reconsolidation:contradicts` + `contradicts:<prior_doc_id>`. Both frames stay — the tag is the signal.

This lets retrieval tools (admin UI, caller-side LLM) surface conflicts without silently overwriting.

## Dream v2 note — why there's no auto-Semantic

Decision 5 v2 (2026-04-22) removed the content-consolidation path. The rationale: deterministic clustering without language understanding produces noisy "facts" that hurt retrieval. Caller LLMs at read time are the right consolidator.

The brain-state dream (fingerprint-threshold drift, S_slow, recall-weight decay) is still active and affects Semantic frames like any other pillar.

## Typical queries

```
said ask "what do we know about Alice?"                 # ← Semantic dominates
said ask pillar=semantic "user preferences"             # ← explicit scope
said ask "contradicts:sem_142"                          # ← find conflicts with a fact
```

## How to test

```rust
sf.remember_with_pillar(
    Some("fact_veg"),
    "User is vegetarian",
    None,
    Pillar::Semantic,
    vec![],
);
// Later — re-write with a correction; surprise detector fires
sf.remember_with_salience(
    Some("fact_veg_v2"),
    "User is vegan, not vegetarian",
    None,
    Pillar::Semantic,
    vec![],
);

let meta = sf.frames.get_meta("fact_veg_v2").unwrap();
assert!(meta.tags.iter().any(|t| t == "reconsolidation:contradicts"));
```

## See also

- [Row 41 Surprise detector](../05-features/row-41-surprise.md)
- [Row 35 Dream v2 (brain-state only)](../05-features/row-35-brain-state-dream.md)
- [Row 48 Migration adapters](../05-features/row-48-migration.md) — mem0 preference → Semantic mapping

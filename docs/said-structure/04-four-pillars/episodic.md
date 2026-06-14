# Pillar — Episodic

Raw turns, session events, tool completions. Append-only. This is where `.said` captures what actually happened, verbatim.

## What goes here

- Conversation turns between a user and an agent
- Session-end summaries (the whole session flushed as one Episodic frame)
- Per-tool-completion events (agent ran tool X, got result Y)
- Any event-shaped remember that wasn't promoted to another pillar

## Writer APIs

```rust
// Explicit write (caller chooses pillar)
brain.remember_with_pillar(doc_id, content, title, Pillar::Episodic, extra_tags);

// Scored write — runs salience + surprise detection
let (frame_id, salience) = brain.remember_with_salience(
    doc_id, content, title, Pillar::Episodic, extra_tags,
);

// CLI convenience (defaults to Episodic when no pillar specified)
// via MCP remember tool — also defaults to Episodic
```

From MCP:

```json
{"name": "remember", "arguments": {"content": "...", "pillar": "episodic"}}
{"name": "session_end", "arguments": {"summary": "..."}}
{"name": "tool_completion", "arguments": {"tool": "bash", "args": "...", "result": "..."}}
```

## Tags applied automatically

- `pillar:episodic`
- `salience:<low|medium|high>` (from `score_turn`)
- `reconsolidation` + `reconsolidation:contradicts` + `contradicts:<doc_id>` if surprise detector fires (see [Row 41](../05-features/row-41-surprise.md))
- `session_end:true` for session-flush writes
- `tool_completion:true` + `tool:<name>` for tool hooks

## Retrieval ranking

Recency-weighted — Generative Agents pattern. The formula from the architecture spec:

```
score = SCA_score × exp(-age_hours / τ)
```

with `τ = 24` hours by default. Implemented in `recall::rerank_by_pillar(hits, pillar_of, age_hours_of, tau_hours)`.

Opt-in: callers of `search_full_scoped_pillars` can pass Episodic ranking by supplying the `age_hours_of` closure. Default retrieval (without the opt-in) uses the standard SCA + BM25 + graph fan-out pipeline across all pillars.

## Typical queries

```
said ask "what did Alice say about the launch?"      # ← mostly returns Episodic
said ask "when did we last discuss retention policy?" # ← recency matters, Episodic wins
MCP search pillar=episodic query="deployment"        # ← explicit scope
```

## Auto-dream interaction

Every `remember` of any pillar updates the Brain's `S_slow` tensor and accumulates the query embedding for dream-cycle drift. Episodic frames in particular dominate the S_slow signal because they're the highest-volume pillar in most brains — meaning the fingerprint threshold drifts toward whatever dialogue-style vocabulary is actually in use.

## Dream consolidation — intentionally disabled

Decision 5 v1 had a path that clustered Episodic frames and wrote Semantic summaries. It hurt LoCoMo by ~2 points (clustering without LLM-level semantics produces noisy summaries). Decision 5 v2 stubbed it as a no-op.

**Callers who want Episodic→Semantic consolidation run their own LLM at read time:** query `.said` with `pillar=episodic`, pass results to their LLM, have it produce a summary, then call `brain.remember_with_pillar(..., Pillar::Semantic, ...)` with that summary. This is the BYO-LLM pattern.

## How to test

```rust
let mut sf = SaidFile::create("test.said");
sf.engine.load_static_encoder("SAID-LAM-private/said-lam-static")?;
sf.engine.core.set_holographic_16view(false, None);

let fid = sf.remember_with_pillar(
    Some("turn_1"),
    "Alice: I think we should ship Friday.",
    Some("session-2026-04"),
    Pillar::Episodic,
    vec![],
);

let meta = sf.frames.get_meta("turn_1").unwrap();
assert_eq!(meta.pillar, Pillar::Episodic);
assert!(meta.tags.contains(&"pillar:episodic".to_string()));
```

## Known limitations

- Default retrieval doesn't yet apply the per-pillar recency decay automatically. Callers must route through `rerank_by_pillar` to get it. See [Row 31 — per-pillar retrieval scope](../05-features/row-31-per-pillar-retrieval.md).

## See also

- [Row 32 Episodic writer + hooks](../05-features/row-32-episodic-writer.md)
- [Row 33 Salience scorer v0](../05-features/row-33-salience.md)
- [Row 41 Surprise detector](../05-features/row-41-surprise.md)
- [3.3 Brain](../03-core-subsystems/3.3-brain.md) — S_slow, recall-weight, dream

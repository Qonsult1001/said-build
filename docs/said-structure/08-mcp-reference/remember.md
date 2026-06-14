# MCP tool: remember

Store text as a memory. Routes through [`remember_with_salience`](../../../crates/sca-core/src/said_file.rs) so every call gets salience scoring + Surprise / reconsolidation detection auto-applied.

## Schema

```json
{
  "name": "remember",
  "arguments": {
    "content": "string, required",
    "id": "string, optional (auto-generated if omitted)",
    "title": "string, optional",
    "pillar": "string, optional — episodic (default) | semantic | procedural | external | code | memory",
    "tags": "array of strings, optional"
  }
}
```

## Description

> Store a piece of text in the brain as a searchable memory. Use for notes, decisions, conversation summaries, user preferences. Routes through salience scoring (Row 33) and Surprise detection (Row 41) — so contradictions with prior memories get surfaced automatically.

## Behavior

1. Parse `pillar` string to `Pillar` enum (default Episodic on unknown / missing)
2. Call `brain.remember_with_salience(id, content, title, pillar, tags)` which:
   - Runs `salience::score_turn` — 8 signals, produces band + tags
   - Runs `find_prior_match` + `classify_surprise` — semantic contradiction detection
   - Merges tags (caller + salience + surprise)
   - Writes via `remember_with_pillar` — which audit-logs + sets pillar byte
3. `brain.build_index()` + `brain.save()`
4. Fetch the written frame's tags back for the response
5. Return a result line describing the new frame + any surprise findings

## Response format

Base:
```
✓ Saved to brain (frame #142, pillar=episodic). salience=35 (medium)

Brain now has 1847 total frames. This memory is searchable — future `search` calls can find it.
```

With contradiction:
```
✓ Saved to brain (frame #142, pillar=semantic). salience=35 (medium) · ⚠ contradicts prior frame `fact_veg`
...
```

With topical update:
```
✓ Saved to brain (frame #142, pillar=semantic). salience=35 (medium) · ↻ updates prior frame `plan_v1`
...
```

Agents consuming the tool can regex the note line for `⚠` / `↻` + the referenced doc_id to surface conflicts to the user.

## Example

```json
{"method":"tools/call","params":{"name":"remember","arguments":{
  "content":"User is vegetarian",
  "pillar":"semantic",
  "id":"fact_veg"
}}}
```

Then later:

```json
{"method":"tools/call","params":{"name":"remember","arguments":{
  "content":"actually, user is vegan not vegetarian",
  "pillar":"semantic"
}}}
```

Response includes `⚠ contradicts prior frame fact_veg` — because lexical `actually` + semantic overlap both fire.

## Auto-side-effects

Every `remember` call:
- Produces an audit entry
- Updates S_slow tensor
- Updates the doc's recall_weight (first-write = fresh)
- Mark-populated on the brain so `open` doesn't later collapse it to a placeholder

## See also

- [Row 32 Episodic writer + hooks](../05-features/row-32-episodic-writer.md)
- [Row 33 Salience scorer](../05-features/row-33-salience.md)
- [Row 41 Surprise detector](../05-features/row-41-surprise.md)
- [CLI said add / remember](../07-cli-reference/other-commands.md#said-add--said-remember)

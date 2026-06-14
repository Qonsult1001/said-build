# SAID Memory Architecture — Four Pillars

**The portable `.said` memory model, aligned with cognitive neuroscience**

Version 0.1 · April 2026

---

## Overview

SAID memory is organized into four pillars, each with a distinct cognitive-science analog, a distinct storage strategy, and a distinct retrieval pattern. A **dream function** runs consolidation over the episodic pillar, routing survivors to the appropriate long-term pillar.

| Pillar | Cog-sci analog | Write policy | Role |
|---|---|---|---|
| **s-fast** (episodic) | Hippocampus | Append on every event boundary | Raw trace of experience |
| **s-slow / semantic** | Neocortex (declarative) | Written only by dream function | Distilled facts, beliefs, preferences |
| **s-slow / procedural** | Basal ganglia + cerebellum | Written on successful action sequences | Skills, workflows, how-to patterns |
| **External** | Extended mind (Clark & Chalmers) | Indexed on ingestion | Pointers to docs, URLs, Excel, DBs, APIs |

---

## Pillar 1: s-fast (episodic)

**Append-only trace of everything that happens.** No judgment, no filter, no LLM call. The hippocampal analog.

**Write triggers:**
- Turn boundary (with gating predicate)
- Topic shift detected via embedding centroid drift
- Tool-call completion
- Plan/subgoal completion (POMDP belief-state update)
- Explicit user marker (`/remember`, "save that")
- Session end

**Entry schema:**
```json
{
  "ts": "2026-04-21T10:15:03Z",
  "actor": "user | agent | tool",
  "content": "...",
  "context_embedding": [64 floats],
  "modality": "text | tool_result | observation",
  "session_id": "...",
  "salience_score": 0.74
}
```

**Storage:** `.said/episodic.jsonl` — append-only, no dedup.

**Lifecycle:** Raw entries retained 30 days or until consolidated by the dream function, whichever comes later.

---

## Pillar 2: s-slow — semantic

**Distilled declarative knowledge.** Facts about the user, the world, the project. Written *only* by the dream function. The neocortical analog.

**Entry schema:**
```json
{
  "id": "sem_...",
  "content": "User prefers DD/MM/YYYY date format",
  "domain": "personal",
  "confidence": 0.92,
  "provenance": ["ep_...", "ep_...", "ep_..."],
  "created": "...",
  "last_reinforced": "...",
  "reinforcement_count": 3,
  "decay_halflife_days": 30,
  "embedding": [64 floats]
}
```

**Decay rule:** entries decay unless reinforced. Reinforcement (being retrieved, or matching a new episodic trace) resets the counter. Entries reinforced ≥ N times stop decaying — matches CLS: replayed traces get consolidated harder.

**Storage:** `.said/semantic/<domain>.said` — one file per Latent Firm Taxonomy domain (architecture, backend, frontend, cloud, security, personal, …).

---

## Pillar 3: s-slow — procedural

**How-to knowledge.** Sequences of actions that produced good outcomes. Skills, workflows, successful tool chains.

**Write condition:** dream function detects `(action_sequence, outcome=success)` pattern in episodic batch.

**Entry schema:**
```json
{
  "id": "proc_...",
  "name": "deploy_to_azure_sa_north",
  "trigger_conditions": "...",
  "steps": [...],
  "success_rate": 0.87,
  "invocation_count": 14,
  "embedding": [64 floats]
}
```

**Storage:** `.said/skills/*.said` — one file per skill, consistent with existing SKILLS.md convention.

**Retrieval:** task-match — given a new task, retrieve the k most similar procedural entries and their success rates.

---

## Pillar 4: External (extended mind)

**Pointers to structured or referenceable data.** Not experiences — *handles*. Documents, URLs, Excel files, DB connections, API endpoints, images.

**Critical design rule:** the raw contents never enter the episodic buffer or the dream function. Only the pointer + metadata is indexed.

**Entry schema:**
```json
{
  "id": "ext_...",
  "type": "pdf | excel | url | db | api | image | csv",
  "uri": "s3://... | postgres://... | https://...",
  "title": "Q3 2025 Sales Report",
  "summary": "...",
  "summary_embedding": [64 floats],
  "schema": {...},           // for structured sources
  "access_pattern": {
    "credentials_ref": "...",
    "ttl_seconds": 3600,
    "read_only": true
  },
  "content_hash": "...",
  "indexed_at": "...",
  "last_accessed": "..."
}
```

**Storage:** `.said/references.jsonl` or `.said/references/<type>.jsonl`.

**Retrieval:** metadata-filter first, schema-aware query second. The semantic pillar points *to* external entries ("there's a spreadsheet with that data, here's the handle"); the agent then fetches live contents on demand.

---

## The dream function

**Consolidation pass that promotes episodic → s-slow.**

### Trigger (layered)

1. **Primary — salience-accumulated.** Running sum of per-entry salience scores crosses threshold (default 150, following Generative Agents). Fires when dense/important activity has occurred, regardless of turn count.
2. **Secondary — counter.** Every 100 episodic entries, run the dream function as a safety net.
3. **Tertiary — scheduled.** Session end and nightly cron, always.

Use the salience-accumulator as the main trigger. The fixed-100 counter catches low-salience sessions. Scheduled catches everything else.

### Pipeline

1. **Cluster** recent episodic entries by topic embedding.
2. **Score** each cluster for salience: novelty, reward signal, recurrence, user emphasis, contradiction count.
3. **Classify shape** of each surviving cluster:
   - Fact-shaped → route to **semantic**
   - Sequence with outcome → route to **procedural**
   - Reference mentioned (URL, file, DB, API) → route to **external** as pointer
   - Conversational noise → drop
4. **Dedup** within the target domain — cosine threshold 0.92 against existing entries. Merge on match, create on miss.
5. **Cascade** — update existing s-slow entries in light of the new batch (A-MEM memory-evolution step). Contradictions are preserved as first-class disagreements, not silently overwritten.
6. **Decay tick** — reduce half-life counter on all s-slow entries not touched this round.
7. **Compress** the consolidated episodic batch into a single meta-summary entry. Archive raw if storage permits.

---

## Recall — one pillar, one strategy

The four pillars require different retrieval patterns. Lumping them is why most agent memory systems retrieve poorly.

| Pillar | Retrieval |
|---|---|
| Episodic | Recency-weighted cosine (Generative Agents formula) |
| Semantic | Relevance-weighted cosine with confidence gating |
| Procedural | Task-match retrieval (Voyager-style skill retrieval) |
| External | Metadata-filter + schema-aware query (never hallucinate structured data) |

SAID's existing hash recall → SCA recall → SSP-MCTS pipeline maps onto this: different recall phases for different pillar types, with the routing now explicit.

---

## `.said/policy.yaml` v0.1

```yaml
version: 1

pillars:
  episodic:    .said/episodic.jsonl
  semantic:    .said/semantic/*.said
  procedural:  .said/skills/*.said
  external:    .said/references.jsonl

triggers:
  event_boundary:
    - type: session_end
      action: append_episodic + dream_function
    - type: tool_completion
      condition: "result.success and returns_fact"
      action: append_episodic
    - type: topic_shift
      detector: embedding_centroid
      window: 8
      threshold: 0.55
      action: append_episodic
    - type: plan_step_complete
      action: append_episodic

  surprise:
    - type: correction
      markers: ["actually", "no,", "wrong", "not X"]
      semantic_detector: contradiction_classifier_v1
      action: append_episodic
      priority: high
      tag: reconsolidation
    - type: memory_contradiction
      detector: cosine_with_inversion
      threshold: 0.85
      action: append_episodic + flag_for_dream

  salience:
    - type: learned_classifier
      model: said-salience-v1    # Model2Vec 64-dim + linear head
      min_score: 0.7
      action: append_episodic
    - type: accumulated
      scorer: salience_classifier
      threshold: 150
      window: rolling
      action: dream_function

  explicit:
    - type: user_marker
      markers: ["/remember", "remember this", "save that"]
      action: append_episodic
      priority: highest
    - type: user_forget
      markers: ["/forget", "forget about"]
      action: tombstone

dream_function:
  trigger:
    primary: salience_accumulator_over_150
    secondary: every_100_episodic_entries
    tertiary: [session_end, nightly]
  pipeline:
    - cluster_by_topic
    - score_salience
    - classify_shape             # fact | sequence+reward | reference | noise
    - route_to_pillar
    - dedup
    - cascade_links
    - decay_tick
    - compress_batch

routing:
  fact_shaped:      semantic
  sequence_reward:  procedural
  reference:        external
  noise:            drop

retrieval:
  episodic:    recency_weighted_cosine
  semantic:    relevance_weighted_cosine_with_confidence
  procedural:  task_match
  external:    metadata_filter + schema_query
```

---

## Daemon API

Three entry points. MCP, Claude Code hooks, raw Python — all adapters on top of these.

```
said.ingest(turn)         → evaluates triggers, may fire writes
said.dream()              → forces consolidation pass
said.query(context, pillar=auto)  → routes to correct retrieval strategy
```

---

## Cognitive-science anchor points

| Design choice | Neuroscience basis |
|---|---|
| Event-boundary write triggers | Event Segmentation Theory (Zacks et al., 2007); boundary-locked hippocampal activity predicts later retrieval (Ben-Yakov & Dudai, 2011) |
| Surprise as high-priority trigger | Prediction-error drives hippocampal reconsolidation (Sinclair et al., PNAS 2021) |
| Two-stage fast/slow architecture | Complementary Learning Systems (McClelland, McNaughton & O'Reilly, 1995; Spens & Burgess, Nature Human Behaviour 2024) |
| Salience-accumulated consolidation trigger | Generative Agents importance threshold (Park et al., UIST 2023); synaptic tag-and-capture (Frey & Morris, 1997) |
| Four pillars | Tulving's episodic/semantic/procedural taxonomy + Clark & Chalmers' extended mind |
| Retroactive strengthening on reinforcement | CLS replay; behavioral salience modulating consolidation (iScience, 2024) |
| Contradictions preserved, not overwritten | Reconsolidation literature; conflict-as-data (open problem in current agent memory) |

---

## Implementation priority

1. **Pillar skeleton** — create the four files/directories, define schemas.
2. **s-fast writer** — event-boundary + explicit triggers only. Deterministic, no ML. ~70% of value.
3. **Salience classifier v1** — Model2Vec 64-dim + linear head. Few hundred labeled turns.
4. **Dream function v1** — heuristic pipeline, no RL yet. Fires on every-100 + session-end.
5. **Surprise / reconsolidation** — contradiction detector, UPDATE-with-history.
6. **External pillar indexer** — mime-type router, metadata extractor, pointer-only storage.
7. **RL-tuned dream policy** — defer until outcome data is available (Memory-R1 needed 152 QA pairs).

# A/B result — claude-native (no `.said`) vs `.said`, head-to-head

The honest "is it unique / does it save" test. Same two builds, same architecture skills, same spec, same
model — the ONLY variable is `.said` memory ON vs OFF. Four real Claude-agent runs total (2 already done
with `.said`, 2 native).

## The numbers (measured, real agent token + wall-time)

| Build | `.said` tokens | native tokens | token Δ | `.said` time | native time | `.said` files | native files |
|---|---|---|---|---|---|---|---|
| **C# (qonsult DDD)** | 100,025 | 124,669 | **`.said` −24,644 (−20%)** | 936 s | 711 s | 63 | 107 |
| **Rust (workspace)** | 87,170 | 82,503 | **`.said` +4,667 (+6%)** | 740 s | 564 s | 39 | 38 |

## The honest read (no spin)

- **It split — which is the most truthful outcome.** On the larger C# build `.said` used **20% fewer
  tokens**; on the smaller Rust build it cost **6% more**. There is a real **crossover**: `.said`'s
  per-call overhead (each `recall-blueprint`/`learn-fix`/`init` spends tokens + a subprocess) is NOT
  amortized on a small build, but IS paid back once the repeated structure is large enough.
- **`.said` was SLOWER in BOTH** (936 vs 711 s; 740 vs 564 s). The recall/learn/init subprocess calls add
  wall-clock even when they save tokens. Memory costs time to maintain.
- **The agents' own words are the mechanism.** Native Rust: *"I had no memory tool, so for each module I
  re-read my prior files and re-typed each file from that template … the canonical body … re-derived and
  re-written every single time (4×)."* Native C#: *"I re-typed each new service from that mental template …
  the repetition was entirely manual re-typing of the same skeleton — exactly the pattern-reuse cost a
  memory-equipped arm would avoid."* So the duplication cost `.said` targets is REAL; whether avoiding it
  nets out positive depends on scale.

## The confound (must be stated)

The C# −20% is NOT a perfectly controlled diff: the native arm wrote **107 files vs 63** — same 4 bounded
contexts, but native split into more, smaller per-slice files (Accounts 28, Search 34). Part of the 24,644-
token gap is genuine skeleton re-derivation `.said` would recall; part is native simply producing more
output. So "`.said` is 20% cheaper on C#" is directionally true and measured, but it is NOT a clean
"identical-output" benchmark. One run per arm = one sample (LLM nondeterminism). We report the numbers and
the trend; we do NOT claim a precise "Nx cheaper."

## What this DOESN'T test (where `.said`'s real value is)

This A/B is a single in-session build that **never compacted** and used **one agent at a time**. It does
NOT exercise the things `.said` is actually for:

- **Compaction-survival** (the headline moat) — the build was short enough that no `/compact` fired, so the
  out-of-band brain's main job (surviving the lossy compaction that makes agents "go stupid") was never
  needed. On a long session that compacts repeatedly, the native agent loses its in-context template and
  must re-read; `.said` recalls it byte-exact. THAT is where the token/time math flips decisively, and a
  short build hides it.
- **Cross-session / cross-agent / cross-project reuse** — native built C# and Rust as two SEPARATE efforts
  with zero shared learning. `.said` carried the **byte-identical canon** across both (proven in
  `real/REAL-RESULT.md`). Native structurally CANNOT do this — a new session/agent/project starts cold.
- **Cross-language canon** — the `validate→authorize→repository→DTO→return` 80% learned in C# was rendered
  in Rust from the same stored phrases. No per-tool memory (Claude/Kimi/Cursor) reuses across languages.

## Honest conclusion

- **Per-build token savings are NOT a reliable headline** — they depend on scale (C# yes, Rust no) and are
  offset by slower wall-time. Do not market "`.said` saves tokens" from this.
- **`.said`'s uniqueness is NOT per-build token count** — it is **persistence across compaction, sessions,
  agents, projects, and languages**, none of which a single short native build can replicate. The A/B
  confirms what `.said` is FOR (durable, compounding, shared memory) by showing what it is NOT for (shaving
  tokens off one small from-scratch build).
- The right next measurement to prove the savings claim is a **long, compaction-triggering, multi-session
  build** (native loses context and re-reads; `.said` re-grounds) — that is where the math should clearly
  favor `.said`, and it is the moat the docs actually claim (doc 30 compaction-survival), not single-build
  token shaving.

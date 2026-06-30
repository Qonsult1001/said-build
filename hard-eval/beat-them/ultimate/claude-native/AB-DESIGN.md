# A/B baseline — claude-native (no `.said`) vs `.said`

The honest "is this unique / does it actually save" test. Run the SAME two builds a third way — **claude-
native: a normal Claude agent with NO `.said` at all** — and compare **tokens + wall-time** against the
`.said` runs already done. Only ONE variable differs: `.said` memory on vs off. Everything else identical
(same spec, same architecture skill, same target structure, same model).

## Fairness controls

| Held CONSTANT (both arms) | The ONE variable |
|---|---|
| the build brief / spec (qonsult 36-endpoint C#; same Rust service) | `.said` memory: **ON** (recall-blueprint / learn-fix / harvest / sym) vs **OFF** (none) |
| the architecture skill (architecture-csharp / architecture-rust) | |
| the 4 contexts/modules + 4-layer structure | |
| the model (same Claude) | |

Native agents build the same thing with plain file ops only — to reuse a pattern across contexts they must
re-derive it from their own context / re-read prior files (the doc-28 grep-and-read cost `.said` avoids
via blueprint recall).

## The `.said` arm (already measured — `ultimate/real/`)

| | qonsult C# (.said) | project2 Rust (.said) |
|---|---|---|
| tokens | 100,025 | 87,170 |
| wall-time | 936 s (15.6 min) | 740 s (12.3 min) |
| files | 63 `.cs` | 39 `.rs` |
| compounding | recall 0→0.86→0.89 | recall 0→0.87→0.89 |

## The claude-native arm (this folder — to measure)

Same builds, no `.said`. We capture tokens + time + files, then compute the delta.

## What the result tells us

- **Uniqueness / savings**: native_tokens − said_tokens (and time). If `.said` is lower, the memory paid
  for itself; if higher, be honest about it.
- **Compounding (the better signal than counting blueprints)**: the GAP should WIDEN per context — context
  #1 is similar both arms (nothing to recall yet); by context #4 the `.said` agent recalls the established
  80% while native re-derives it. Watch tokens-per-context, not just the total.

## Honest caveat baked in

A single build is one sample (LLM nondeterminism). The total token delta is indicative, not a clean
benchmark; the per-context trend (does the gap grow?) is the more trustworthy signal. We report both, and
we do NOT claim a precise "Nx cheaper" from one run — we report the measured numbers and the trend.

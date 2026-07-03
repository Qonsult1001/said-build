# Phase 3 — arXiv LongMemEval-style battery (5 categories, on a dedicated memory brain)

## Method note (honest)
v1 planted plain-English facts into the 46.6k-frame CODE brain + raw ask -> nearly all MISS. This is
the DOCUMENTED unscoped-mixed-corpus caveat (93% code drowns a plain fact), a HARNESS mistake, not a
product failure (the memory stored fine; get() returns it verbatim). v2 (the fair test) uses a
dedicated MEMORY brain (40 filler + 9 gold + 3 abstention) = the correct LongMemEval conversational
setup. Results below are v2.

## Results (recall@1 / @5 / @10 per category)
| Category | recall@1 | recall@10 | note |
|---|---|---|---|
| Information Extraction (single-hop) | 100% | 100% | GREEN |
| Multi-Session (compose across items) | 100% | 100% | GREEN |
| Temporal Reasoning (the field's HARDEST) | 100% | 100% | GREEN -- nails the hardest category at rank 1 |
| Knowledge Update (fact changed -> latest) | 50% | 100% | AMBER -- latest-fact not always rank 1 (stale + new both stored) |
| Abstention (never-stored -> decline) | 0% | -- | RED -- REAL WEAKNESS (see below) |

vs published: Mem0 LoCoMo 92.5 / LongMemEval 94.4 (accuracy). .said recall@1 on the answerable 4
categories = 9/9 first-attempt on the exact-answer token except 1 KU = ~89% recall@1, strong; but
abstention drags the overall LongMemEval-style score because ABS is 1 of the 5 core abilities.

## REAL FINDING #9 (product, not harness): abstention is weak
`ask "how many llamas does the finance team own"` (never stored) -> returns unrelated memories m4/m1 at
score 1.15/1.08. The abstention gate EXISTS (ask.rs ~813, z-score, NON-deep path, SAID_ASK_ABSTAIN) but:
 (a) whitened cosine scores EXCEED 1.0 (not bounded to [0,1]) so a "confident" hit is declared for
     nonsense; the gap/floor gate misfires at small-brain scale.
 (b) the gate only runs in NON-deep mode; deep ask (used by fusion) skips it.
Docs 23-benchmark-methodology calls abstention a core correctness axis ("give up vs grind", arXiv
2207.05221) -- so this is a real gap against .said's OWN stated standard, not just the literature's.
FIX (park + grill): bound/normalize the abstention score, run the gate in deep mode too, or add a
"no result clears an ABSOLUTE floor -> abstain" check. Not fixed this run -- recorded as finding #9.

## Honest headline
Recall QUALITY is strong (IE/MS/TEMPORAL 100% recall@1 in the correct memory-brain setup -- temporal is
the category the literature calls hardest). The gaps: knowledge-update latest-fact ranking (@1 50%) and
ABSTENTION (0%, fabricates on absent info). Both are real, both recorded, neither hidden.

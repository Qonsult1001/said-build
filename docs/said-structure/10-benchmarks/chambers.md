# Chambers — stress-test suites

A **chamber** is a small, purpose-built test corpus that isolates one behaviour the brain must exhibit. Where MTEB and LoCoMo measure aggregate quality, chambers verify a specific property on a small, deterministic fixture.

## The 30-chamber pass rate

Row 19 of `SAID_MVP_PLAN.md` requires **30/30 chambers passing**. Each chamber is a pair:

- `fixture/` — small corpus (3 – 30 frames)
- `probe.rs` — a harness that ingests, queries, and asserts the top result is the expected one

Current count: 30 shipped chambers, all passing in the nightly sweep.

## Shipped chambers (examples)

### Retrieval boundaries

- **Hyphen asymmetry** — query `"well-being"` must match document `"wellbeing"` and vice versa. Gated by the normalize_for_match hyphen-strip in the query-variant expansion.
- **Entity-speaker boost** — query `"Alice said the feature ships Thursday"` must prefer frames with `speaker:alice` tag over frames containing the word "alice" in free text.
- **Negation** — query `"did NOT approve the deal"` must not return a frame that says `"approved the deal"`. Uses BM25 negative-term weighting.
- **Multi-hop bridge** — query `"what does Bob think about X"` where no single frame answers; must bridge Alice→Bob via a third frame.

### Pillar routing

- **Episodic-only scope** — query with `pillar=episodic` must drop every Semantic/Procedural/Code frame even if they score higher.
- **Code symbol precision** — query `"validate_user"` must hit the function symbol before any free-text mention.

### Admin + compliance

- **Legal hold blocks retention** — after `legal-hold-add doc_X CASE-A`, a retention sweep with `--older-than-days 0` must leave `doc_X` intact.
- **Restore round-trip** — delete → tombstone → restore must return the exact original frame (byte-for-byte content hash match).
- **Audit chain break detection** — flip one byte in an AUDT entry; `audit --verify` must exit non-zero and name the first broken entry.

### Ingestion edge cases

- **DOCX with embedded images** — must extract every paragraph even when the document has inline images between runs.
- **Scanned PDF fallback** — a PDF with no extractable text must route through OCR when the `ocr` feature is on.
- **UTF-8 BOM handling** — a SQL file starting with `\u{FEFF}` must be ingested without the BOM leaking into the first chunk.

### Brain state

- **Dream threshold monotonicity** — with N frames, dream_threshold(N) must increase monotonically with N.
- **S_slow recovery** — after reloading a brain from disk, S_slow must match the pre-save tensor exactly (f32 bitwise).

## Why chambers and not just MTEB

MTEB is aggregate. A small regression in one behaviour (e.g. negation handling) can be offset by an improvement elsewhere and still show +0.001 on the mean. Chambers catch the individual behaviour regardless.

Rule of thumb: if you shipped a code path that a future engineer might misunderstand, there should be a chamber that fails loudly when they accidentally break it.

## Adding a new chamber

1. Pick a single behaviour to protect. If you're about to write "and also…", split into two chambers.
2. Create a fixture under `crates/sca-core/examples/chambers/<name>/` — 3 to 30 frames as plain `.txt` or `.md` files is ideal.
3. Write a probe (`chamber_<name>_probe.rs`) that: ingests the fixture, runs the canonical query, asserts the expected doc_id is #1 (or in top-N), and prints PASS / FAIL with a deterministic JSON summary.
4. Register it in `crates/sca-core/Cargo.toml` as a new `[[example]]` with `required-features = ["static-embed"]`.
5. Add it to the nightly runner script. Row 19 pass count bumps by 1.

## Running all chambers

```bash
cd crates/sca-core
for chamber in examples/chambers/*/probe.rs; do
  name=$(basename $(dirname $chamber))
  cargo run --release --example chamber_${name}_probe --features static-embed || echo "FAIL: $name"
done
```

A clean run prints 30 green PASS lines and exits 0.

## See also

- [Real-world probes](realworld-probes.md) — larger corpora, less isolated behaviours
- [11-known-limitations.md](../11-known-limitations.md) — what chambers are *intentionally* not protecting yet

# Future enhancement — collapse document extraction into ONE place (deferred)

**Status:** deferred. Tracked for a later refactor; not blocking. The two paths work today and
were deliberately kept separate (different jobs); this note records *why* and *what* a future
unification would do, so the decision isn't re-litigated from scratch.

## The two DOCX/PDF parsers today (intentionally separate plugins)

| Path | Crate | Job | Searchable text | 1:1 restore |
|------|-------|-----|-----------------|-------------|
| `document_ingest::extract_docx_bytes` | `sca-core` | **search index** — extract text → frames | body `<w:p>` + **header/footer** parts (table cell text included; structure flattened) | ❌ not its job |
| `said_vault::parser::docx::parse` | `said-vault` | **compliance archive** — 1:1 restore | body `<w:p>` (same paragraph view) | ✅ preserves **raw bytes** of every zip part (headers, footers, tables, numbering, fonts, media, customXml) so `rebuild` re-emits byte-identical |

Both extract the *same searchable text* (paragraph `<w:t>` walk). The vault's 1:1 magic is **raw
byte preservation for restore**, not richer text. They are separate because `sca-core` must NOT
depend on `said-vault` (vault depends on core; that'd be circular), and because their outputs
differ (text segments vs raw-byte structural parts).

## What was changed (and why it stayed in document_ingest, not vault)

`document_ingest::extract_docx_bytes` now also reads `word/headerN.xml` + `word/footerN.xml`
(previously only `word/document.xml`). Header/footer text — case numbers, dates, "RE:" lines,
references — is often the most discriminating content in legal/business docs and was silently
unsearchable. Measured on the legal bench-corpus: a single cost-statement went 21→33 segments,
1097→1779 chars (+62%); "BIDFOOD NOMAGEBA TRADING" (footer-only) became recallable.

**Decision (confirmed with the product owner):** table-structure flattening is fine for SEARCH —
as long as every cell's TEXT is present and recallable, rows/cols don't matter for finding it.
Structure only matters for 1:1 restore, which the vault already owns. So we fixed the *search*
plugin (header/footer text) and did NOT touch the vault.

## The future unification (deferred)

A later refactor could collapse the two into **one DOCX/PDF parser** with two output views:
- a **text view** (segments) for the search index, and
- a **structural view** (raw parts + hashes) for 1:1 restore.

Shape: put the single parser where both can reach it without a cycle — either a small
`said-doc-parse` leaf crate that `sca-core` and `said-vault` both depend on, OR keep it in
`sca-core` and have `said-vault` depend on core for the text view while adding its raw-byte
preservation on top. Then `said init` could optionally keep the vault manifest (1:1 restore) for
documents — genuinely valuable for legal/compliance — from the same single parse pass.

Why deferred: the two paths are correct today, the dependency inversion needs care (avoid the
cycle), and the search win (header/footer text) was the only urgent gap. Unification is hygiene +
a restore-on-init feature, not a recall fix — so it waits.

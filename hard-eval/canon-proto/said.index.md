---
type: said.canon.index
spec: okf/1.0
project: said-demo
---

# Said Canon Index (the wiki)

Library drill-down: project -> file -> section. Click a file to open the code. Each section tag
says: **can I edit it?** GENERATED = .said rewrites it (don't edit). YOURS = .said keeps it (edit freely).
How to change a GENERATED section, eject, review: see "Controls" at the bottom -- stated ONCE.

## Files (3)

### [invoice.rs](invoice.rs) -- 6 sections (3 YOURS / 3 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | accept-and-audit | GENERATED -- no |
| S2 | idempotency | GENERATED -- no |
| S3 | guards | YOURS -- yes |
| S4 | save | YOURS -- yes |
| S5 | response | YOURS -- yes |
| S6 | wrap+return | GENERATED -- no |

### [InvoiceController.v2.cs](InvoiceController.v2.cs) -- 6 sections (3 YOURS / 3 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | accept-and-audit | GENERATED -- no |
| S2 | idempotency | GENERATED -- no |
| S3 | guards | YOURS -- yes |
| S4 | save invoice | YOURS -- yes |
| S5 | response | YOURS -- yes |
| S6 | wrap + return | GENERATED -- no |

### [UpdateInvoice.cs](UpdateInvoice.cs) -- 4 sections (2 YOURS / 2 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | accept-and-audit | GENERATED -- no |
| S2 | guards | YOURS -- yes |
| S3 | update row | YOURS -- yes |
| S4 | wrap + return | GENERATED -- no |

## Totals
Across 3 files: **you own 8** sections, **.said generates 8** (re-emitted free on every entity).

## If you edit a GENERATED section (stated once)
GENERATED sections are written by .said from a saved template and get rebuilt. If you change one,
.said asks before rebuilding over it:
- **Keep my change** -- stop auto-generating just that one section.
- **Discard my change** -- restore the generated version.
- **Take ownership** -- that section becomes yours forever; .said never regenerates it.
To change a GENERATED section everywhere at once, change the saved template it comes from.

History: [said.log.md](said.log.md)

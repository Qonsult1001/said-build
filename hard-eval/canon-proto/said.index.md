---
type: said.canon.index
spec: okf/1.0
project: said-demo
---

# Said Canon Index (the wiki)

Click a file to jump to its code. Each section is wrapped `[S1] ... [/S1]`. One tag tells you the only
thing you need: **can I edit it?**

- **GENERATED** -- .said wrote it and re-writes it. Do NOT edit (your change is lost on the next render).
  To change a GENERATED section: edit the **canon** (changes it everywhere), or run `said eject S1` to
  take it over (it becomes YOURS).
- **YOURS** -- .said keeps your code here. Edit freely; it is preserved every time.

(Everything is AI-built in a new project -- so the useful question isn't "who wrote it" but "will my edit
survive". For an EXISTING codebase you scan in, .said also marks WAS-YOURS vs SAID-ADDED so you can review
exactly what it changed.)

## Files

### [InvoiceController.cs](InvoiceController.v2.cs) -- Create Invoice (dotnet)
| Section | What it does | Can I edit it? |
|---|---|---|
| [S1] accept-and-audit | take request, write audit row | GENERATED -- no |
| [S2] idempotency | reject duplicate requests | GENERATED -- no |
| [S3] guards | validate required fields | YOURS -- yes |
| [S4] save invoice | insert the invoice | YOURS -- yes |
| [S5] response | shape the response object | YOURS -- yes |
| [S6] wrap + return | wrap + audit + return | GENERATED -- no |

### [invoice.rs](invoice.rs) -- Create Invoice (rust, same canon)
Same six sections, in Rust, from the SAME canon. Only the YOURS parts differ.

## How much you wrote (this file)
- **You write 3 of 6 sections** (the YOURS parts: guards, save, response).
- **.said generated the other 3** (accept-and-audit, idempotency, wrap+return) -- and generates them again
  on every entity you create, so you never write them by hand.

Change history: [said.log.md](said.log.md)

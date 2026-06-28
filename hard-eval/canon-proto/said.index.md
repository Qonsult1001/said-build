---
type: said.canon.index
spec: okf/1.0
project: said-demo
---

# Said Canon Index (the wiki)

This is the clickable hub. Every code file's sections are catalogued here. In a Markdown viewer/IDE the
**file links below are Ctrl/Cmd-clickable** -- click to jump straight to the code. (Markdown is where
clickable links work; that is why the navigable map lives here, and each code file just points back to
`said.index.md`.)

How to read a code file: each section is wrapped `[S1] ... [/S1]`. The number is execution order. The
class on the open tag tells you who owns it:
- **Fully** = framework, written by AI once, regenerated free every time (the token saving).
- **AI** = new code the AI wrote for this entity (real work).
- **Ignore** = the 20% slot you/AI fill for this entity (real work).

## Files

### [InvoiceController.cs](InvoiceController.v2.cs) -- Create Invoice (dotnet)
| Section | What it does | Owner |
|---|---|---|
| [S1] accept-and-audit | take request, write audit row | Fully (free) |
| [S2] idempotency | reject duplicate requests | AI (net-new) |
| [S3] guards | validate required fields | Ignore (20%) |
| [S4] dml | INSERT the invoice | Ignore (20%) |
| [S5] response | shape the response object | Ignore (20%) |
| [S6] envelope+return | wrap + audit + return | Fully (free) |

### [invoice.rs](invoice.rs) -- Create Invoice (rust, same canon)
Same six sections, rendered in Rust from the SAME canon. Only the 20% slots differ.

## Audit total (this file)
- Framework regenerated free (Fully): **2 / 6** sections
- Real work (AI + Ignore): **4 / 6** sections
- Old way: a human writes all 6 by hand, every entity. Here, 2 come free from canon -- and across N
  entities those 2 are emitted N times for ~0 tokens.

Change history: [said.log.md](said.log.md)

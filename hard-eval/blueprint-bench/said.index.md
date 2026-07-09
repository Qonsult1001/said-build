---
type: said.canon.index
spec: okf/1.0
project: said-demo
---

# Said Canon Index (the wiki)

Library drill-down: project -> file -> section. Click a file to open the code. Each section tag
says: **can I edit it?** GENERATED = .said rewrites it (don't edit). YOURS = .said keeps it (edit freely).
How to change a GENERATED section, eject, review: see "Controls" at the bottom -- stated ONCE.

## Files (9)

### [examples/01-rest-crud/csharp/InvoiceController.cs](examples/01-rest-crud/csharp/InvoiceController.cs) -- 6 sections (3 YOURS / 3 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | accept-and-audit | GENERATED -- no |
| S2 | idempotency | GENERATED -- no |
| S3 | guards | YOURS -- yes |
| S4 | save | YOURS -- yes |
| S5 | response | YOURS -- yes |
| S6 | wrap-and-return | GENERATED -- no |

### [examples/01-rest-crud/csharp/OrderController.cs](examples/01-rest-crud/csharp/OrderController.cs) -- 6 sections (3 YOURS / 3 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | accept-and-audit | GENERATED -- no |
| S2 | idempotency | GENERATED -- no |
| S3 | guards | YOURS -- yes |
| S4 | save | YOURS -- yes |
| S5 | response | YOURS -- yes |
| S6 | wrap-and-return | GENERATED -- no |

### [examples/01-rest-crud/rust/invoice.rs](examples/01-rest-crud/rust/invoice.rs) -- 6 sections (3 YOURS / 3 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | accept-and-audit | GENERATED -- no |
| S2 | idempotency | GENERATED -- no |
| S3 | guards | YOURS -- yes |
| S4 | save | YOURS -- yes |
| S5 | response | YOURS -- yes |
| S6 | wrap+return | GENERATED -- no |

### [examples/02-http-client/csharp/GeoClient.cs](examples/02-http-client/csharp/GeoClient.cs) -- 6 sections (2 YOURS / 4 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | build-client | GENERATED -- no |
| S2 | build-request | YOURS -- yes |
| S3 | send | GENERATED -- no |
| S4 | guard-status | GENERATED -- no |
| S5 | parse-body | GENERATED -- no |
| S6 | map-result | YOURS -- yes |

### [examples/02-http-client/csharp/WeatherClient.cs](examples/02-http-client/csharp/WeatherClient.cs) -- 6 sections (2 YOURS / 4 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | build-client | GENERATED -- no |
| S2 | build-request | YOURS -- yes |
| S3 | send | GENERATED -- no |
| S4 | guard-status | GENERATED -- no |
| S5 | parse-body | GENERATED -- no |
| S6 | map-result | YOURS -- yes |

### [examples/02-http-client/rust/weather.rs](examples/02-http-client/rust/weather.rs) -- 6 sections (2 YOURS / 4 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | build-client | GENERATED -- no |
| S2 | build-request | YOURS -- yes |
| S3 | send | GENERATED -- no |
| S4 | guard-status | GENERATED -- no |
| S5 | parse-body | GENERATED -- no |
| S6 | map-result | YOURS -- yes |

### [examples/03-cli-command/csharp/AddUserCommand.cs](examples/03-cli-command/csharp/AddUserCommand.cs) -- 6 sections (2 YOURS / 4 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | parse-args | GENERATED -- no |
| S2 | validate | YOURS -- yes |
| S3 | load-context | GENERATED -- no |
| S4 | execute | YOURS -- yes |
| S5 | print-result | GENERATED -- no |
| S6 | return-exit-code | GENERATED -- no |

### [examples/03-cli-command/csharp/RemoveUserCommand.cs](examples/03-cli-command/csharp/RemoveUserCommand.cs) -- 6 sections (2 YOURS / 4 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | parse-args | GENERATED -- no |
| S2 | validate | YOURS -- yes |
| S3 | load-context | GENERATED -- no |
| S4 | execute | YOURS -- yes |
| S5 | print-result | GENERATED -- no |
| S6 | return-exit-code | GENERATED -- no |

### [examples/03-cli-command/rust/add_user.rs](examples/03-cli-command/rust/add_user.rs) -- 6 sections (2 YOURS / 4 GENERATED)
| S | Section | Edit? |
|---|---|---|
| S1 | parse-args | GENERATED -- no |
| S2 | validate | YOURS -- yes |
| S3 | load-context | GENERATED -- no |
| S4 | execute | YOURS -- yes |
| S5 | print-result | GENERATED -- no |
| S6 | return-exit-code | GENERATED -- no |

## Totals
Across 9 files: **you own 21** sections, **.said generates 33** (re-emitted free on every entity).

## If you edit a GENERATED section (stated once)
GENERATED sections come from a saved template (the canon) so the same pattern is reused, not
recreated. If you edit one because you prefer a different way, .said treats it as feedback and asks:
- **Make this my new standard** -- update the canon; every future one is built your way (you stop
  re-fixing the same thing).
- **Just this once** -- keep your edit local; the canon is unchanged.
(Keeping/reverting the edit itself is your editor's diff + git -- .said doesn't touch that.)

History: [said.log.md](said.log.md)

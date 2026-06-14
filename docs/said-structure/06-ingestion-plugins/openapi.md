# openapi — OpenAPI 3.x directive adapter

**Part of** said-forge — not of the `docs` ingestion plugin. This page documents how OpenAPI documents are **parsed into user stories** for spec-driven workspace generation, NOT how they'd be ingested as opaque document frames.

Entry point: [`crates/said-forge/src/source/openapi.rs`](../../../crates/said-forge/src/source/openapi.rs) — `OpenApiSource` implements the `DirectiveSource` trait.

## Input shapes

| Source | Example |
|---|---|
| Local file | `crates/said-forge/fixtures/petstore.yaml` |
| Local JSON | `schema.json` |
| HTTPS URL | `https://petstore3.swagger.io/api/v3/openapi.json` |
| HTTP URL | `http://internal.example.com/api/openapi.yaml` |

Detection:
- `.yaml` / `.yml` / `.json` file extension, OR
- URL starts with `http://` / `https://` AND ends with a supported extension OR contains `openapi` / `swagger` in path

Non-http schemes (e.g. `ftp://`) are rejected up front. Content peek of the first 512 bytes confirms `openapi:` / `"openapi"` / `swagger:` when the file is readable.

## Dependencies

- `serde_yaml` (YAML → JSON round-trip; `oas3` was considered and rejected — its API churns between minor versions)
- `serde_json` (walker over the document tree)
- `reqwest` (HTTPS URL fetch)

## One operation → one story

For each `(path, method)` pair in `paths.*`, one `Story` is written:

| Story field | Source |
|---|---|
| `slug` | `<method-lowercase>-<sanitized-path>` (e.g. `post-pet`, `delete-store-order-orderid`) |
| `title` | `operation.summary` or fallback `"<METHOD> <path>"` |
| `kind` | `StoryKind::ApiEndpoint` |
| `raw_text` | `"<METHOD> <path> — <title>"` |
| `source_anchor` | `paths.<path>.<method>` (JSON pointer) |
| `fields.method` | Upper-case HTTP verb |
| `fields.path` | Raw path with `{paramName}` templates |
| `fields.operation_id` | When present |
| `fields.summary`, `fields.description` | OpenAPI fields, passthrough |
| `fields.tags` | Array of tag strings |
| `fields.parameters` | Array of `{name, in, required}` triples |
| `fields.responses` | Map of `{status_code → {code}}` |

Supported HTTP methods: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS`, `TRACE`.

## Frame tags on the directive

| Tag | Purpose |
|---|---|
| `forge:directive:<hash>` | Raw YAML/JSON bytes + metadata (source path, adapter name, timestamp, operator) |
| `forge:story:<hash>:<slug>` | One per extracted story |

Pillar: **External** (both directive and stories are pointers to / derived from an authoritative external source).

## Limitations

- Schema `$ref` is not resolved — schema names referenced by `requestBody` / `responses` are preserved verbatim in `fields.responses.<code>.code` but their inlined bodies are not walked. The generator's grounding retrieval picks up schema names via the brain's Code pillar when the underlying types are ingested separately (`said init` on source code + SQL).
- OpenAPI 2 (Swagger) is **not** supported. Convert with `swagger2openapi` first.
- Extensions (`x-*`) are silently dropped (not preserved in `fields`).
- The adapter does not validate the OpenAPI document beyond "has a `paths` object". Invalid schemas produce partial stories.

## How to test

```bash
# With a local fixture:
said forge load crates/said-forge/fixtures/petstore.yaml
# → "Loaded directive c7658 via openapi: 20 stories"

# With a live URL (requires internet):
said forge load https://petstore3.swagger.io/api/v3/openapi.json
```

## Source

[`crates/said-forge/src/source/openapi.rs`](../../../crates/said-forge/src/source/openapi.rs) — `OpenApiSource::detect / load / extract_stories`, 5 unit tests covering detection, URL rejection, fixture parsing (20 operations), slug disambiguation on `/pet/{petId}`, and tag preservation.

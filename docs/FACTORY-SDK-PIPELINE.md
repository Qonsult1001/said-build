# The Factory-owned SDK pipeline — globally unique to Orchestration Factory

**Goal:** Orchestration Factory **is** the source of native SDKs for any orchestration layer it fronts.
A user picks a platform in *our* UI and downloads a real, Factory-branded SDK from *us*. They never see
`openapi-generator.tech` — OpenAPI Generator is an **invisible internal engine**, self-hosted, behind
our own endpoint. The Factory owns the pipeline end to end.

This is the opposite of "go use openapi-generator.tech with your spec." That makes the third-party tool
the product. Here, the **Factory** is the product; the generator is a replaceable engine inside it.

## The pipeline (what the Factory owns)

```
Orchestration layer (the API)  ──①──▶  OpenAPI spec  ──②──▶  generator engine  ──③──▶  Factory SDK
   OrchestrationFactory.API          /openapi/v1.json      (self-hosted,            (branded, real,
   (9 endpoints, net10)              emitted by US          openapi-generator         downloadable)
                                     AddOpenApi())          in Docker, OURS)
        │                                                        │
        └── the user only ever sees OUR UI + OUR download ───────┘
            (the engine + spec are internal plumbing)
```

Three owned stages, each replaceable without the user noticing:

### ① Spec emission — the Factory describes its own surface
The `OrchestrationFactory.API` serves its own **OpenAPI spec** at `/openapi/v1.json` via .NET 10's
built-in `AddOpenApi()` (BCL-only — no third-party NuGet, keeps the offline-build guarantee). This spec
**is** the contract any native SDK is generated from. It is *ours*, emitted from *our* endpoints —
the spec is documentation the Factory produces about itself.

### ② Generation — an engine we host, not a service we depend on
A **self-hosted** OpenAPI Generator (Docker `openapitools/openapi-generator-online`, already available
on this machine) turns the spec into a native SDK for any target: `swift6` (iOS), `kotlin` (Android),
`python`, `csharp`, `typescript-fetch`, … The public beta service (`api.openapi-generator.tech`,
explicitly "no service-level guarantee") is **never** a runtime dependency — we run the engine
ourselves, so the Factory's SDK generation is always up and always ours.

### ③ Branding — the output is unmistakably Orchestration Factory
Generation options stamp the Factory's identity onto every SDK: package id, namespace, and metadata
(`OrchestrationFactory.Sdk.iOS`, `io.orchestrationfactory:sdk`, `orchestration-factory` on PyPI,
`@orchestrationfactory/sdk` on npm). A developer who pulls the SDK pulls **the Orchestration Factory
SDK**, not "a generic client for some API."

## Why this is globally unique to us

- **The user's entry point is our UI**, not a third-party site. "Generate iOS SDK" is a Factory button.
- **The spec is generated from the orchestration layer the user built** — so the SDK reflects *their*
  flow/contract, not a static template. No other tool sits between the user's work and their SDK.
- **The engine is invisible and swappable.** openapi-generator today; a better engine tomorrow; the
  user's experience ("pick platform → download Factory SDK") never changes. The moat is the *pipeline
  and the brand*, not the engine.
- **It composes with the orchestration layer.** The same single contract that fronts AB/AfricanBank is
  the spec that generates the iOS/Android/Python SDKs — one source, every backend, every platform.

## What it is NOT

- **Not a runtime call to a beta public service.** Generate-and-commit (or self-hosted on demand), never
  "depend on openapi-generator.tech being up."
- **Not a replacement for hand-crafted ergonomics where they matter.** The C# `SaidApi` front door stays
  the gold standard for .NET; generated SDKs give *real* native clients for platforms we'd otherwise have
  only snippets for. (A future enhancement: graft a thin Factory front-door onto each generated SDK.)
- **Not locked to one engine.** openapi-generator is the first engine; the pipeline owns the seam so it
  can change.

## Build order

1. **Emit the spec** — `AddOpenApi()` in `OrchestrationFactory.API`; serve `/openapi/v1.json`. (small,
   BCL-only) — the prerequisite for everything.
2. **Stand up the engine** — run the self-hosted generator (Docker), prove one real SDK (e.g. `python`
   or `swift6`) generates from our spec.
3. **Own the endpoint** — a Factory API route (e.g. `POST /sdk/generate?platform=swift6`) that emits the
   spec → calls the internal engine → returns the branded SDK zip. The user hits *our* route.
4. **Wire the UI** — the Export panel's platform tabs gain "Download real {platform} SDK" hitting our
   route. (folds into the parked Export redesign.)
5. **Codify in the skill** — `sdk-factory` recipe part 1: the spec-driven, Factory-owned multi-platform
   path (self-hosted engine, branded output, generate-and-commit not runtime-dependency).

Stages 1–2 prove the whole thing works before any UI or endpoint investment.

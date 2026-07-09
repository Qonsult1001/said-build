# Consumer model — any-language SDK → central per-tenant enforcement

**Status:** Implemented (with two named deployment gaps)
**Date:** 2026-06-24

Answers the core question: *"No matter which SDK a customer picks (C#, TypeScript, …), they change
rules in the portal, and those changes take effect for THEIR environment, enforced by the global
orchestrator."* — Is that the case?

## The model (and it is correct)

```
   Customer's C# app   ──┐
   (C# SDK)              │
                         ├──►  Global Orchestrator  ◄── rules changed in the PORTAL,
   Customer's TS app   ──┘        /enforce              stored per tenantId
   (TypeScript SDK)              (per-tenant)
                                      │
                          rules enforced HERE, server-side —
                          IDENTICAL for every SDK language
```

**The SDK is just the doorway. The rules live and are enforced centrally.** The customer's SDK
language never affects the rule logic, because rules are not enforced in the SDK — they're enforced by
the orchestrator's `/enforce` endpoint, which every SDK simply calls.

## What is TRUE today (verified)

| Requirement | Status | Evidence |
|---|---|---|
| Pick any SDK language (C#, TS, +80 more) | ✅ | factory generates each via `/sdk/generate?platform=…` |
| Point the SDK at the global orchestrator (not localhost) | ✅ | `localhost:5015` is only a DEFAULT; set `BasePath` (C#) / `basePath` (TS) at construction |
| Rules changed in the portal stored per-tenant | ✅ | control plane keyed by `(tenantId, endpoint)` |
| Rules enforced centrally, identical for every language | ✅ | server-side `/enforce`; C# and TS both verified to pass/reject identically |
| The SDK reaches the orchestrator from an external env | ✅ | both SDKs verified against the live API; TS portable in raw Node ESM + bundlers |

**So: yes — a customer picks C# or TS, points it at the orchestrator URL, and the rules they changed in
the portal are enforced for their tenant. The enforcement is genuinely SDK-language-agnostic.**

## The two GAPS (deployment, not architecture)

These are what stand between "works on localhost" and "a customer plugs it into their production env":

1. **No deployed global orchestrator.** Today the API runs on `localhost:5015`. To be a "global
   orchestrator" a customer can point at, it must be deployed to a public, stable URL. The SDKs already
   accept that URL at construction — there's just nothing deployed to point at yet.

2. **Tenant identity is not authenticated.** The SDK sends `X-Tenant-Id` as a plain header — any caller
   can claim any tenant. For real per-tenant isolation ("only MY rules apply to MY traffic"), the
   orchestrator must authenticate the tenant (an API key / token issued per tenant), not trust a header.
   Until then the model is correct but not secure for multi-tenant production.

## How a customer uses it (once deployed)

**C# consumer (a .NET app/service):**
```csharp
var of = new OrchestrationFactoryApi("https://orchestrator.yourco.com", tenantId: "acme", token: "<key>");
await of.Enforce("POST", "orders", payload);   // their portal rules apply
```

**TypeScript consumer (web/Node):**
```ts
const sdk = new OrchestrationFactoryAPIApi(new Configuration({ basePath: "https://orchestrator.yourco.com" }));
await sdk.v2IotLogicFlowPublishPost({ publishFlowBody: { flowId } });
```

Same orchestrator, same rules, different language. The portal is where rules change; the SDK is how the
customer's environment talks to the orchestrator that enforces them.

## The browser-UI clarification

The Orchestration **portal UI is a browser app (TypeScript)** — it cannot load a C# DLL (browsers run
JS only). So the UI consumes the **TypeScript SDK**. A customer's **C# environment** consumes the **C#
SDK**. They are parallel clients of the same orchestrator, not connected to each other. "Use the UI to
connect to the C# SDK" is a category error — the UI uses TS; a .NET app uses C#; both hit the same
central orchestrator.

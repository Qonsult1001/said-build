# The Single Orchestration Layer — design doc

**Purpose:** agree on the architecture *before* building. You want what FutureBank has — **one central
point every app and system connects through** — for `.said` now, and for banking later. This doc
explains exactly what that pattern is (from the real FutureBank code), maps it onto `.said`, and shows
how the same shape is reused for banking. No code yet; this is the agreement.

---

## 1. What FutureBank's orchestration layer actually is

It is **one interface that everything goes through**, with **swappable backends behind it**. Concretely:

```
Apps (Accounts API · Approvals API · Beneficiaries API …)
        │  every domain service depends on ONE interface, never a backend
        ▼
╔══════════════════════════════════════════════╗
║   THE SINGLE POINT:  IBankingAdaptor          ║   ← the orchestration layer
║   (accounts · beneficiaries · users ·         ║
║    approvals · notifications)                 ║
╚══════════════════════════════════════════════╝
        │ chosen ONCE at the composition root (DI registration)
   ┌────┴─────────────────────┐
   ▼                          ▼
Gateway SDK              DirectTransact SDK      ← interchangeable implementations
(15s, gateway API)       (30s, direct SQL)         (same contract, different backend)
   │                          │
   ▼                          ▼
downstream system A      downstream system B
```

The five load-bearing facts (all verified in the FutureBank source):

1. **One composite contract** — `IBankingAdaptor` (in `gk-fb-core-adaptor-abstractions`) combines every
   operation (accounts/beneficiaries/users/approvals/notifications). This single interface **is** the
   orchestration point. Nothing downstream is called except through it.
2. **Interchangeable backends** — `AdaptorGatewayIntegrationService` and `DirectTransactIntegrationService`
   *both* implement `IBankingAdaptor` identically. They differ only in endpoint + timeout + internal
   mapping.
3. **Consumers depend only on the interface** — `AccountService`, `BeneficiaryService`, `ApprovalsService`
   each take `IBankingAdaptor` in their constructor and never reference a backend. Their logic is
   backend-agnostic.
4. **One-line selection at the root** — an app picks a backend with a single DI call:
   `services.AddAdaptorGatewaySdk(baseUrl)` **or** `services.AddDirectTransactSdk(baseUrl)`. Swapping
   backends is a **one-line change**; zero domain code moves.
5. **Cross-cutting threads through the one point** — correlation (`AddCorrelationHeader` delegating
   handler), retry policy, and the session resolver (`ISessionResolver`: token + correlationId +
   userProfileId) are wired into the SDK registration, so **every call** through the layer
   automatically carries them. Add a concern once; it applies everywhere.

**Why it's "one point," not scattered integrations:** every app talks to the *same* interface;
swapping/adding a backend, or adding a cross-cutting concern, happens in *one* place. The alternative —
each app calling each downstream system directly — is the point-to-point mess this avoids.

---

## 2. How this maps onto `.said` (today)

`.said` already has the *raw materials* but **not yet assembled into a single layer**. Here is the
one-to-one mapping:

| FutureBank piece | `.said` equivalent today | Gap to close |
|------------------|--------------------------|--------------|
| `IBankingAdaptor` (the one contract) | `ISaidGateway` (search/ask/remember/get/delete/status/…) — **already exists** in SaidFlow.Application | It's already the single contract — promote it to a shared **abstractions** package |
| Gateway vs DirectTransact (backends) | `SdkSaidGateway` (live, via the C# SDK) + `StubSaidGateway` (offline) — **already two** | Already interchangeable; formalize the registration so swapping is one line |
| One-line registration | `SaidClientFactory.Create` / `AddSaidSdk` — **exists** | Wrap in an `AddSaidOrchestration(...)` that picks the backend |
| Session resolver | `ISaidSessionResolver` (brain path + correlation id) — **exists** | Already the identity seam |
| Correlation through every call | `SaidTransport` threads `_meta._correlationId` — **exists** | Already uniform |
| Consumers depend only on the contract | SaidFlow's executor depends on `ISaidGateway` — **already true** | Exactly the FutureBank shape |

**The verdict:** `.said` is *one small refactor* away from FutureBank's shape. `ISaidGateway` is already
your `IBankingAdaptor`. What's missing is **formalizing it as a standalone orchestration package** so
*other* apps (not just SaidFlow) consume the one layer, and so backends register with one line.

Proposed `.said` orchestration layer:

```
Apps (SaidFlow · a web portal · a future service · an MCP agent)
        │  all depend on ISaidGateway (the ONE contract)
        ▼
╔══════════════════════════════════════════════╗
║   Said.Orchestration  (ISaidGateway)          ║   ← promote the existing interface here
║   search · ask · remember · get · delete ·    ║
║   status · salience · sym · deliver           ║
╚══════════════════════════════════════════════╝
        │ AddSaidOrchestration(...) picks ONE backend
   ┌────┴───────────────┬──────────────────┐
   ▼                    ▼                  ▼
MCP backend         (future) HTTP      Stub/Fake
(said-mcp stdio)     remote .said       (offline/CI)
```

The change is **additive**: pull `ISaidGateway` into a `Said.Orchestration` (abstractions) project,
have the gateways implement it from there, and add a single `AddSaidOrchestration(backend, options)`
registration. SaidFlow keeps working unchanged (it already uses `ISaidGateway`); any *new* app now
plugs into the same one point.

---

## 3. How banking reuses the exact same shape (later)

When you build banking, you **don't invent a new architecture** — you instantiate the same pattern with
banking's contract:

```
Banking apps (accounts · payments · approvals …)
        ▼
╔══════════════════════════════════════════════╗
║   Bank.Orchestration  (IBankAdaptor)          ║   ← same shape, banking ops
╚══════════════════════════════════════════════╝
        │ AddBankOrchestration(...)
   ┌────┴───────────────┬──────────────────┐
   ▼                    ▼                  ▼
core-banking API     legacy SQL        sandbox/mock
```

It's **literally FutureBank again** — which is why FutureBank is the perfect reference. The reusable
*meta-pattern* (one contract · interchangeable backends · one-line selection · cross-cutting threaded ·
consumers depend only on the interface) is what the `sdk-factory` skill should teach, so both `.said`
and banking are produced from the same recipe.

`.said` and banking stay **completely separate codebases** (different contracts, different backends).
What they share is the **pattern**, codified once in the skill.

---

## 4. What the `sdk-factory` skill must gain

The skill currently teaches the SDK (client) and its parts. It should also teach the **orchestration
layer** as a first-class architecture: when many apps/systems must connect through one point, generate

1. an **abstractions package** holding the single composite contract (the `IBankingAdaptor` / `ISaidGateway`
   role),
2. **≥2 interchangeable backend implementations** of it (this is the part-8 adaptor seam, elevated to
   the system level),
3. a **one-line registration** that selects the backend at the composition root,
4. **cross-cutting (correlation/retry/session) wired into that registration** so it threads every call,
5. **consumers that depend only on the contract** — never a backend.

with the guard: *it's a real orchestration layer only when ≥2 backends exist and ≥2 consumers go through
it* — otherwise it's a single client dressed up (the deletion test again).

---

## 5. The decision for you

Three things, in order:

1. **Agree this is the architecture** (this doc).
2. **Upgrade the skill** to teach the orchestration-layer pattern (so it's reproducible for `.said` +
   banking).
3. **Apply it to `.said`** — the small refactor that promotes `ISaidGateway` into `Said.Orchestration`
   and adds `AddSaidOrchestration(...)`, so apps beyond SaidFlow consume the one layer.

Steps 2 and 3 are independent — we can do the skill first (pattern captured), then apply to `.said`
when you're ready. Banking comes later, reusing the skill.

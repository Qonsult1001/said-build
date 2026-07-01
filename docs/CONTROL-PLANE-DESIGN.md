# Control Plane Design — Tenant Self-Service API Policy

**Status:** Proposed (design review)
**Date:** 2026-06-23
**Scope:** A multi-tenant control plane where an enterprise client ("Tenant X") logs into the
OrchestrationFactory portal and changes how *their own* API endpoint behaves — make a field
required, add a validation rule, transform a payload — enforced at the **vendor-owned .NET API
runtime**, never by editing the tenant's code. Staged **draft → test-live → publish → rollback**,
per-`tenantId` isolation, versioning, audit.

---

## 1. The vision (in one sentence)

> Tenant X declares how their endpoint should behave as **DATA scoped to their `tenantId`**, tests
> it live in a sandbox, and clicks **Publish** — and our global runtime (the layer every tenant's
> traffic flows through) enforces it. We never touch their code; they never touch ours.

Concrete example (the user's words): *"User X connects to our portal, makes some fields required on
his API endpoint, and tests it live."*

---

## 2. What the research proved

A deep, adversarially-verified research pass (25/25 claims confirmed 3-0, 0 refuted; sources are
overwhelmingly primary vendor docs) found that **every world-class platform converges on one
pattern**:

> **Rules are DATA, scoped per-tenant, interpreted by the vendor's runtime — never customer code.**

| Concern | How the best-in-class do it | Citation |
|---|---|---|
| Per-tenant scoping | Kong **Consumer Groups** — a new policy instance per tenant; precedence Global < Service < Route < Consumer | [Kong Consumer Groups](https://developer.konghq.com/gateway/entities/consumer-group/) |
| "Make a field required" | Kong **Request Validator**: a JSON Schema stored as config data; rejects **400 before** upstream | [Kong Request Validator](https://docs.konghq.com/hub/kong-inc/request-validator/) |
| Contract enforced in middleware | Kong **OAS Validation**: validates traffic against an uploaded OpenAPI spec (data, not code) | [Kong OAS Validation](https://docs.konghq.com/hub/kong-inc/oas-validation/) |
| Safe rule engine (no arbitrary code) | **CEL** — non-Turing-complete, no I/O, no loops, terminates, sees only host data. Used by K8s, Envoy, KrakenD, protobuf | [cel.dev](https://cel.dev/), [K8s CEL](https://kubernetes.io/docs/reference/using-api/cel/) |
| Validate at author time | Tenant config checked against a schema **on save**; invalid → 400 immediately | [Kong plugin config](https://docs.konghq.com/gateway/latest/plugin-development/configuration/) |
| Logic/config split for staging | K8s **Policy + Parameter + Binding**: same rule, different params per env (test=3, prod=100), bound per-namespace(tenant) | [K8s ValidatingAdmissionPolicy](https://kubernetes.io/docs/reference/access-authn-authz/validating-admission-policy/) |
| Validation as data, second gateway | **KrakenD**: per-endpoint CEL expressions in JSON config; `has(JWT.user_id)` = required-field check | [KrakenD CEL](https://www.krakend.io/docs/endpoints/common-expression-language-cel/) |
| Versioning / publish / rollback | **decK** diff-then-sync: declared state vs running state, applied atomically | [decK](https://github.com/Kong/deck) |
| Payload transform (reshape, not gate) | **JSONata** (Confluent Schema Registry migrationRules: rename `ssn → socialSecurityNumber`) | [Confluent data contracts](https://docs.confluent.io/platform/current/schema-registry/fundamentals/data-contracts.html) |
| Audit | LaunchDarkly: every change versioned, attributed, revertible | [LaunchDarkly audit log](https://docs.launchdarkly.com/home/flags/audit-log-history) |

### Two honest gaps the research flagged
1. **CEL / boolean gates only validate — they do NOT transform payloads.** Reshaping requires a
   separate mechanism (JSONata, or a mapping engine). Don't make one tool do both.
2. **No vendor ships a turnkey draft→test-live→publish workflow.** They compose it from versioned
   config + diff-then-sync + the param/binding split. **That lifecycle is ours to design — it is the
   differentiator.**

---

## 3. What we already have (the 80% nobody realized had a name)

The existing `OrchestrationFactory` is **already a rules-as-data interpreter**. Verified in code:

| World-class primitive | What we already built | File |
|---|---|---|
| Rules-as-data interpreter | `FlowExecutor` walks a graph, interprets node `data` bags | [FlowExecutor.cs](../apps/OrchestrationFactory/OrchestrationFactory.Infrastructure/GraphExecution/FlowExecutor.cs) |
| Safe expression DSL (≈ CEL) | `ExpressionEvaluator` — JEXL/Navixy subset: no I/O, no loops, errors→null, non-Turing-complete | [ExpressionEvaluator.cs](../apps/OrchestrationFactory/OrchestrationFactory.Infrastructure/GraphExecution/ExpressionEvaluator.cs) |
| Payload transform (≈ JSONata) | `MappingEngine` + `MappingSpec` — field rename/coerce/default | FlowExecutor `ApplyMapping` |
| Boolean validation gate | `Logic` node → `EvaluateCondition` | FlowExecutor `ProcessAsync` |
| Per-tenant scoping | `TenantId` on the `Flow` aggregate | [Flow.cs](../apps/OrchestrationFactory/OrchestrationFactory.Domain/Flows/Flow.cs) |
| Visual builder | React Flow canvas, click-to-add, AI-build | `ui/src/App.tsx` |

**Implication:** we do NOT need to import Kong, CEL, or JSONata. We have the equivalents. The build is
a **lifecycle wrapper** around the engine we already have, plus one new validation rule type.

### What is genuinely missing
| Missing | Why it matters |
|---|---|
| **Versioned** `Policy` records (draft/published/archived) | rollback + audit + staging all derive from versioning |
| **draft → test-live → publish → rollback** state machine | the user's explicit requirement; the differentiator |
| **`require` (JSON-Schema) rule kind** | the user's literal "make a field required" example |
| **`PolicyEnforcer`** seam | policies must *auto-gate* a tenant's traffic, distinct from "run this flow on demand" |
| **Audit** (author, ts, before/after diff per version) | enterprise requirement |

---

## 4. Decisions (locked with the user)

| Decision | Choice |
|---|---|
| v1 rule scope | **Validation + Transform together** (require + validate + transform) |
| Enforcement point | **Inside the .NET API runtime** (OrchestrationFactory.API middleware) |
| Policy target | **Per endpoint** — key = `(tenantId, endpoint, version)` |
| Apply model | **Staged: draft → test → publish** |
| Process | **Plan-first** (this doc), then build |

---

## 5. Architecture

```
┌─ OrchestrationFactory UI ─────────────────┐   the portal Tenant X logs into
│  X builds a Policy for one endpoint        │   (reuses the existing canvas + builder)
│  draft → [Test live] → Publish / Rollback  │
└───────────────┬────────────────────────────┘
                │  rules as DATA (JSON), scoped by (tenantId, endpoint, version)
                ▼
┌─ Control-plane store (versioned) ─────────┐
│  Policy {                                  │   every version row = audit + rollback target
│    tenantId, endpoint, version,            │
│    state: draft | published | archived,    │
│    author, timestamp,                      │
│    rules: [                                │
│      { kind:"require",  fields:["email"] },             ← JSON-Schema-style (reject 400)
│      { kind:"validate", expr:"amount > 0" },            ← ExpressionEvaluator (≈ CEL gate)
│      { kind:"transform",mappings:[{from,to,coerce}] }   ← MappingEngine (≈ JSONata)
│    ] }                                     │
└───────────────┬────────────────────────────┘
                │  PUBLISHED rules hot-loaded by (tenantId, endpoint)
                ▼
┌─ PolicyEnforcer (in .NET API runtime) ────┐   the layer WE own — enforcement point
│  on each request for tenant X, endpoint E: │
│   1. load published policy (tenantId, E)   │
│   2. require  → reject 400 if field absent │
│   3. validate → reject 400 if expr false   │
│   4. transform→ reshape the payload         │
│   5. pass to X's endpoint                  │
│                                            │
│  [Test mode] runs the DRAFT policy against │
│   a sample payload, returns before/after + │
│   pass/fail trace. Production untouched.   │
└──────────────────────────────────────────────┘
```

### Lifecycle state machine (the differentiator)
```
   draft ──submit──▶ pending-approval ──approve──▶ published ──supersede──▶ archived
     ▲                     │                           │
     │                  reject                         │
     └─────────────────────┘        ◀──── rollback ────┘   (re-point published at a prior version)
```
- **Draft** — a new version, `state=draft`, NOT loaded by the enforcer. Validated on save (Kong
  pattern: malformed rule → 400 immediately).
- **Test-live** — enforcer evaluates the *draft* rules against a sample/mirrored request; returns
  before/after + trace. Production traffic still uses `published`. (K8s test-param vs prod-param.)
- **Submit / pending-approval** — the maker submits the draft; it cannot self-publish. Awaits a
  second person. **(Maker-checker / four-eyes — mandatory for banking; see §11.)**
- **Approve (publish)** — a *different* user approves; only then does the version flip to `published`,
  the prior published version becomes `archived` and is retained as the rollback target.
  (decK diff-then-sync.) A reject sends it back to `draft` with the reviewer's note.
- **Rollback** — re-point `published` at the prior archived version. One operation (still recorded +
  attributed; in regulated mode a rollback may itself require approval).
- **Audit** — the **append-only, tamper-evident** version rows ARE the audit log (who/when/before-after).
  See §11 for the banking-grade audit requirement (signed, immutable).

> For non-regulated tenants the maker-checker step can be configured off, collapsing to a direct
> `draft → published`. It is **on by default for any tenant flagged `regulated`** (banking).

---

## 6. Data model (DDD placement)

| Element | Layer | Project |
|---|---|---|
| `Policy`, `PolicyRule`, `PolicyState`, `PolicyVersion` | Domain | `OrchestrationFactory.Domain/Policies/` |
| `IPolicyRepository` (versioned CRUD), lifecycle handlers | Application | `OrchestrationFactory.Application/Policies/` |
| `PolicyRepository` (store), `PolicyEnforcer`, rule evaluators (reuse `ExpressionEvaluator`/`MappingEngine`) | Infrastructure | `OrchestrationFactory.Infrastructure/Policies/` |
| `PolicyDto`, rule DTOs | Contracts | `OrchestrationFactory.Contracts/` |
| `/policy/*` endpoints + enforcement middleware | API | `OrchestrationFactory.API/` |

### Rule kinds (v1)
```jsonc
// require — the user's "make a field required" example
{ "kind": "require", "fields": ["email", "customer.id"] }      // missing → 400

// validate — boolean gate via existing ExpressionEvaluator (CEL-class: safe, no code)
{ "kind": "validate", "expr": "amount > 0 && has('user_id')", "message": "amount must be positive" }

// transform — payload reshape via existing MappingEngine (JSONata-class)
{ "kind": "transform", "mappings": [ { "from": "customer.name", "to": "who", "coerce": "string" } ] }
```
**Safety:** all three are DATA validated against a fixed schema on save. No arbitrary code path — the
same guarantee CEL gives Kubernetes. `ExpressionEvaluator` is already non-Turing-complete and
error-safe (errors → null/false), matching the research's "no sandbox-escape" requirement.

### Rule enforcement class — `advisory` vs `authoritative` (the banking guardrail)

Every rule carries an `enforcement` field. **This is the single most important banking guardrail**
(see §11): it makes explicit whether our middleware is the *sole* control or a *redundant edge check*
on top of a system-of-record control.

```jsonc
{ "kind": "require", "fields": ["purposeCode"], "enforcement": "authoritative" }  // edge owns this
{ "kind": "validate", "expr": "amount <= dailyLimit",  "enforcement": "advisory" }  // CORE owns this; we only pre-check
```

| Class | Meaning | Allowed rule kinds | Banking use |
|---|---|---|---|
| `authoritative` | Our layer is the **system of record** for this rule. Fully enforced; rejection is final. | `require`, `validate` (contract shape), `transform`, routing | Channel/contract hardening: required fields, payload shape, format checks |
| `advisory` | Our layer enforces it as a **redundant convenience check**; the **core remains authoritative**. We MUST NOT be the only thing standing between the request and the money. | `validate` (only) | Pre-flighting a funds/limit/entitlement check for fast UX feedback — but the core re-checks and wins |

**Hard rule (enforced in the domain model):** a rule that touches money movement, limits-on-funds,
AML/sanctions, fee/interest calc, or authorization **may only be `advisory`**, and the doc/UI must
state the core is authoritative. The `PolicyEnforcer` records advisory outcomes but never treats an
advisory *pass* as authorization. See §11 for why.

> Future hardening (from research): add an **evaluation cost budget** — CEL being non-Turing-complete
> prevents infinite loops but does NOT bound cost. Our evaluator is already bounded (no loops, no
> recursion in user expressions), but per-request step budgets should be enforced like `FlowExecutor`'s
> `MaxSteps`.

---

## 7. API surface

| Endpoint | Purpose |
|---|---|
| `POST /policy/draft` | Create/update a draft version for `(tenantId, endpoint)`. Validates rules; 400 if malformed. |
| `GET  /policy?tenantId=&endpoint=` | List versions (the audit trail) + current published. |
| `POST /policy/test` | Run a **draft** against a sample payload → `{ before, after, trace, passed }`. Prod untouched. |
| `POST /policy/submit` | Maker submits a draft → `pending-approval`. Cannot be the eventual approver. |
| `POST /policy/approve` | Checker (a *different* user) approves → publishes; archives the prior. **Rejected** in regulated mode if approver == submitter. |
| `POST /policy/reject` | Checker sends a pending draft back to `draft` with a note. |
| `POST /policy/rollback` | Re-point published at a prior archived version (attributed; may require approval in regulated mode). |
| `GET  /policy/audit?tenantId=&endpoint=` | The append-only audit trail: who/when/before-after for every state change. |
| *(middleware)* | `PolicyEnforcer` — auto-applies the published policy to live tenant traffic. |

---

## 8. Build order (each step independently testable)

1. **`Policy` domain + versioned repository** — the aggregate, states (incl. `pending-approval`),
   `enforcement` class on each rule, version store.
2. **`require` rule + `PolicyEnforcer`** — delivers the user's literal example end-to-end
   (make `email` required → request without it gets 400).
3. **Lifecycle endpoints** — draft / test / submit / approve / reject / rollback, with the
   **maker-checker** guard (approver ≠ submitter for regulated tenants) and **append-only audit**.
4. **`validate` + `transform` rules** — wire through `ExpressionEvaluator` + `MappingEngine`;
   enforce the advisory/authoritative classification (money-touching rules forced `advisory`).
5. **UI lifecycle controls + before/after test panel** — reuse the canvas; add the
   draft → test → submit → approve bar and the rule-class badge.

Checkpoint with the user after step 2 (the example works end-to-end) before continuing.

---

## 9. Open questions (from research, to resolve during build)

1. **Endpoint identity** — how is a tenant's "endpoint" identified at enforcement time? (route path,
   a registered endpoint id, an OpenAPI operationId?) Determines the enforcer's lookup key.
2. **Test-live fidelity** — does "test live" run against a *sample payload* the tenant supplies, or
   *mirror* a real recent request? (Start with sample payload; mirroring is a v2 enhancement.)
3. **Transform + validate ordering** — require → validate → transform, or allow transform-then-
   validate? (Default: require → validate → transform, matching the diagram.)
4. **Where do versions persist?** — the existing flow store, or a new policy table? (Follow whatever
   `IFlowRepository` uses for consistency.)

---

## 10. Why this is lower-risk than adopting the research's products

We are NOT bolting on Kong/CEL/JSONata. We are adding a **versioned lifecycle + one rule type** to an
interpreter we already own and understand. The research's value was **confirming the pattern is
correct** (rules-as-data, per-tenant, safe DSL, staged publish) and **naming the two gaps** (transform
needs a separate engine — we have MappingEngine; lifecycle is ours to build — section 5). Every
architectural choice here traces to a verified, primary-source finding.

---

## 10b. Implementation status (spike — built + verified)

The thin spike (Alt B: extend the existing `Flow`/`FlowExecutor`, no new aggregate, no banking
ceremony) is **built and verified live against the running API**:

| Capability | Status | Where |
|---|---|---|
| `require` validation rule (make a field required) | ✅ built + verified | `NodeType.Validation`, `FlowExecutor.CheckValidation` |
| Reject with reason → HTTP 400 | ✅ verified | `FlowRunResult.Rejection` → `/flow/run` returns 400 + `rejection_reason` |
| `has()` presence function (reused by future `validate`) | ✅ built | `ExpressionEvaluator` |
| `draft`/`published` lifecycle state | ✅ built | `LifecycleState`, threaded through `Flow`→DTO→mapper |
| Publish (draft → live snapshot) | ✅ built + verified | `IFlowRepository.PublishAsync`, `POST /flow/publish` |
| Rollback (restore prior published, atomic) | ✅ built + verified | `RollbackAsync`, `POST /flow/rollback` |
| Edit draft without affecting live | ✅ verified | `POST /flow/update` (was a gap; added) |
| Get live snapshot | ✅ built | `GetPublishedAsync`, `GET /flow/published` |

**Verified invariants (live test, flow #64):** publish V1 → live `[email]`; edit draft to `[email,phone]`
→ live still `[email]` (editing never touches live); publish V2 → live `[email,phone]`; rollback →
live `[email]` (prior restored atomically); second rollback → 400 (correctly refuses, no prior).

**Deliberately NOT in the spike** (deferred to the north-star design below): the dedicated `Policy`
aggregate, full version history (spike keeps ONE prior snapshot as the rollback target), the
`PolicyEnforcer` that auto-applies published policies to live tenant traffic (blocked on the
endpoint-identity decision, §9.1), `validate`/`transform` rule kinds, maker-checker, and tamper-evident
audit. These are "when a bank asks" / "when the enforcer is wired" features, per the anti-over-
engineering decision.

---

## 11. Banking applicability

> **The governing principle:** *the orchestration layer governs the rules that protect the **channel**
> and the **contract**; the core banking system governs the rules that protect the **money**.* Blur
> that line and you fail a bank audit.

This control plane is valuable to banks **across all three channels equally** — mobile, business/
corporate, and old/legacy core — but only for a specific class of rules. The design is channel-agnostic:
the same `(tenantId, endpoint)` policy model serves a mobile app contract, a corporate-payments API,
and a legacy-core anti-corruption layer without change.

### 11.1 What the layer SHOULD govern (strong fit, fully `authoritative`)

Edge / contract / experience rules — the rules-as-data, per-tenant, staged-publish pattern fits:

| Rule type | Banking example | Why middleware is the right place |
|---|---|---|
| Validation / required fields | "Corporate payment API now requires `purposeCode` + IBAN checksum" | Reject malformed requests at the edge before they touch the core — protects legacy from junk |
| Payload transformation | Legacy core speaks fixed-width / ISO 8583; mobile wants JSON — reshape per channel | Decouples a 30-yr-old core from a modern app without changing either |
| Channel-specific shaping | Mobile gets a slim response; business portal gets the enriched record | One core, many channel contracts — governed as data |
| Routing | Route migrated accounts to the new core, the rest to the old core | Enables a safe **strangler-fig** core migration |
| Enrichment / aggregation | Combine balance (core A) + cards (core B) + FX (core C) into one mobile call | The orchestration layer's home turf |
| Throttling / consumer policy | Per-fintech-partner (open-banking TPP) rate limits | Per-`tenantId` is exactly our model |

**Strongest pitch — legacy systems:** a modern core has its own API management; a 30-year-old core
does not. So this layer becomes the *only* place a bank can safely add a required field or reshape a
payload for that system **without a mainframe change release**. Speed of contract change without
touching the core is the core value.

### 11.2 What the layer must NOT solely govern (hard boundary — `advisory` at most)

These belong in the core / dedicated engines. Putting them *only* in editable middleware is an audit
and risk failure, because **any channel that bypasses our layer would bypass the rule**:

| Rule | Why not (solely) middleware |
|---|---|
| Money movement / debit-credit / balance checks | Must be transactional + ACID in the system of record |
| Limits that gate funds (daily transfer, overdraft) | Edge-only = bypassable via another path = real loss |
| AML / sanctions / fraud scoring | Regulated; needs dedicated engines + heavy audit; not a tenant-editable JSON rule |
| Interest / fee calculation | System-of-record correctness; cannot be "draft-tested" by a portal user |
| Authorization / entitlements (who may approve a $1M payment) | Security-critical; belongs in IAM + core |

**The auditor's question** our design must answer: *"What happens if a request reaches the core NOT
through your orchestration layer?"* If the answer is "the rule isn't enforced," that rule **cannot
live only in our layer** → it is at most `advisory` (§6 rule class), with the core authoritative.

### 11.3 The three banking guardrails (baked into the design)

1. **Rule enforcement classification (`advisory` vs `authoritative`)** — §6. The domain model forbids
   marking a money-/regulation-touching rule `authoritative`; the enforcer never treats an advisory
   pass as authorization.
2. **Immutable, tamper-evident audit** — the versioned policy rows are **append-only and signed**
   (hash-chained per `(tenantId, endpoint)`), capturing who/when/before-after for every state change.
   LaunchDarkly's audit model is the floor; banking needs tamper-evidence on top.
3. **Maker-checker (four-eyes) on publish** — §5 state machine. No single person publishes a rule
   change to a banking channel: `draft → pending-approval → published`, approver ≠ submitter, on by
   default for any `regulated` tenant.

### 11.4 The credible sales position (and what would sink a deal)

- **Sellable:** *"We govern the edge and the contract as data — required fields, payload shape,
  routing, per-partner policy — with full append-only audit and maker-checker. Your core remains the
  system of record for money and regulation; our layer hardens in front of it and lets your
  integration team ship contract changes without a core release."*
- **Would sink a bank procurement review:** claiming *all* rules (including funds/limits/AML) live in
  the middleware. Never claim the layer is the sole control for anything that protects money or
  satisfies a regulation.

### 11.5 Residual open questions for banking

1. Signing/anchoring for the tamper-evident audit — per-row HMAC hash-chain (cheap, in-DB) vs external
   notary/WORM store (stronger, heavier). Start with hash-chain; offer WORM export.
2. How `regulated` is set per tenant (provisioning flag) and whether maker-checker is ever
   per-endpoint rather than per-tenant.
3. Whether advisory pre-checks need a documented "core re-checks and wins" contract test, so a bank
   can prove the edge can't authorize funds movement on its own.

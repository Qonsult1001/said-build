# Enterprise Audit — Compliance Roadmap

Status: **planning** — phases scoped, none started yet.
Owner: TBD
Last updated: 2026-05-03

## Why this exists

`.said` already has the right primitives for a compliance-grade audit story:
append-only event log, hash-chained, BLAKE3 content addressing, tombstones
with byte-exact restore, per-doc lineage with `superseded_by` pointers, and
local-first data residency.

What it doesn't have yet is the **surface** an enterprise auditor expects:
reasoned writes, proof of chain integrity, cryptographic deletion certificates,
external time anchoring, role-based access, and SIEM integration.

This doc defines the gap, in three priority tiers, and proposes a sequenced
build plan to close it without disrupting the personal-user story that
already works.

## What's already enterprise-grade today

- **Append-only event log** with `seq`, `timestamp`, `actor`, `kind`, `target`,
  `detail`, `hash_hex` per entry. See [3.7-audit-log.md](../03-core-subsystems/3.7-audit-log.md).
- **Hash chain**: each entry's `hash_hex` is `blake3(prev_hash || event_n)`.
  Tamper detection is theoretically possible — just not surfaced.
- **Tombstones**: soft-delete with byte-exact restore via `brain.restore(doc_id)`.
- **Per-doc lineage**: every `edit()` / `replace_frame()` creates a new active
  frame and tombstones the old, with `superseded_by` pointers.
- **Legal hold backend**: `legal_hold_add(doc_id, case_id)` exists in
  `crates/said-wasm/src/lib.rs:713-720` and `sca-core::admin_legal_hold_add`.
- **BLAKE3 content addressing** end-to-end: every frame body has a verifiable
  fingerprint independent of metadata.
- **Local-first**: data never leaves the device unless the user explicitly
  exports. This is itself a compliance posture (data residency, no third-party
  data processor).

## Tier 1 — required to claim "audit-ready"

These are the table stakes. Without them, a compliance officer at a regulated
customer (financial, health, government, legal) will not green-light `.said`
for production use.

### 1.1 Reason field on every write op

**Today**: `delete()` / `edit()` / `restore()` / `compact()` log audit events
with `kind` and `target` but no `reason`. The UI shows generic labels like
"manual delete."

**Needed**: optional `reason: Option<String>` parameter on every destructive
op, recorded into the event's `detail` field. UI prompts for it on
user-initiated deletes (with sensible defaults like "user requested").

**Compliance hook**: SOX §404 (internal controls), GDPR Art. 17 ("right to
erasure" requires proof of *why* — request, retention expiry, etc.).

**Effort**: ~half a day. Pure Rust change in `delete`/`edit` signatures,
bubble through CLI/MCP/wasm, prompt in UI.

### 1.2 Stable actor identity

**Today**: `actor: String` is free-form. Often just `"user"` or `"agent"`.

**Needed**:
```rust
struct AuditActor {
    actor_id: String,        // stable identifier (email, UUID)
    display_name: String,
    auth_method: String,     // "password" | "sso:okta" | "api_key:<id>" | "system"
    session_id: Option<String>,
    ip: Option<String>,      // populated by host integration
    user_agent: Option<String>,
}
```

**Compliance hook**: HIPAA §164.312(b) audit controls, ISO 27001 A.9.4.2,
"who accessed what when from where." All major SIEM correlation requires
stable actor IDs.

**Effort**: 1 day Rust + half a day to wire up host integration points.

### 1.3 Chain integrity verification

**Today**: hash chain exists but nothing surfaces "verify it's intact."

**Needed**:
- CLI: `said audit verify` walks every event, checks `hash_n = blake3(prev_hash || event_n)`,
  reports first break by seq.
- WASM/UI: button in Admin panel that runs the same and shows pass/fail
  with the offending seq if any.
- Output format: human + JSON, exit code 0 (intact) or 1 (broken).

**Compliance hook**: every audit framework expects "the audit log itself is
tamper-evident." Without a verify command you can't prove it.

**Effort**: 1 day. Pure verification logic + CLI surface + UI button.

### 1.4 External anchoring (time-stamping)

**Today**: nothing prevents an attacker with file access from rewriting
history and recomputing the hash chain.

**Needed**: periodic Merkle root commit to an external store. Two modes:
- **RFC 3161 timestamping**: send the latest `hash_hex` to a trusted TSA,
  store the signed timestamp response inside the audit log as a
  special "anchor" event.
- **Internal anchor log**: a write-once external file (S3 object lock, blockchain,
  or just an append-only signed log on a separate machine).

**Compliance hook**: GDPR Art. 32 (security of processing, "ability to
restore"), eDiscovery (court-admissible chain of custody requires external
timestamps).

**Effort**: 2-3 days. RFC 3161 integration is a known pattern; UI/CLI for
"anchor now" + scheduled anchoring.

### 1.5 Legal hold UI

**Today**: backend exists. UI doesn't surface it. Compact would happily
purge tombstones under hold.

**Needed**:
- Admin panel: per-doc hold toggle, list of active holds, list of cases.
- `compact --drop-history` must refuse to remove tombstones under hold,
  with a clear error: "doc X is on hold under case Y, opened YYYY-MM-DD by Z."
- Audit events for `hold_add` and `hold_release`.

**Compliance hook**: litigation hold is a legal *obligation* (FRCP 37(e)).
Failing to honor it is sanctionable.

**Effort**: 2 hours. Backend exists; just a UI panel + a sca-core guard.

## Tier 2 — needed for big-customer wins

### 2.1 Retention policy engine

**Need**: rules like "tombstones with tag `pii` must be physically purged
after 90 days" or "tag `legal:case-2024-007` must be retained 7 years."
Engine runs daily, applies policy, logs every action as an audit event with
reason="retention policy XYZ executed."

**Compliance hook**: GDPR Art. 5(1)(e) storage limitation, HIPAA §164.530(j)
retention requirements, sector-specific schedules.

**Effort**: 3-4 days. Rule schema + scheduler + dry-run mode.

### 2.2 Approval workflow on destructive ops

**Need**: `compact --drop-history` and `wipe local` require a 2nd approver
in enterprise mode. Today anyone with file access can purge.

**Effort**: 2-3 days. Pending-action queue + approval audit events.

### 2.3 Read audit (optional mode)

**Need**: `read()` / `ask()` / `grep()` log entries with the doc_ids touched
when read-audit mode is enabled. SOX cares about who read sensitive data,
not just who wrote.

**Caveat**: 10-50x audit log size growth. Must be opt-in, ideally per-tag
(e.g. only audit reads of frames tagged `pii` or `phi`).

**Effort**: 2 days.

### 2.4 SIEM export

**Need**: structured JSON export of audit log + a streaming mode that pushes
events to an HTTP endpoint or syslog/CEF as they happen. Splunk / Datadog /
Elastic ingestion is required by every enterprise security team.

**Effort**: 2-3 days. JSON schema + push mode + delivery guarantees
(at-least-once with retry).

### 2.5 Differential snapshots

**Need**: "what was the brain's state on March 15?" Captured by a timestamped
marker event + replay logic.

**Effort**: 3 days. Cheap because we're already append-only.

## Tier 3 — premium / pitchable differentiators

### 3.1 Discovery export bundles

eDiscovery requests are court orders. CLI command takes a query + date range
and exports matching frames (active + tombstoned) as a sealed bundle with
chain-of-custody metadata, in a format ready to hand to opposing counsel.

### 3.2 Field-level redaction with audit

GDPR Art. 17 sometimes requires redacting a *single field* in a frame, not
deleting the whole thing. `redact(doc_id, span)` op produces a new active
version with the span replaced by `[REDACTED]`, leaves the original
tombstoned with restricted access.

### 3.3 Role-based access on the audit log

Auditors must read it; ordinary users must not write; admins must not edit.
Today `.said` has no access control beyond "anyone with the file is root."
Encryption + role keys for the audit section.

### 3.4 Cryptographic deletion certificate

When a GDPR Art. 17 erasure request is fulfilled, generate a signed
certificate: "content X was deleted on date Y by user Z, hash A no longer
present at chain position B, anchored at TSA timestamp T."

## Suggested sequencing

If we attack this as a separate workstream from personal-user features:

| Phase | Tickets | Effort | Outcome |
|------|---------|--------|---------|
| **A — Honest audit** | 1.1 reason field, 1.3 chain verify, 1.5 legal hold UI | ~2 days | Audit cards + UI tell the truth; chain integrity provable; holds enforced |
| **B — Trustable audit** | 1.2 stable actor identity, 1.4 external anchoring | ~4 days | Forensically sound; meets RFC 3161; suitable for SOX walkthrough |
| **C — Operational audit** | 2.4 SIEM export, 2.2 approval workflow | ~5 days | Slots into existing enterprise security pipelines |
| **D — Compliance program** | 2.1 retention engine, 2.3 read audit, 2.5 snapshots | ~7 days | Customer can build a documented compliance program around `.said` |
| **E — Premium** | All of Tier 3 | ~10 days | Differentiator vs. mem0 / Letta / hosted LLM memory products |

Total: ~28 engineering days for a fully audit-pitchable product. Phase A alone
(2 days) takes us from "developer demo" to "honest demo" — recommended next
step after personal-user UI is stable.

## Out of scope here

- **Provenance/lineage of model outputs** (which prompts produced what answer)
  — that's a separate "trust the answer" story, tracked in dream/episodic
  writer roadmap, not audit.
- **Multi-tenant isolation** — `.said` is single-tenant by design; multi-tenant
  enterprise SKU is a separate product line.
- **HSM-backed key management** — handled by the host's existing key
  infrastructure, not built into `.said`.

## References

- [3.7-audit-log.md](../03-core-subsystems/3.7-audit-log.md) — current audit
  log structure and hash chain.
- [row-42-admin.md](row-42-admin.md) — admin command surface (export, rekey,
  wipe, legal hold backend).
- [row-43-admin-ui.md](row-43-admin-ui.md) — current admin UI inventory.
- [11-known-limitations.md](../11-known-limitations.md) — known compliance gaps.
- `crates/sca-core/src/audit.rs` — implementation of the hash chain.
- `crates/sca-core/src/admin.rs` — legal hold + tombstone admin ops.

## Open questions for product/legal

1. Which compliance regimes are highest-priority customer asks — SOX (financial),
   HIPAA (health), GDPR (EU), FedRAMP (US gov), ISO 27001 (general)?
2. What's the first regulated customer we want to pitch? That defines which
   tier-2 items become tier-1.
3. SaaS-hosted control plane for SIEM/audit aggregation — in scope for the
   personal-first product, or strictly enterprise SKU?
4. RFC 3161 TSA — do we run our own, or integrate with DigiCert / GlobalSign /
   public TSAs? Cost vs. trust trade-off.

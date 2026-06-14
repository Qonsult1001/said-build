# License — Planning Document

> Status: **Draft for review.**
> Author: design conversation, 2026-05-04.
> Decision-required sections are marked **[DECIDE]**.

---

## TL;DR

The `.said` file is the user's identity AND their licence. Drop the file on the
public WASM site (`said.app`) and what you can do is determined by a
**server-signed tier blob** stored inside the file's header.

The pitch: *"Your `.said` is your account. Drop it on the site, the site reads
the signed tier blob inside, the right features unlock. No password, no email
prompt, no auth roundtrip. Free users get basic. Pro users get document
ingest. Developer users get code + SQL. Lifetime users get everything,
forever."*

This document is the engineering + cryptography + operations + pricing plan
for building that, honestly scoped, with clear decision points before we
write any code.

---

## What we're building (the user-facing promise)

Three things, in order of importance:

1. **Drop-to-auth on a public WASM site.** The site is public. Anyone can
   load it. What you can do depends on which `.said` you drop. No login form,
   no email/password, no OAuth dance. The file is the credential.

2. **Offline-verifiable tier gating.** Once a user upgrades, the new tier is
   baked into their `.said` as a server-signed blob. After that, the WASM
   verifies the tier locally on every open — no network call needed. Works on
   a plane, on a USB stick, in an air-gapped bank.

3. **Anti-sharing by cryptographic binding.** A licence isn't a generic key
   somebody can paste into anyone's file. The signed tier blob is bound to
   the user's public key (which lives in the `.said` header). A friend who
   gets your licence string and tries to apply it to *their* `.said` will
   fail signature verification — the blob says "user X" and their file says
   "user Y."

What we are explicitly NOT building (out of scope for this milestone):

- A vendor SDK / "Sign in with .said" plugin for third-party sites. That is
  a separate workstream that builds on this one.
- An app-scoped preferences section (third-party apps writing into `.said`).
  Same — separate workstream.
- Recurring subscription infrastructure with metered usage. Tier blobs have
  an expiry; renewal is a re-fetch, not a meter.
- Account recovery via email if the user loses their `.said`. Loss of the
  file is loss of the account; that is the design, not a bug. (Backups are
  the user's responsibility — we'll document this prominently.)

---

## The pricing model this gates

The licence system has to encode the full pricing surface. From the approved
pricing document:

| Tier | Price | What unlocks |
|---|---|---|
| **Personal** | Free, forever | `home.said`, download brains from `said.directory`, query unlimited, CLI + MCP, `gert-1347.said`-style suffix handle |
| **Pro** | $9/mo · $79/yr | + Document RAG (PDF/DOCX/OCR/audio/video), multiple brains, publish up to 10 brains, private brains, clean handle |
| **Developer** | $15/mo · $129/yr | + AST-aware code memory (8 langs), symbol lookup, cross-repo synthesis, `--deep` mode, IDE integrations, unlimited publishing |
| **openclaw.said** | $5 one-time (capped at first 100k, then $19) | OpenClaw context-engine plugin SKU. Self-contained, doesn't grant other tiers. |
| **Founder Lifetime** | $119 one-time (capped at first 10k) | Pro + Developer forever, founder badge, first-crack handles, locked-in rate |
| **Standard Lifetime** | $150 (Y2+: $250, Y4+: $400) | Pro + Developer forever |
| **Enterprise** | Talk to us | Pointers + summaries only mode, DNS-verified handles, audit/compliance, on-prem |

Tier blobs MUST be able to express:
- Tier identifier (`Free` / `Pro` / `Developer` / `FounderLifetime` /
  `StandardLifetime` / `Enterprise` / `OpenclawAddon`)
- Expiry (`Date(YYYY-MM-DD)` for monthly/yearly, `Never` for lifetime/free)
- Optional add-on flags (e.g., `openclaw_addon: true` independent of base
  tier)
- Issued-at, issuer key fingerprint
- Bound user public key (anti-sharing)
- Server signature over all of the above

That schema is the contract between server and WASM. Everything else is
implementation detail.

---

## Architecture

```
        USER'S BROWSER (public WASM site)
        ┌────────────────────────────────────────────┐
        │  said.app (WASM, served from CDN)          │
        │   ├─ user drops file or picks via Circle   │
        │   ├─ reads .said header                    │
        │   ├─ extracts: pubkey, identity sig,       │
        │   │            TIER blob, BIND blob        │
        │   ├─ verifies TIER sig against pinned      │
        │   │   server pubkey (offline)              │
        │   ├─ verifies BIND signature using pubkey  │
        │   │   from same file (offline)             │
        │   ├─ checks expiry (offline)               │
        │   └─ feature_gate(action) -> bool          │
        └────────────────┬───────────────────────────┘
                         │
                         │  Only when buying / upgrading:
                         │  HTTPS POST /tier  (with challenge)
                         │
        ┌────────────────▼───────────────────────────┐
        │  Licence Server (Supabase prototype,        │
        │  Cloudflare Workers + D1 in production)     │
        │   ├─ users table (id, email, pubkey, tier,  │
        │   │   expires, stripe_customer)             │
        │   ├─ POST /signup       (create account)    │
        │   ├─ POST /tier         (issue signed blob) │
        │   ├─ POST /licence/file (offline-export)    │
        │   ├─ POST /webhook/stripe (payment events)  │
        │   ├─ signing key in secrets (NEVER in DB)   │
        │   └─ rate limits + abuse controls           │
        └────────────────────────────────────────────┘
```

### .said header additions

Two new sections inside the existing v7_x file format. Both are optional —
old files without them are treated as `Free` tier with no expiry, and the
WASM gates accordingly.

| Section | Contents | Size estimate |
|---|---|---|
| `IDNT` | User Ed25519 public key (32B) + self-signature over BLAKE3(file_root) (64B) + handle string (variable) | ~120 bytes |
| `TIER` | Server-signed CBOR blob: `{ user_pubkey, tier, expires, issued_at, issuer_kid, addons[], sig }` | ~200 bytes |
| `BIND` | User-signed proof: Ed25519 sig over `BLAKE3(TIER_blob \|\| nonce)` using the file's private key. Proves the holder of `IDNT.private` actively accepted this tier blob. | ~80 bytes |

**Total overhead per file:** ~400 bytes. Negligible.

These get added as new section markers in the v7_2 (or v7_3) header. The
existing header has 7 reserved offsets — we use three of them. Old readers
that don't know about IDNT/TIER/BIND skip them and treat the file as Free.

### Components in concrete terms

| Component | Language | LOC est. | Where it lives |
|---|---|---|---|
| `crates/sca-core/src/licence.rs` | Rust — sign/verify/parse blobs, gate helper | 400–600 | new module |
| `crates/said-cli/src/cmd/licence.rs` | CLI: `said licence apply <key>`, `said licence show`, `said licence export` | 200 | new file |
| `crates/said-wasm/src/licence.rs` | wasm-bindgen wrapper around `sca-core::licence` | 150 | new file |
| `crates/said-wasm/web/licence.js` | UI: feature gates, paste-licence dialog, upgrade banners | 300–500 | new file |
| `services/licence-server/` | Supabase project (SQL + Edge Functions) for prototype | 800 | new directory, not part of cargo workspace |
| Stripe integration | webhook handler, product setup, test-mode keys | 300 | inside licence-server |

### Key flows

**First-run (free tier, never bought anything)**
1. User runs `said init`. CLI generates Ed25519 keypair, writes IDNT section
   with public key + self-signature over the (empty) file root, no TIER
   section.
2. User opens `said.app`, drops file. WASM sees IDNT but no TIER → treats as
   Free tier, expiry Never. Basic features unlocked. Upgrade banner shown.

**Upgrade purchase (online flow — option C "auto-fetch")**
1. User clicks Upgrade → Pro on the WASM site. WASM extracts `user_pubkey`
   from the file's IDNT section and sends them to the licence-server's
   checkout endpoint.
2. Server creates Stripe checkout session bound to `user_pubkey`. User pays.
3. Stripe webhook fires → server bumps user's row to `tier=Pro,
   expires=2027-05-04`.
4. WASM polls `POST /tier { user_pubkey }`. Server returns signed TIER blob.
5. WASM hands TIER blob to a `sign_bind(blob)` function which uses the file's
   private key to produce the BIND signature.
6. WASM writes both into the `.said` (header rewrite — already proven safe by
   the v7 patching code in `said_file.rs`).
7. Page reloads with new tier active. Works offline forever (until expiry).

**Upgrade purchase (offline / closed-environment flow — option C "manual")**
1. User on a bank-issued laptop with no internet. They (or their IT admin)
   buy the licence via a browser on a different machine.
2. Server returns a base64 string: `licence-v1:<TIER_blob>`.
3. User pastes the string into `said licence apply` (CLI) or the
   "Apply licence" dialog in WASM.
4. Local code verifies the server signature, produces the BIND signature
   using the file's private key, writes both sections.
5. Same outcome as the online flow. Zero network involvement.

**Tier verification (every file open)**
1. WASM reads IDNT, TIER, BIND from header.
2. Verifies IDNT self-signature: `Ed25519::verify(IDNT.pubkey, file_root,
   IDNT.sig)`. Proves the file was created/owned by holder of IDNT private
   key.
3. Verifies TIER server signature: `Ed25519::verify(server_pubkey,
   tier_blob, tier.sig)`. Proves the licence is real.
4. Verifies BIND: `Ed25519::verify(IDNT.pubkey, BLAKE3(tier_blob),
   BIND.sig)`. Proves the holder of the IDNT private key accepted this
   licence (anti-sharing).
5. Checks `tier.expires` against today's date.
6. All four pass → tier active. Any one fails → fall back to Free.

**Expiry**
- WASM shows a "renew" banner starting 14 days before expiry.
- Day after expiry: tier silently downgrades to Free. Files keep working,
  paid features just become read-only / disabled.
- Re-purchase issues a fresh TIER blob. No data loss, ever.

**Lifetime tiers**
- `expires: Never`. Same flow, same blob format, just no date check.

**openclaw.said add-on**
- Independent boolean in the TIER blob: `addons: ["openclaw"]`.
- The OpenClaw context-engine plugin checks `tier.has_addon("openclaw")` at
  load time. Doesn't grant any other capability.

---

## Cryptography

This is the second-most-important section after the threat model. **The
licence system is an authorisation product. If the crypto is wrong, every
paid feature is free.**

### Algorithm choices

| Purpose | Algorithm | Why |
|---|---|---|
| User identity keys | Ed25519 | Standard, fast, small (32B pubkey, 64B sig), constant-time impls everywhere |
| Server signing key | Ed25519 | Same algorithm = one verification primitive in WASM |
| File-root commitment | BLAKE3 | Already used throughout `.said` for content addressing |
| TIER blob encoding | CBOR (deterministic) | Canonical form prevents signature-malleability attacks |
| Licence string transport | base64url(CBOR(TIER_blob)) | URL-safe, paste-able, no ambiguity |

**No self-rolled crypto. Use vetted crates only:**
- `ed25519-dalek` for sign/verify
- `blake3` (already a workspace dep)
- `ciborium` for canonical CBOR
- `base64` (URL-safe variant) for transport

### Key management

| Key | Generated | Stored | Lost = | Compromised = |
|---|---|---|---|---|
| User identity (private) | Locally on `said init` | Inside the `.said` file (encrypted at rest in a future "vault mode" milestone) | Account loss. Documented up front. | Identity theft for that one user. They issue a key-rotation; old `.said` becomes "compromised, please regenerate." |
| Server signing (private) | Once, at server bring-up | Cloudflare Workers Secrets / Supabase Vault. NEVER in DB. Backed up offline (sealed envelope + hardware token). | Cannot issue new licences. Existing licences keep working. We bring up a new key and re-sign every active licence (tracked by user ID). Painful but recoverable. | **Catastrophic.** Anyone can mint Lifetime tiers. Mitigation: rotate to a new server key, push WASM update with new pinned pubkey, re-sign every active licence, brick old blobs via a published revocation list embedded in WASM. |
| Server verification (public) | Derived from signing key | **Hardcoded into the WASM binary** at build time. Re-deploys with key rotation. | Not lossable — it's in source. | Not compromisable — it's public. |

### Anti-sharing: the binding mechanism in detail

The naive design ("sign tier with server key, paste into `.said`") fails
because anyone who buys a licence can post it to a forum. We use a
double-signature pattern:

```
TIER blob (server-signed, transferable):
  {
    user_pubkey: <user's Ed25519 public key>,
    tier: "Pro",
    expires: "2027-05-04",
    issued_at: "2026-05-04",
    issuer_kid: "saidhome-2026-rotation-1",
    addons: [],
    sig: <Ed25519(server_priv, CBOR(rest))>
  }

BIND blob (user-signed, NON-transferable):
  {
    tier_blob_hash: <BLAKE3(TIER blob bytes)>,
    nonce: <random 16 bytes>,
    sig: <Ed25519(user_priv, tier_blob_hash || nonce)>
  }
```

**Why this works against sharing:**
- A friend gets your TIER blob. They paste it into their `.said`. Their
  `.said`'s IDNT section says `pubkey = THEIR_pubkey`. Your TIER blob says
  `user_pubkey = YOUR_pubkey`. WASM checks `IDNT.pubkey == TIER.user_pubkey`
  — fails — falls back to Free.
- A friend tries to forge a TIER blob with their pubkey instead. They don't
  have the server's private key. Server signature verification fails. Falls
  back to Free.
- A friend takes your entire `.said` (your pubkey, your private key, your
  TIER, your BIND). Now they have your whole identity, including all your
  memories. This is equivalent to giving them your password manager. Same
  risk as drag-drop sharing on Circle. Documented.

### What we explicitly do NOT defend against

- A user who shares their entire `.said` file with a friend. That's the
  Apple-ID-shared-with-spouse problem — solvable only with biometrics or
  tied-to-a-device hardware enclave, neither of which fits "drop the file
  on a website."
- A WASM user running a forked WASM that ignores the gates. The official
  WASM binary at `said.app` is the only one that earns trust; modified
  forks running locally are out of our control. **However:** if a vendor
  embeds our verification (future "Sign in with .said" plugin), they ship
  the official WASM and the gate still holds.
- Time manipulation (user sets system clock to 2025 to bypass expiry). The
  WASM trusts `Date.now()`. To defend: server can re-issue blobs with very
  short expiries, or we can require an online revalidation every N days for
  paid tiers. Both add network dependence, which fights the offline-first
  promise. **Decision punted to operations [DECIDE #5].**

---

## Threat model

### Threat list

| # | Threat | Mitigation | Residual risk |
|---|---|---|---|
| 1 | Friend pastes user's licence string into their `.said` | BIND signature fails because their `.said`'s pubkey ≠ TIER's `user_pubkey` | None — math defeats this |
| 2 | Attacker forges a TIER blob | Cannot — needs the server signing key (lives in secret store, hardware-backed) | None unless server key compromised |
| 3 | Server signing key compromised | Rotate key, push new WASM with new pinned pubkey, embed revocation list in WASM update | Window between compromise and detection is "free Lifetime for everyone" — must be detected fast. Mitigate with HSM + audit logging |
| 4 | User shares whole `.said` with friend | Documented as "this is like sharing your password manager" | Yes. Out of scope to defend against |
| 5 | User runs modified WASM that ignores gates | We control `said.app` — official deployment is the trust anchor. Vendor-embedded use cases ship the official wasm | A user can self-host a modified WASM for personal use, but they don't get vendor-side benefits |
| 6 | User rolls back system clock to extend expired tier | None in v1 if we go pure offline | Yes — see [DECIDE #5]. Mitigations: short expiry windows, optional online revalidation |
| 7 | Replay of old TIER blob after user downgraded | TIER blobs include `issued_at`. We can publish a revocation list inside WASM updates (small JSON of `(user_pubkey, min_issued_at)` tuples) | Need the user to update their WASM occasionally, or for vendors to refresh their pinned WASM |
| 8 | Stripe webhook spoofed → fake tier upgrade | Standard Stripe signature verification on the webhook handler | None if implemented correctly |
| 9 | Licence-server database breach | `users` table contains `(id, email, pubkey, tier, expires, stripe_customer)`. Pubkeys + emails leak; private keys don't (we never have them). Damage = email list leak | Standard breach mitigation: Postgres at-rest encryption, rotate any session secrets |
| 10 | DoS on `/tier` endpoint | Cloudflare rate-limit; cached responses (TIER blobs are deterministic per `(user_pubkey, current_state)` until tier changes) | Acceptable degradation: people can't upgrade for the duration of an attack |

### Things we MUST do before any public launch

These are non-negotiable:

- **Threat model document** (separate doc, ~20 pages) reviewed by an external
  security consultant
- **Third-party crypto review** of the licence module + server signing
  pipeline. ~$10–20k, ~2 weeks
- **Hardware-key signing** for the server's signing key (YubiKey HSM minimum,
  Cloud HSM preferred for production)
- **Reproducible builds** of the WASM so anyone can verify the signed binary
  matches the public source — same standard as Circle
- **Public revocation procedure** documented before launch (what we do if
  the server key is compromised)
- **Bug bounty programme** — share the pool with Circle's

### Things that are NOT acceptable shortcuts

- ❌ Self-rolled crypto
- ❌ Storing the server signing key in the database, env vars in plaintext,
  or any cloud config that admins can read
- ❌ Skipping the BIND signature ("the server sig is enough") — this is the
  whole anti-sharing mechanism
- ❌ Letting the WASM accept TIER blobs without verifying both sigs
- ❌ "We'll add expiry checks later" — expiry is the whole point of
  subscriptions

---

## Operations

### Things we have to run

| Service | Purpose | Sees | Cost (small scale) | Cost (10k users) |
|---|---|---|---|---|
| Licence server (Supabase prototype, Workers + D1 in prod) | Signup, payment, tier issuance | email, pubkey, tier, payments | $0 (Supabase free) → $25/mo (Pro) | $50–150/mo |
| Stripe (or alternative) | Payment processing | full payment data | 2.9% + $0.30 per transaction | same |
| Cloudflare CDN + WASM hosting | Serves `said.app` | request metadata | $0 (free tier) | $5–20/mo |
| HSM / hardware key for signing | Holds server signing key | the key | $50 (YubiKey) one-time | $1–2k for Cloud HSM |
| Status page | Tells users if licence-server is down | — | $0 | $20/mo |

**Total at small scale:** ~$30/month. **Total at 10k users:** ~$200/month +
Stripe fees. Both are real recurring costs but bounded.

### Disaster scenarios

**Server signing key lost (not compromised, just lost):**
- Cannot issue new licences. Existing licences keep working forever (lifetime
  blobs) or until expiry (subscription blobs).
- Recovery: bring up new key, push WASM update that pins new key alongside
  old, re-sign every active licence over ~24h. Subscription users may
  briefly fail validation during this; fallback to Free is graceful.

**Server signing key compromised:**
- Treat as data breach. Revoke immediately, rotate, ship WASM update with
  embedded revocation list, re-sign every active licence within 48h.
- Public post-mortem within 7 days.

**Licence server down for extended period:**
- Existing users unaffected (verification is offline).
- New signups blocked. New purchases blocked. Renewals blocked.
- Status page communicates ETA. If down >7 days, announce ETA + reasons.

**We go out of business:**
- Open-source the licence-server code AND the signing key (only after we
  can no longer issue licences anyway).
- Existing lifetime users keep their files forever — verification doesn't
  need our server.
- Existing subscription users keep working until expiry, then fall back to
  Free. They can re-create their `.said` with a community-built tool that
  uses the published-key to mint local "free forever" blobs.
- This is the structural promise that makes the product trustable. We MUST
  publish this commitment in the pricing page before launch.

---

## Phased delivery

Five weeks of focused engineering for an honest MVP, plus 2 weeks of beta
hardening before public launch. Total: **~7 weeks**. This sequences after
the WASM ingest pipeline is stable; it does not block other workstreams.

### Phase 0 — Decisions and design (1 week)

Outcomes:
- This document approved by you
- Threat model document drafted (separate, ~20 pages)
- TIER blob CBOR schema locked
- Server signing key generated, stored in HSM, public half pinned in WASM
- Licence server table schema locked
- UI mockups: upgrade banner, paste-licence dialog, expiry warning,
  revocation notice

Gate: you sign off on this plan AND the threat model AND the TIER schema
before any code is written.

### Phase 1 — Core licence module (1 week)

Outcomes:
- `crates/sca-core/src/licence.rs` compiles, signs/verifies/parses blobs
- `crates/sca-core/src/said_file.rs` extended with IDNT/TIER/BIND sections
  (header v7_3 or whatever the next bump is)
- Round-trip tests: write IDNT, write TIER, write BIND, reload, verify all
  three signatures
- 100% unit test coverage on the cryptographic paths

Gate: an offline test produces a `.said` with a synthetic Pro TIER blob,
and a separate test rejects every tampered variant of it (wrong pubkey,
wrong expiry, missing BIND, replayed BIND from another file).

### Phase 2 — CLI + WASM integration (1 week)

Outcomes:
- `said licence apply <key>` works against the synthetic tier from Phase 1
- `said licence show` prints tier, expiry, addons, issuer kid
- `said licence export` produces a base64 string for offline transport
- WASM exposes `verify_tier(file_bytes)` and `apply_licence(file_bytes,
  licence_str)` via wasm-bindgen
- WASM `feature_gate(action)` helper used by every gated UI element

Gate: a Free `.said` dropped on `said.app` shows Free UI; a Pro `.said`
(synthetic) shows Pro UI; pasting a synthetic Developer licence string
upgrades the file in-place.

### Phase 3 — Licence server (Supabase) (1 week)

Outcomes:
- Supabase project with `users` table, RLS policies, `signup` Edge Function
- `POST /tier` Edge Function reads tier from DB, signs blob with server key,
  returns to caller
- `POST /licence/file` returns base64-encoded blob for manual export
- Stripe test-mode integration: checkout session, webhook handler, tier
  bumps on `payment_intent.succeeded`
- Hardcoded 3 test users for skeleton-mode (option C from brainstorming)

Gate: end-to-end test passes — a fresh user signs up, buys Pro in Stripe
test mode, the WASM auto-fetches the new TIER blob, the file shows Pro
features unlocked.

### Phase 4 — Hardening + crypto review (2 weeks)

Outcomes:
- All inputs from server fuzzed
- Constant-time hardening pass on all sig verifications
- Server signing key migrated from local file to HSM
- WASM bundle reproducible-built; pinned pubkey verified at runtime against
  hash baked into the binary
- External crypto review booked, executed, all findings fixed
- Revocation procedure rehearsed end-to-end (intentional key rotation in
  staging, verify all clients re-validate cleanly)

Gate: external review report shows 0 unfixed CRITICAL/HIGH and we publish
the report.

### Phase 5 — Closed beta (1 week)

Outcomes:
- 50 users with real Stripe charges (refunded after beta)
- Each tier purchased and verified end-to-end
- Offline export tested by 5 users in air-gapped environments
- Failure modes exercised: expired tier, revoked tier, tampered file,
  shared licence string

Gate: 50 users would recommend the upgrade flow to a friend.

### Phase 6 — Public launch (1 day, then ongoing)

Outcomes:
- Pricing page live at `said.ai/pricing` (already drafted)
- Stripe live-mode keys deployed
- Licence server cut over from Supabase to Cloudflare Workers + D1
- Founder Lifetime counter live (X / 10,000)
- openclaw.said SKU live (X / 100,000)
- Public revocation procedure published

Ongoing: weekly security review for the first month, monthly thereafter.
Quarterly key-rotation drills.

---

## What needs deciding before any code is written **[DECIDE]**

### 1. Pricing model lock

**Question:** Is the pricing in this doc (Personal Free / Pro $9/mo / Dev
$15/mo / openclaw.said $5 one-time / Founder Lifetime $119 / Standard
Lifetime $150 / Enterprise) the final shape we encode into the TIER blob
schema?

Why this matters: the TIER schema's `tier` enum is part of the
on-disk format. Adding tiers later is fine. Renaming or splitting tiers
later means every existing `.said` needs migration.

**Options:**
- (a) Yes, lock as-is.
- (b) Yes, but add a generic "custom" tier for Enterprise variations.
- (c) Refine pricing further before locking the enum.

**Your answer:**

### 2. Licence-server vendor

**Question:** Supabase for prototype, Cloudflare Workers + D1 for
production — keep or change?

**Options:**
- (a) Yes, exactly that sequence.
- (b) Skip Supabase, go straight to Cloudflare. Slower start, no migration.
- (c) Stay on Supabase forever. Simpler ops, slightly more vendor lock.
- (d) Self-host on a VPS. Most control, most ops burden.

**Your answer:**

### 3. Stripe vs alternative

**Question:** Stripe for payments?

**Options:**
- (a) Yes, Stripe (most coverage, best DX).
- (b) Paddle (handles VAT/tax for us — saves real accounting work in EU).
- (c) Both — Stripe primary, Paddle for EU. More integration work.
- (d) LemonSqueezy / Polar — newer indie-friendly options.

**Your answer:**

### 4. Account loss recovery

**Question:** What's our story when a user loses their `.said` file
(broken laptop, no backup)?

Why this matters: the file IS the account. No central password reset is
possible by design. But a user who paid Lifetime and lost their file is
going to be very unhappy.

**Options:**
- (a) "Backups are your responsibility. We document this prominently. Lost
  file = lost account. No exceptions." Most honest, hardest pill.
- (b) Allow a one-time "rebuild" via email verification: user re-creates
  `.said`, server re-issues TIER blob bound to the new pubkey, old pubkey
  added to a revocation list. Saves the customer relationship; weakens the
  "your file is your only credential" promise.
- (c) Encourage Circle for cross-device sync and treat "no backup" as
  user error, but allow case-by-case manual recovery via support.

**Your answer:**

### 5. Time-manipulation defence

**Question:** Pure offline expiry checks trust the system clock. A
sophisticated user can roll back the clock to extend an expired tier.
Do we defend against this?

**Options:**
- (a) Don't defend. Document it. "If you set your clock to 2020 to extend
  Pro, you're stealing $9/mo from us. We notice in aggregate, not per
  user."
- (b) Require online revalidation every 30 days for paid tiers. WASM
  refuses to gate beyond a fresh TIER blob older than 30 days. Hurts
  offline-first promise but plugs the hole.
- (c) Hybrid: online revalidation for subscription tiers, no check for
  Lifetime tiers. Lifetime users have already paid the most; subscription
  users are the ones with motive to dodge.

**Your answer:**

### 6. Pricing page tier counters

**Question:** The pricing page advertises live counters: "X / 10,000
Founder Lifetime sold" and "X / 100,000 openclaw.said sold." These need a
trustworthy source.

**Options:**
- (a) Derive from licence-server DB; cache for 5 min; show on pricing page
  via a public read-only endpoint. Easy. Trusted because it's our number.
- (b) Make it auditable: publish a daily-signed Merkle commitment to the
  total counts; pricing page shows the latest signed snapshot.
  Overengineered for v1 — defer to v2.
- (c) Hand-curated number, updated weekly. Honest but not "live."

**Your answer:**

### 7. Backwards compatibility for existing `.said` files

**Question:** People are already using `.said` files (we have a working
file format and CLI). When the licence system ships, what happens to
their files?

**Options:**
- (a) Existing files have no IDNT section → CLI/WASM treat as Free →
  user can `said licence init` to generate identity → from then on it's
  a normal account. No data loss, no forced migration.
- (b) On next CLI update, auto-generate IDNT for any file that lacks it.
  Less friction, but writes to user's file without explicit consent.
- (c) Refuse to open files without IDNT once the licence system ships.
  Forces migration but hostile.

**Your answer:**

---

## Risk register (top 5)

1. **Server signing key compromise.**
   Mitigation: HSM from day one, audit logging on every signature,
   rotation procedure rehearsed quarterly, revocation list shipped with
   every WASM build.

2. **Stripe / payment processor account closure.**
   Mitigation: maintain backup processor (Paddle or similar) ready to
   swap in within 48h. Don't take crypto / unconventional payment
   methods that increase closure risk.

3. **Pricing page lies about tier counters.**
   Mitigation: counters wired to the same DB that gates issuance. If we
   say "9,999 / 10,000 Founder Lifetime sold," the 10,000th genuinely
   closes the SKU. No manual override.

4. **Founder Lifetime sells out faster than we can support.**
   Mitigation: have the upgrade pipeline + audit dashboards ready
   *before* opening the SKU. 10k purchases in a single weekend is a
   real possibility for a launch event; don't ship without observability.

5. **Stripe webhook miss → user pays but doesn't get tier.**
   Mitigation: webhook handler is idempotent, replays are safe; pricing
   page polls `POST /tier` for 60 seconds after checkout completes;
   support runbook for "I paid and didn't get my tier" with one-click
   manual re-issue.

---

## What does NOT block licence shipping

So we don't get scope-creeped:

- The "Sign in with .said" vendor SDK (separate workstream)
- App-scoped preferences inside `.said` (separate workstream)
- DNS-verified Enterprise handles (separate workstream)
- The `said.directory` registry (separate workstream that consumes
  licence as a dependency, not the other way around)
- Vault mode (passphrase-encrypted `.said` at rest) — interacts but doesn't
  block; if shipped first, makes the licence story stronger

These are all separate workstreams. Licence is its own thing.

---

## What I will NOT do

- Touch any licence code until you sign off on this plan and the threat
  model
- Ship the licence system without external crypto review
- Add new tiers to v1 once Phase 0 is locked. Tier creep is the enemy of
  the on-disk schema
- Promise a launch date publicly until Phase 4 is complete
- Hardcode the server signing key in source for "convenience during
  development" — HSM from day zero in any environment that signs real
  blobs

---

## Summary table

| Aspect | Answer |
|---|---|
| What it is | Drop-to-auth tier gating via signed-blob inside `.said` header |
| Trust anchor | Server Ed25519 pubkey pinned in WASM; user identity Ed25519 pubkey in `.said` IDNT section |
| Anti-sharing | BIND signature requires user's private key; pasted licences alone don't unlock another file |
| Online required? | Only for purchase + initial issuance. Verification + gating are 100% offline forever after. |
| Server stack | Supabase (prototype) → Cloudflare Workers + D1 (prod) |
| Payment | Stripe |
| How long to MVP | 5 weeks build + 2 weeks beta = ~7 weeks |
| Cost to operate (small scale) | ~$30/month |
| Cost to operate (10k users) | ~$200/month + Stripe fees |
| Crypto-review budget | $10–20k |
| Critical pre-launch reqs | Threat model, crypto review, HSM, reproducible WASM, revocation procedure, bounty |
| What can ship tonight | This document only — code is a Phase 1+ workstream |

---

## Next step

Review this document. Answer the seven **[DECIDE]** sections inline.
Once those seven are locked, I write the threat-model document and we
begin Phase 0.

Until then, no licence code, no on-disk schema changes, no server
provisioning. The pricing page can be drafted in parallel because it
doesn't bind any code yet.

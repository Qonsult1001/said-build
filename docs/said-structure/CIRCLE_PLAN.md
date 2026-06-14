# Circle — Planning Document

> Status: **Draft for review.**
> Author: design conversation, 2026-05-02.
> Decision-required sections are marked **[DECIDE]**.

---

## TL;DR

Circle is a small native helper app + browser client that lets a `.said` file
stored on one of your machines (the **host**) be opened from a browser tab on
any other machine (the **client**), without the file ever sitting on a server
we run.

The pitch: *"You're in London, your memory is on your iMac in Cape Town. Open
said.app from any browser. Your iMac hands the file to your tab over an
encrypted peer tunnel. The bytes never live in our cloud."*

This document is the engineering + security + operations plan for building
that, honestly scoped, with clear decision points before we write any code.

---

## What we're building (the user-facing promise)

Three things, in order of importance:

1. **Cross-device access without copies.** I'm in London on a hotel laptop, my
   `.said` lives on my home Mac. Open `said.app`, click *Connect with Circle*,
   pick the file, work. No upload, no cloud copy, no account.

2. **Per-site permission console.** Every site that ever asks for a `.said`
   file is listed in Circle's tray UI with a revoke button. Drag-drop can't
   give you that — once a tab has the bytes, they're gone.

3. **Recents, securely cached.** Circle remembers which `.said` you've opened
   on which devices, when, and from where. Useful for audit ("I opened
   willie.said from a London IP at 14:32, then again from home at 19:00")
   and for making the daily flow one-click instead of file-picker every time.

What we are explicitly NOT building (out of scope for this milestone):

- Cloud sync, multi-master writes, conflict resolution
- A `.said` file repository / hosting service
- An account system or SSO
- Mobile background hosting on iOS (Safari can be a client; iOS cannot be a
  host without a native app, which is a separate ~3-month build)

---

## Architecture

```
        HOST DEVICE (your iMac, awake + online)
        ┌────────────────────────────────────────────┐
        │  Circle Agent (Rust, system tray)          │
        │   ├─ holds device key (ed25519, in OS      │
        │   │   keychain / Credential Manager /       │
        │   │   libsecret)                           │
        │   ├─ knows where willie.said lives on disk │
        │   ├─ runs WebRTC peer + Noise tunnel       │
        │   ├─ surfaces tray UI (recents, granted    │
        │   │   origins, revoke buttons, audit log)  │
        │   └─ prompts user (native OS dialog) for   │
        │       every new origin / device            │
        └────────────────┬───────────────────────────┘
                         │
                         │  ICE / STUN to find the peer
                         │  WebRTC datachannel for bytes
                         │  Noise XX handshake on top
                         │  (forward secrecy, replay-safe)
                         │
        ┌────────────────▼───────────────────────────┐
        │  Rendezvous + TURN relay (we run)          │
        │   ├─ helps two devices find each other     │
        │   ├─ relays bytes if direct P2P impossible │
        │   ├─ CANNOT decrypt (end-to-end Noise)     │
        │   └─ logs only metadata (durations, IPs)   │
        └────────────────┬───────────────────────────┘
                         │
        ┌────────────────▼───────────────────────────┐
        │  CLIENT DEVICE (browser tab, hotel laptop) │
        │   ├─ said.app loaded                       │
        │   ├─ user clicks "Connect with Circle"    │
        │   ├─ shows pairing-list of host devices    │
        │   │   (only those previously paired)       │
        │   ├─ on pick: WebRTC tunnel opens          │
        │   ├─ bytes stream into WASM brain          │
        │   └─ same WASM ingest path as drag-drop    │
        └────────────────────────────────────────────┘
```

### Components in concrete terms

| Component | Language | LOC est. | Where it lives |
|---|---|---|---|
| `crates/said-circle-agent` | Rust + tray UI (`tao` or `tauri`) | 5–8k | new crate |
| `crates/said-circle-rendezvous` | Rust async server | 1.5–2k | new crate, deployed to a tiny VPS |
| `crates/said-wasm/web/llm/circle.js` | JS WebRTC client | 500–800 | new file in the existing wasm web dir |
| `crates/said-wasm/web/circle-pair.html` | Pairing flow UI | 200 | new |
| Installers (signed) | platform-specific tooling | — | `dmg` / `msi` / `AppImage` build scripts |

### Key flows

**First-time pairing (host ↔ client)**
1. User installs Circle on their Mac (the host). Agent generates an ed25519
   device key, stores private half in OS keychain, prints public half + a
   6-digit pairing PIN.
2. User opens `said.app` on phone (the client). Clicks *Connect with Circle →
   Pair a new device*. Browser shows a "scan QR or enter code" dialog.
3. Client scans QR → contacts rendezvous → sends pairing intent.
4. Host's tray pops a system-level prompt: *"Pair with iPhone (Safari) at
   45.x.x.x?"* User enters the 6-digit PIN on the phone, host shows
   matching PIN, user taps Approve.
5. Both sides commit the pairing: each remembers the other's device-key
   fingerprint. **Server retains nothing about this pairing** other than the
   timestamp and IP-pair (rotated weekly).

**Daily access**
1. User opens `said.app` on any paired device. Clicks *Connect with Circle*.
2. Client lists the user's paired hosts ("Home iMac · last seen 2 min ago").
3. User picks a host. Client → rendezvous: "I want to talk to host-id X".
4. Rendezvous wakes the host (if online), forwards an ICE handshake.
5. Host's tray shows: *"Allow this iPhone session to read willie.said?
   [Allow] [Allow always] [Deny]"*. (Per-session by default, "always" is a
   one-click toggle for trusted devices.)
6. On Allow: WebRTC tunnel opens, Noise handshake runs, bytes stream from
   disk to browser. Same WASM ingest path as drag-drop.

**Revocation**
1. User opens Circle tray on host. Sees list of paired devices.
2. Clicks *Revoke* next to "iPhone (lost)".
3. Agent rotates the host's signing key. All in-flight sessions to that
   device fail within ~1 second. The revoked device is wiped from the
   pairing list.

---

## Security model

This is the most important section. **Circle is a security-critical product.**
We're going to be honest about every threat we can think of and how we
mitigate it.

### Threat list

| # | Threat | Mitigation | Residual risk |
|---|---|---|---|
| 1 | Bytes intercepted in transit | Noise XX over WebRTC datachannel; forward secrecy; the rendezvous/TURN relay sees only ciphertext | None at the wire layer. If TURN is compromised it can collect ciphertext but cannot read it. |
| 2 | Malicious site impersonates `said.app` | Origin-bound permissions in the agent; pairing is per-origin; system prompt shows full origin string + favicon | User can still mis-click Allow on a near-look-alike domain. Mitigate with origin-similarity warning. |
| 3 | Malware on the host machine | Out of scope — if your machine is owned, the file is accessible directly anyway. Circle doesn't make this worse. | Yes — this is the OS account's problem. |
| 4 | Lost / stolen client device | Host-side revocation; one-click in the tray; revoked tokens reject within 1s. Optional auto-revoke on inactivity. | A stolen device that's still paired and online has access until revoked. Mitigate with "require unlock" mode (passphrase prompt per session). |
| 5 | Replay attack (recorded handshake replayed later) | Noise XX is replay-safe by construction (fresh ephemeral keys per session) | None. |
| 6 | Man-in-the-middle on rendezvous | Pinned host key from the pairing step; rendezvous can't substitute identities without breaking the key fingerprint check | If user fails to verify the PIN during pairing, an attacker on the network at pairing time can MITM. Standard SAS limitation. |
| 7 | Supply-chain attack on Circle binary | Signed releases (Apple notarisation / Authenticode / GPG); reproducible builds from public source; SBOM published per release | If our signing key is stolen, attacker can ship signed updates. Mitigate with hardware key (YubiKey) for releases. |
| 8 | Rendezvous operator (us) goes rogue / is compelled by court order | Operator can see who connected to whom and when; CANNOT see file contents (E2E encrypted) | Metadata leakage. Documented in the privacy policy. Users in adversarial environments should self-host the rendezvous. |
| 9 | `.said` file at rest on host disk | Out of scope for Circle MVP — the file is plaintext on disk. **Vault mode** (passphrase-encrypted `.said`) is a separate workstream that compounds with Circle. | Laptop theft = file readable by attacker who has unlocked the laptop. Most likely real-world attack. Vault mode is the fix. |
| 10 | Browser compromise (malicious extension, CVE) | None at the browser layer — once bytes are in the tab, they're in the tab | Same risk drag-drop has. Document it. |

### Things we MUST do before any public launch

These are non-negotiable; without them, claiming "private memory" on the
splash is a marketing claim, not a security one:

- **Threat model document** (separate doc, ~30 pages) reviewed by an external
  security consultant
- **Third-party penetration test** of the agent + rendezvous + browser client.
  Reputable firm, ~$15–30k, ~3 weeks. We commit to fixing all CRITICAL and
  HIGH findings before launch
- **Reproducible builds** so anyone can verify the signed binary matches the
  public source
- **SBOM published** per release (CycloneDX or SPDX)
- **Bug bounty programme** funded with a real payout pool ($25k+ initial pool)
- **Hardware-key signing** (YubiKey or equivalent) for release signing, never
  cloud-CI-stored
- **Public security policy** with a `security@` contact, GPG key, 90-day
  embargo

### Things we MUST do during build, not after

- Noise XX implementation reviewed by someone other than the original author
  (preferably `snow` crate adopted as-is rather than custom)
- Forward secrecy verified per-session (new ephemeral keys, no key reuse)
- Constant-time comparisons for all key/MAC operations (use `subtle` crate)
- Fuzzing harness for the agent's network surface
- All inputs from the wire treated as untrusted; defensive-parse everything

### Things that are NOT acceptable shortcuts

- ❌ Self-rolled cryptography
- ❌ Storing the device private key anywhere outside the OS keychain
- ❌ Allowing pairing without an out-of-band confirmation step (PIN or QR)
- ❌ Allowing origin grants without explicit user prompt
- ❌ Logging file contents anywhere, ever
- ❌ "We'll add encryption later" — encryption is the product

---

## Operations

### Things we have to run

| Service | Purpose | Sees | Cost (small scale) | Cost (10k users) |
|---|---|---|---|---|
| Rendezvous | Helps two paired devices find each other | metadata (which device IDs talked, when, IP pair) | $5–10/mo VPS | $50–100/mo |
| TURN relay | Forwards traffic when direct P2P fails (~30% of cases due to NAT/firewall) | ciphertext only | $20–50/mo | $200–500/mo egress |
| Code-signing certificates | Apple notarisation + Authenticode + GPG release signing | — | ~$300/yr (EV cert) | same |
| Status page | Tells users if rendezvous/TURN is down | — | $0 (Statuspage free tier) | $20/mo |

**Total at small scale:** under $100/month. **Total at 10k users:** under
$1k/month, dominated by TURN egress. Both are real recurring costs.

**Self-host option:** the rendezvous + TURN code is open-source from day one.
Privacy-paranoid users (governments, journalists, defence) can run their own
infrastructure. A 5-minute Docker compose recipe should be the public default.

### Distribution

- **macOS:** signed `.dmg`, notarised by Apple. Released via direct download
  + Homebrew cask.
- **Windows:** signed `.msi` with Authenticode EV cert.
- **Linux:** signed `AppImage` + `.deb` + `.rpm`.
- **Auto-update:** Sparkle on macOS, WinSparkle on Windows, native package
  manager on Linux. Updates are signed and verified before install.

---

## Phased delivery

Six weeks of focused engineering for an honest MVP, plus 4 weeks of beta
hardening before public launch. Total: **~10 weeks**. This is the minimum
that ships something we can defend on a security review.

### Phase 0 — Decisions and design (1 week)

Outcomes:
- This document approved by you
- Threat model document drafted (separate, ~30 pages)
- API contract between agent and browser client locked
- Wire protocol locked (Noise XX over WebRTC datachannel, msgpack frames)
- UI mockups for: tray menu, pairing flow, permission prompt, audit log

Gate: you sign off on the threat model AND the wire protocol before any
code is written.

### Phase 1 — Agent + rendezvous skeleton (2 weeks)

Outcomes:
- `said-circle-agent` crate compiles on macOS / Windows / Linux
- System tray icon + menu (no real functionality yet, just structure)
- Rendezvous server boots; ICE handshake passes through it
- WebRTC peer connection establishes in a contrived test (host on one machine,
  client browser on the same LAN)

Gate: a hardcoded test file streams from one machine to a browser tab on the
same LAN, with no encryption layer yet. Proves the plumbing.

### Phase 2 — Pairing + Noise tunnel (2 weeks)

Outcomes:
- Device key generation + OS-keychain storage (Keychain / Credential Manager /
  libsecret)
- QR-code pairing flow, PIN confirmation, mutual fingerprint pin
- Noise XX handshake on top of the WebRTC datachannel
- Forward secrecy verified manually with traffic captures
- Pairing list persisted across restarts; revocation rotates keys

Gate: a paired browser on a different network can connect to a paired host
across the public internet, encrypted end-to-end. External crypto reviewer
reads the handshake code.

### Phase 3 — Permission console + audit (1 week)

Outcomes:
- Per-origin permission grants (browser asks → host prompts user → grant
  recorded)
- Revoke from tray UI works in real-time
- Audit log: every grant, every session, every revocation, with timestamps
- Recents list: last 20 files opened from each paired device
- Optional "always allow" toggle per origin, default off

Gate: usability test with 5 internal users; nobody gets confused; the audit
log makes sense to a non-technical user.

### Phase 4 — Hardening + pen-test (3 weeks)

Outcomes:
- All inputs from the wire fuzzed (cargo-fuzz, libFuzzer)
- Constant-time hardening pass (audit with `subtle` crate)
- Reproducible builds verified across two independent CI environments
- SBOM generation in the release pipeline
- Hardware-key signing on the release machine
- External pen-test booked, executed, all CRITICAL+HIGH findings fixed
- Bug bounty programme launched at this point with a private group (50 users)
  before public

Gate: pen-test report shows 0 unfixed CRITICAL/HIGH and we publish the report.

### Phase 5 — Closed beta (2 weeks)

Outcomes:
- 50 users across 3+ time zones use Circle for daily work
- Crash-free rate ≥99% over the 2-week window
- Telemetry (opt-in) shows the actual TURN-fallback rate, handshake durations,
  failure modes
- Usability bugs filed and fixed
- Documentation: install, pair, troubleshoot, FAQ, security FAQ

Gate: 50 users would recommend it to a friend, measured by short survey.

### Phase 6 — Public launch (1 day, then ongoing)

Outcomes:
- Public download page: `circle.said.ai/download`
- Splash button on `said.app` switches from "coming soon" to live
- Self-host docs published
- Security policy + bug bounty page live
- Status page wired

Ongoing: weekly security review for the first month, monthly thereafter.

---

## What needs deciding before any code is written **[DECIDE]**

### 1. Operations commitment

**Question:** Are you committed to running and paying for the rendezvous +
TURN infrastructure indefinitely, even if Circle is free for users?

Why this matters: without these, Circle works only on the same LAN. Across
networks, you must run a relay. That's $5–500/month depending on scale, plus
operational responsibility for uptime.

**Options:**
- (a) Yes, we run it. (~$50–500/mo)
- (b) Bake in self-host as the default; we don't run a public one. Users who
      don't want to self-host can pay a vendor like Cloudflare for managed
      TURN.
- (c) Only on-LAN for v1; document it; cross-network in v2.

**Your answer:**

### 2. Security audit budget

**Question:** Are you committing to a paid third-party security audit before
public Circle launch?

Why this matters: without an audit, "private memory" is a marketing claim,
not a verified one. The audit is what lets you defend the pitch when someone
serious starts probing.

**Options:**
- (a) Yes, $20–30k pen-test plus $25k initial bug bounty pool.
- (b) Smaller audit ($10k) + larger bounty pool ($50k).
- (c) Skip audit; rely on bounty programme alone.
- (d) Skip both; ship at your own risk.

**Your answer:**

### 3. Splash messaging — what do we say *today*?

**Question:** Right now, the splash has a "Connect with Circle" card that
toasts "coming soon." What should it say while Circle is being built?

**Options:**
- (a) Drop the card entirely. Drag-drop is the only path until Circle ships.
- (b) Keep it, label honestly: *"Connect with Circle (coming Q3) — your
      devices in one private mesh, no cloud, no copies."*
- (c) Keep it as is. Toast on click. Set expectation later.

**Your answer:**

### 4. Mobile hosting — iOS app?

**Question:** Eventually, do we want the user's iPhone to be able to be the
HOST (not just the client)? E.g. their `.said` lives on the phone and a
laptop fetches it?

Why this matters: iOS Safari can't run a background WebRTC peer. To make the
phone a host, we need a native iOS app, which is a separate ~3-month build.
Android with Chrome PWA can be a host today.

**Options:**
- (a) iOS host is a future workstream, document it as known-not-yet.
- (b) iOS host is a launch blocker — wait to ship Circle until the iOS app
      is also ready (~+3 months).
- (c) Phone is client-only forever, file always lives on a real computer.

**Your answer:**

### 5. Vault mode (passphrase-encrypted `.said` at rest)

**Question:** Vault mode is a SEPARATE feature that encrypts the `.said` file
on disk with a passphrase. It's the single biggest security win for the most
likely real attack (laptop theft). Do we ship vault mode AT THE SAME TIME as
Circle, or as a separate milestone?

Why this matters: if the file is plaintext on disk, Circle's transit
security is undermined — an attacker who steals the host machine just opens
the file directly.

**Options:**
- (a) Ship vault mode WITH Circle (adds ~2 weeks but closes a real gap).
- (b) Ship vault mode separately, before Circle (sequence: vault → Circle).
- (c) Skip vault mode; document the residual risk; leave as user's
      responsibility.

**Your answer:**

### 6. Naming + brand

**Question:** Is "Circle" the final name? It's generic. Trademark search
hasn't been done. Other "Circle" products exist (the cryptocurrency company,
the social-network startup, etc.).

**Options:**
- (a) Keep "Circle" — do trademark search before any public mention.
- (b) Rename — suggestions: *Mesh*, *Ring*, *Tether*, *Echo*, *Halo*, *Anchor*.
- (c) Skip the brand naming for now, internal codename only until close to
      launch.

**Your answer:**

---

## Risk register (top 5)

1. **Pen-test finds a critical issue → 4-week launch delay.**
   Mitigation: book the audit early, in phase 4, not phase 6. Build in
   schedule slack.

2. **TURN egress costs explode at scale.**
   Mitigation: forecast at 10k DAU before launch; if costs > $1k/mo, we
   switch to a paid tier model OR aggressive self-host promotion. Decision
   point at 1k users.

3. **iOS Safari WebRTC quirks block client-side connection.**
   Mitigation: prototype on iOS Safari in phase 1, not phase 5. If
   blocking, scope down v1 to "desktop-to-desktop only" and announce iOS
   in a follow-up.

4. **Apple/Microsoft revoke our code-signing cert (false-positive abuse
   report).**
   Mitigation: use EV cert from day one, maintain backup signing identity,
   build a signed/notarised release pipeline early so we're not surprised.

5. **Bug bounty surfaces a critical issue post-launch.**
   Mitigation: 90-day embargo, hot-patch pipeline ready, communications
   plan drafted. Treat it as inevitable, not an exception.

---

## What does NOT block Circle

So we don't get scope-creeped:

- The admin dashboard mockup
- The Open Admin Dashboard button
- The View Activity button on the rail
- The four-card-rail polish
- Multi-agent SAID system (searcher / writer / compiler)
- The Phase 2 prompt-architecture work for forge

These are all separate workstreams. Circle is its own thing.

---

## What I will NOT do

- Touch any Circle code until you sign off on this plan and the threat model
- Ship Circle without a third-party audit, regardless of timeline pressure
- Add new features to v1 once phases 0–1 are locked. Feature creep is the
  enemy of security
- Promise a launch date publicly until phase 4 is complete

---

## Summary table

| Aspect | Answer |
|---|---|
| What it is | Cross-device file bridge, encrypted, no cloud copies |
| Who runs the relay | We do (small VPS) — open-source for self-host |
| How long to MVP | 6 weeks build + 4 weeks beta = ~10 weeks |
| Cost to operate (small scale) | <$100/month |
| Cost to operate (10k users) | ~$1k/month |
| Pen-test budget | $20–30k |
| Bug bounty initial pool | $25k+ |
| Critical pre-launch reqs | Audit, reproducible builds, SBOM, signed releases, bounty |
| What can ship tonight | Splash messaging change only — Circle itself is a real workstream |

---

## Next step

Review this document. Answer the six **[DECIDE]** sections inline (just edit
the file or reply to me with your answers). Once those six are locked, I
write the threat-model document and we begin phase 0.

Until then, the splash button stays as a stub. I won't touch it.

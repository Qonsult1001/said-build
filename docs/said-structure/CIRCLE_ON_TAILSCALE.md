# Circle on Tailscale — Design Document

> Status: **Decisions locked (2026-06-01) — ready to build on go-ahead.**
> Supersedes the networking half of `CIRCLE_PLAN.md`.
> Decision: build Circle on Tailscale (`tsnet` embedded, Go sidecar), not on
> a hand-rolled WebRTC + Noise + rendezvous + TURN stack. All five design
> forks resolved in favour of the simplest one-click, zero-install experience.

---

## Why this document exists

`CIRCLE_PLAN.md` proposed building the cross-device `.said` bridge from
scratch: custom WebRTC datachannels, a hand-rolled Noise XX tunnel, a
rendezvous server we operate, a TURN relay we pay for, our own device-key
pairing, and a $20–30k third-party pen-test of all that custom cryptography.
That is ~10 weeks of work and a permanent operational + security burden.

**Tailscale already is that stack — audited, battle-tested, and free for this
scale.** It is WireGuard (audited) + a coordination plane + best-in-class NAT
traversal + device identity + instant revocation. Re-implementing it would
produce a worse, unaudited version of a solved problem.

So Circle becomes a **thin `.said`-specific layer on top of Tailscale**:
serve the file over the tailnet, fetch it in the browser, gate it with
consent. Weeks, not months. No custom crypto to audit.

---

## Enterprise identity — white-label roadmap (decided 2026-06-02)

**Decision: ship Circle on Tailscale now; white-label the identity later for
enterprise.**

The friction point: a user clicks "sign in to Tailscale" → lands on Tailscale's
real login (Sign in with Google / Microsoft / GitHub / Apple / passkey). That
IS a one-click professional login — but it shows the **Tailscale brand** and
requires a **Tailscale account**. For enterprise that's not ideal: customers
should log into a **.said / Circle-branded** screen, not Tailscale's.

Why an account is unavoidable (any provider): reaching a device globally and
proving "this is me" *requires* an identity + coordination service. The choice
is never "account vs no account" — it's **whose** account. Options, all with
the same property:

| Path | Login screen the user sees | Who runs the control plane |
|---|---|---|
| **Tailscale** (now) | Tailscale's (Sign in with Google…) | Tailscale Inc. (free tier) |
| **Headscale** (white-label) | **Yours** (.said-branded) | **You** (self-hosted control server) |
| **NetBird** | Yours or theirs (SSO) | You or NetBird |
| Raw WireGuard | none, but manual key exchange per device | You, by hand (bad UX) |

**The enterprise replacement for Tailscale = Headscale (or NetBird).** Both are
open-source Tailscale-compatible control servers. With Headscale, the host
agent (`tsnet`) points at YOUR control server (tsnet supports a custom control
URL), so customers authenticate against a **Circle/.said-branded** login and
never see "Tailscale." This removes the branding friction entirely.

**Sequencing:**
1. **Now:** Circle on Tailscale's free plane — works today, one-click Google
   login. Good enough to prove the product + onboard early users.
2. **Enterprise upgrade (later):** stand up Headscale (or NetBird), point the
   sidecar's tsnet at it via a control-URL flag, brand the login. No change to
   the .said / serve / launcher code — only the control plane the node joins.
   This is the "no Tailscale account, log into us instead" enterprise story.

Not built yet — recorded so the path is explicit and not lost.

---

## FINAL SHAPE (2026-06-02) — "Circle, powered by Tailscale"

After working through OAuth and cloud-proxy options, the simplest design won —
and it's the right one. **Don't reinvent device discovery. Lean entirely on
Tailscale.**

> **Pick a `.said` → Circle starts a local server → you get a URL → open that
> URL from anywhere, as long as you have Tailscale. That's it.**

The whole onboarding is: **"Circle needs Tailscale (free) — install it, then
share."** Circle auto-detects if Tailscale is missing and prompts to install
it. No OAuth, no cloud, no API keys, no pasting addresses to *connect* (you get
a real URL to open).

Why this beats the alternatives we considered:

- **Browser OAuth to Tailscale's API** — rejected. Tailscale OAuth needs a
  `client_secret` (unsafe in a browser) and likely no CORS; it would force a
  server proxy anyway. And listing devices doesn't help — a browser still
  can't pull bytes off a peer without a server there. OAuth solved nothing the
  host doesn't already solve.
- **Cloud proxy / cloud-hosted .said** — rejected. Breaks the "your bytes
  never live in our cloud" promise.
- **Tailscale itself** already solves "find my devices by name" — once a user
  runs Tailscale, `home-imac.<tailnet>.ts.net` just resolves and is reachable.
  We don't re-solve that; we ride it.

### The launcher (the only new piece)

The host already serves the brain + app at a capability URL (Phases 1–3, built
and compiling). The one gap was that starting it was a command line. The
launcher closes that:

- **Pick → Share → Link.** A tiny local control UI: "Choose .said…" → pick
  file(s) → "Share" → it starts the host and shows the URL + a copy button +
  a QR code. (Served by the host on `127.0.0.1` and opened in the browser — no
  native-GUI toolchain; works on every OS.)
- **Tailscale auto-prompt.** On launch, Circle checks for Tailscale; if absent,
  it links the installer and explains the one-time setup before sharing.

### The connecting side

Open the URL on any device **that has Tailscale** → app + brain load. No paste,
no OAuth. (Funnel public-link mode remains for sharing with people who don't
have Tailscale — the other, later, front door.)

**Net:** the magic auto-discovery picker (Phase 3) stays as a convenience, but
it is no longer load-bearing. The core flow is pick-file → get-URL → open-URL,
exactly as the user described — the simplest thing that is also private and
global.

---

## The product, in one sentence (refined 2026-06-02)

> **Click Circle → see all *your* environments, wherever they are in the world
> → pick one → connected.**

Circle v1 is YOUR personal device picker, not (yet) a share-with-others tool.
The list of environments is your **tailnet itself** — every machine of yours
that is online and running a said host appears automatically. No manual
address entry, no link-pasting: your devices just show up. You click "Home
iMac", your brain loads from Cape Town into the browser tab in London.

Two decisions that shape this:

- **Tailnet auto-discovery.** The browser asks which of your devices are
  online and serving, and lists them. The tailnet *is* the environment list.
  (Implementation: each host exposes a tiny `/circle/whoami` identity endpoint;
  the picker enumerates your tailnet peers — via the local Tailscale client's
  status API where available — and probes for said hosts.)
- **Mine-first, share-later.** v1 connects you to your OWN environments. The
  share-a-link-with-someone-else flow (Funnel, capability token) is already
  built into the Phase 1 sidecar and stays as a *later* addition — same host,
  different front door. v1's front door is the my-environments picker.

---

## What Tailscale gives us (so we don't build it)

| `CIRCLE_PLAN.md` was going to build | Tailscale provides, audited |
|---|---|
| WebRTC + Noise XX encrypted tunnel | WireGuard mesh (audited, formally analysed) |
| Rendezvous server (we run + pay) | Coordination plane (DERP) |
| TURN relay for NAT traversal (~30% of cases) | NAT traversal + DERP relays (better than WebRTC) |
| ed25519 device keys in OS keychain | Device identity + key management |
| QR/PIN pairing + fingerprint pinning | Tailscale login + device authorization |
| Per-device revocation | Admin console / ACLs — revoke a device instantly |
| $20–30k pen-test of our crypto | Already pen-tested; SOC 2; open-source core |

Threats #1, #5, #6, #7, #8 from `CIRCLE_PLAN.md`'s table are **Tailscale's
problem now**, and they have solved them properly.

---

## What we still own (the thin layer)

Three small pieces:

1. **Host agent — `said serve`.** A command that joins the tailnet via
   embedded `tsnet` and serves a chosen `.said` file (or a small library of
   them) over HTTP, *only* on the tailnet interface — never on `0.0.0.0`.

2. **Browser fetch.** The WASM app already loads `.said` files by URL
   (`fetch(url) → arrayBuffer() → new SaidBrain`; see
   `crates/said-wasm/phase1-module.js:249`). "Connect with Circle" becomes:
   ask the host's tailnet address, fetch the file, hand the bytes to the same
   ingest path drag-drop uses. **No new load path** — we reuse the proven one.

3. **Consent + listing.** The host agent decides which files it exposes and to
   whom (a one-time approve per requesting device), and shows a tray/CLI list
   of what's shared + an access log. This is the part Tailscale does *not* do
   for us — it secures the pipe, not the per-file authorization.

---

## The `tsnet` decision (and its honest cost)

**Decision: embed `tsnet`.** `tsnet` is Tailscale's library form — the host
agent joins the tailnet *itself*, so the user does **not** install the
separate Tailscale client. One binary, one `said serve`, smoother UX.

**The cost we are accepting, stated plainly:**

- `tsnet` is **Go**. SAID-ECHO is Rust + WASM. Embedding `tsnet` means a Go
  component in the build. Two viable shapes:
  - **(A) Sidecar binary:** a small Go program (`said-circle-host`) built
    separately, shipped alongside the Rust CLI, launched by `said serve`.
    Cleanest separation; the Rust side just supervises a child process and
    talks to it over a local socket. **Recommended** — no cgo, no FFI, two
    toolchains but zero interop fragility.
  - **(B) cgo/FFI into Rust:** link `tsnet` into the Rust binary via a C
    shim. One binary, but cgo cross-compilation is painful (especially
    Windows) and couples the build. **Not recommended** for v1.
- We take a dependency on Tailscale the company's coordination plane (unless
  the user points at a self-hosted **Headscale** control server — which
  `tsnet` supports, and which we document for the privacy-paranoid).
- Auth: the host agent needs to authenticate to the tailnet once. `tsnet`
  uses an auth key or interactive login on first run; we store the resulting
  state in the OS-appropriate config dir.

This is a real, named tradeoff — a Go sidecar in an otherwise Rust/WASM
project — accepted deliberately in exchange for not hand-rolling (and
auditing) a WireGuard-class network.

---

## Architecture

```
   HOST DEVICE (your iMac, awake + online)
   ┌───────────────────────────────────────────────┐
   │  said serve  (Rust CLI command)               │
   │    ├─ supervises ↓                            │
   │  said-circle-host  (Go sidecar, embeds tsnet) │
   │    ├─ joins the tailnet as a node             │
   │    ├─ serves chosen .said over HTTP on the    │
   │    │   TAILNET interface ONLY (never 0.0.0.0) │
   │    ├─ per-device consent: first fetch from a  │
   │    │   new tailnet peer prompts the host user │
   │    ├─ access log: who fetched what, when      │
   │    └─ token-guards each file (capability URL) │
   └───────────────────────┬───────────────────────┘
                           │  WireGuard (Tailscale) — encrypted,
                           │  NAT-traversed, peer-to-peer
   ┌───────────────────────▼───────────────────────┐
   │  CLIENT (browser tab on a paired tailnet dev)  │
   │    ├─ said.app loaded                          │
   │    ├─ "Connect with Circle" → enter/choose     │
   │    │   host (e.g. home-imac.tailXXXX.ts.net)   │
   │    ├─ fetch(https://host.tailnet/f/<token>)    │
   │    └─ bytes → new SaidBrain  (existing path)   │
   └───────────────────────────────────────────────┘
```

Note: the client browser must itself be on the tailnet (the user's device
runs Tailscale, OR we expose the host via Tailscale **Funnel** / **Serve** —
see Open Questions). The bytes only ever traverse the user's own tailnet.

---

## Consent + authorization model (the part Tailscale doesn't do)

Tailscale secures *the pipe* and authenticates *the device*. It does NOT know
that "this tab may read willie.said but not card.said." That per-file consent
is ours:

- **Capability URLs.** Each shared file is served at
  `https://host/f/<random-token>`, not at a guessable path. The token is the
  capability. Revoking = forgetting the token.
- **First-fetch prompt.** The first time a new tailnet peer requests a file,
  the host agent prompts the user (tray dialog / CLI confirm): *"Allow
  laptop-london to read willie.said? [once] [always] [deny]."* Default: once.
- **Access log.** Every fetch recorded: peer hostname, tailnet IP, file, time.
  Surfaced in the agent and (optionally) written into the brain's audit log so
  it's visible in the same place as vault chain-of-custody.
- **Tailscale ACLs as the coarse gate.** The user's tailnet ACLs already
  decide which devices can even reach the host's serve port. Our consent is
  the fine-grained, per-file layer on top.

---

## Security model (what changed vs CIRCLE_PLAN.md)

| # | Threat | Mitigation now | Residual |
|---|---|---|---|
| 1 | Bytes intercepted in transit | WireGuard (audited). Not our code. | None at wire layer |
| 2 | Malicious site impersonates said.app | Capability token + per-device consent; serve only on tailnet | Mis-click "always" on a look-alike — mitigate with clear host name in prompt |
| 3 | Malware on host | Out of scope (owns the file anyway) | OS account's problem |
| 4 | Lost/stolen client device | Tailscale device revocation (instant, admin console) + our token revoke | Stolen+online+unrevoked = access until revoked |
| 5 | Replay / MITM / forward secrecy | WireGuard handles all of this | None we own |
| 6 | Coordination-plane operator (Tailscale) compromise | They see metadata (who's on the tailnet), never file bytes. Self-host Headscale to remove even that. | Metadata to Tailscale unless self-hosted |
| 7 | Supply chain — Tailscale binary | Tailscale's signed, audited, reproducible releases (their burden) + our small sidecar (our burden — sign it) | Our sidecar must be signed/SBOM'd |
| 8 | `.said` plaintext on host disk | **Unchanged from CIRCLE_PLAN.md** — Vault-mode (passphrase-encrypted .said at rest) is still a separate, compounding workstream | Laptop theft = file readable. Vault mode is the fix. |
| 9 | Host serve port exposed beyond tailnet | Bind to the tailnet interface ONLY; never 0.0.0.0; integration-test this | Misconfiguration — make it impossible, not just default |

**The single biggest win:** we no longer ship custom cryptography, so the
$20–30k "audit our crypto" line item largely disappears. What remains to audit
is small: the sidecar's HTTP surface + the consent logic + the
bind-to-tailnet-only guarantee. That is a code review, not a cryptographic
audit.

---

## Browser integration (concrete)

The WASM app already has the load path. The change is small:

1. `splashCircle` click (today: `phase1-module.js:964`, a "coming soon" toast)
   → opens a small "Connect with Circle" panel.
2. Panel asks for / remembers the host address (e.g.
   `home-imac.tailXXXX.ts.net`) and lists files the host offers (a
   `GET /list` returning `[{name, token}]`, itself consent-gated).
3. On pick: `fetch("https://<host>/f/<token>")` → `arrayBuffer()` →
   the **existing** `new SaidBrain(bytes, …)` flow (same as
   `loadBrain`/the `fetch` at line 249).
4. Recents: remember `{host, file, lastOpened}` in the existing IndexedDB
   vault registry, so daily access is one click.

No new WASM binding is required for the happy path — it's a `fetch` the app
can already do. (CORS: the sidecar sets permissive CORS for the said.app
origin, or we serve said.app itself from the host — see Open Questions.)

---

## Phased delivery (Tailscale version)

Far shorter than `CIRCLE_PLAN.md`'s 10 weeks, because the network is done.

### Phase 0 — Design sign-off (this doc)
- You approve this document + the `tsnet` sidecar decision.
- Lock the consent model and the bind-to-tailnet-only requirement.

### Phase 1 — Go sidecar serves a file over the tailnet (~1 week)
- `said-circle-host` (Go) joins a tailnet via `tsnet`, serves one hardcoded
  `.said` at `/f/<token>` on the tailnet interface only.
- Manual test: a browser on another tailnet device fetches it.
- **Gate:** file streams device-to-device over WireGuard, never exposed off
  the tailnet (verified with a port scan from outside).

### Phase 2 — `said serve` + consent + listing (~1 week)
- Rust `said serve` command supervises the sidecar, picks which file(s) to
  share, handles first-run tailnet auth.
- First-fetch consent prompt + access log + `/list` endpoint.
- Token generation + revoke.
- **Gate:** sharing a file prompts on first access; revoke kills access.

### Phase 3 — Browser "Connect with Circle" (~3–5 days)
- Replace the splash stub with the connect panel: host address, file list,
  fetch-into-brain, recents.
- **Gate:** end-to-end — open said.app on device B, Connect with Circle, pick
  willie.said on device A's host, it loads. No drag-drop, no cloud.

### Phase 4 — Hardening + packaging (~1 week)
- Sign the sidecar (Authenticode / notarise / GPG); SBOM.
- Code review of the sidecar HTTP surface + consent + the
  bind-to-tailnet-only guarantee (a review, not a crypto audit).
- Self-host Headscale documented for the privacy-paranoid.
- **Gate:** review clean; bind-only guarantee proven by test.

**Total: ~3–4 weeks** vs ~10. No custom-crypto pen-test line item.

---

## Decisions [DECIDED 2026-06-01]

**Guiding principle (from the user): simplicity for non-technical users —
"click of a button, as simple as possible."** Every fork below is resolved in
favour of the simplest end-user experience.

### 1. Does the client browser's device need Tailscale too? → **BOTH, default Funnel**

**Decision: support both, but DEFAULT to the zero-install path (Tailscale
Funnel).** Rationale: the simplest possible experience for a non-technical
client (a hotel laptop, a stranger's machine) is *install nothing* — open
said.app, click Connect, done. That requires **Funnel** (host exposes the
file over public TLS, gated by capability token + first-fetch consent).

- **Default — Funnel (zero-install, one-click):** any browser can connect.
  Still TLS + token + consent; public-reachable rather than mesh-only.
- **Toggle — Tailnet-only (max privacy):** for users who run Tailscale on the
  client too; bytes never leave the mesh. A switch in `said serve`
  (`--tailnet-only`) and a clear indicator in the UI.
- The host UI states plainly which mode a share is in ("anyone with the link"
  vs "my devices only").

### 2. Headscale — document only, or first-class? → **DOCUMENT ONLY**

**Decision: document-only; default to Tailscale's control plane (zero config).**
Self-hosting a control plane is the opposite of simple. Non-technical users
never see Headscale; it's a footnote for governments / journalists. Tailscale's
plane is the one-click default.

### 3. Where does said.app run? → **HOST SERVES THE APP (self-contained)**

**Decision: the host sidecar serves said.app itself.** Counter-intuitively this
is the *simpler* user experience: the client opens ONE URL and gets both the
app and the file — no CORS, no "load app from X, file from Y." One address,
one click, fully self-contained on the host. Slightly more in the sidecar,
much simpler for the user.

### 4. Vault-mode (encrypted-at-rest) → **STAYS — encrypted at rest**

**Decision: keep vault mode; the `.said` is passphrase-encrypted on disk.** This
is the real protection against the most likely attack (laptop theft) and is
independent of Circle. Sequencing (with vs before Circle) still open, but it
ships — not deferred.

### 5. Name → **KEEP "Circle"**

**Decision: Circle stays.** (Trademark search still TODO before any public
mention; Tailscale is the mesh under the hood, Circle is the front-door button.)

---

## What can ship today vs what's a real build

- **Today (no risk):** the splash button copy can change from "coming soon"
  to an honest "Connect with Circle (Tailscale-powered) — coming soon" if you
  want. The connect *feature* is a real ~3–4 week build.
- **Not in v1:** iOS-as-host (still needs a native app — but Tailscale on iOS
  makes iOS-as-*client* trivial, which is better than `CIRCLE_PLAN.md`'s
  position). Cloud sync, multi-master, conflict resolution — still out.

---

## Summary

| Aspect | CIRCLE_PLAN.md | Circle on Tailscale |
|---|---|---|
| Encrypted transport | Hand-rolled WebRTC + Noise XX | WireGuard (Tailscale), audited |
| Relay we run + pay | Rendezvous + TURN ($50–1000/mo) | None (or Headscale if self-hosting) |
| Device identity + revoke | Custom ed25519 + pairing | Tailscale built-in |
| Crypto pen-test | $20–30k | Not needed (no custom crypto) |
| New code we own | ~10k LOC + relay | Go sidecar + thin Rust + small JS |
| Time to MVP | ~10 weeks | ~3–4 weeks |
| New dependency | None | Tailscale / `tsnet` (Go sidecar) |
| Still-open risk | .said plaintext at rest (vault mode) | Same — unchanged |

**Next step:** all five decisions are now made (see Decisions section). Phase 1
(Go sidecar serving a file over the tailnet, Funnel-default) is the first thing
to build when you give the go-ahead. Until then, the splash button stays a
stub — no code touched.

---

## Locked v1 shape (post-decision)

One-click, zero-install is the north star. Concretely:

1. **Host:** `said serve willie.said` → launches the Go sidecar, which joins
   Tailscale (zero-config control plane), enables **Funnel** by default, and
   serves BOTH said.app and the file at one capability URL
   (`https://<host>.ts.net/c/<token>`). `--tailnet-only` flips off Funnel for
   privacy-maximalists.
2. **Client:** opens that one URL → gets the app + the file, no install, no
   CORS. First access prompts the host user to approve ("Allow this device?").
3. **Encrypted at rest:** the served `.said` is vault-mode (passphrase)
   encrypted on disk; Funnel/Tailscale encrypts it in transit.
4. **Revoke:** forget the token (host tray/CLI) and/or revoke the device in
   Tailscale's console.

Simplest possible: the person you share with clicks one link. Everything else
(Tailscale, Funnel, tokens, consent) is invisible to them.

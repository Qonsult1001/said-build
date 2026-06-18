# Skill-Pack Linking & Trust — download, discover, verify, mount

**Status:** design agreed (2026-06-18). Defines how a `.said` skill pack downloaded
from the shop is auto-discovered and **linked** by the orchestration layer — "drop it in
the folder and it works" — while only an **approved, untampered** `.said` is ever mounted.
Builds on [17-skills-and-skill-packs.md](17-skills-and-skill-packs.md) (the skill model)
and reuses the existing header / MODE / AUDT / BLAKE3 infrastructure — no new file type.

## Principle

The orchestrator is an **orchestration/resolver layer**: it mounts a writable PRIMARY
brain plus any number of read-only SKILL PACKS, discovered from a folder, and federates
their learnings into one recall. A pack is just a normal, valid `.said` whose frames are
verified coding-fixes — so it aligns with how *every* `.said` is created. Nothing about a
pack is a bespoke format.

## Three gates, in order (only an approved pack mounts)

### Gate 1 — FORMAT validity (already enforced by `SaidFile::open`)
Every `.said` begins with the 72-byte v7_1 header ([2.1](02-file-format/2.1-v7_1-header.md)).
`SaidFile::open` already rejects anything that isn't a real, intact `.said`:
- `magic != "SAID"` → "Not a .said file (bad magic)"
- `version > 7` → "Unsupported .said version"
- CRC32 mismatch → "corrupted"
- per-section magic mismatch at each offset → reject

So a random/corrupt/foreign file dropped in the skills folder simply **fails to open and
is skipped** (logged). The header *is* the format-approval gate — consistent across every
`.said` ever written, because there is one header writer.

### Gate 2 — IDENTITY (it's a skill pack, not something else)
- **MODE section** ([2.2](02-file-format/2.2-sections.md), [row-37](05-features/row-37-brain-mode.md)):
  a pack is `Portable` (embeds content, self-contained, offline) — required for a
  downloadable pack. Enterprise (pointer-only) packs are refused for mounting (they
  reference content not present on the downloader's disk).
- **Frame shape (FTOC)**: a pack's learnings are **Procedural-pillar coding-fix frames** —
  the exact frames `learn-fix` / the orchestrator's LEARN step write. The layer mounts the
  coding-fix frames into recall; non-coding frames are ignored. No special "pack type"
  flag is needed — identity is the content shape, which is the same everywhere.

### Gate 3 — TRUST / PROVENANCE (signed + approved — REUSES the Hub spec)
Format-valid ≠ trusted. **This is already specified in [row-52-hub.md](05-features/row-52-hub.md)**
— a skill pack is a Hub artifact and inherits the Hub's trust model wholesale; doc 18 just
applies it to the orchestrator's mount path. Do NOT invent a parallel scheme. The Hub spec
provides:
- **`BrainMode::Locked`** — engine-level read-only mode set at publish, cannot be flipped.
  A sold/published pack is `Locked` → it is the "approved, labeled" artifact.
- **Ed25519 signature in a `SIGN` header block** — at publish, the publisher signs
  `BLAKE3(FTOC || DICT || BLKT || data_section)`; the block stores `magic + sig + pubkey +
  sig_alg`. On load the consumer recomputes the BLAKE3, verifies the sig with the embedded
  pubkey (offline), and **cross-references the pubkey against the registry's
  `publishers.json` allowlist** (the trust root). Unknown/forged → loads in a degraded/
  refused state; for *mounting as a skill* we **refuse + log** (never mount unverified).
- **Registry** (`hub.said.app`: `registry.json` + `publishers.json`) — the shop catalog +
  the publisher allowlist the client caches.

Trust policy for mounting:
- `strict` (default for `~/.said/skills`): only Locked + signed-by-allowlisted-publisher
  packs mount.
- `allow-unsigned` (opt-in, e.g. `./.said/skills` for packs you built yourself):
  format+identity gates only.

### Gate 4 — ENTITLEMENT (sold / encrypted packs — the commercial layer)
For packs we **SELL** (e.g. `apply.said`), signing proves authenticity but does NOT protect
the IP — a signed-but-plaintext pack can be copied and used without buying. So a paid pack
is **encrypted**, and only a licensed buyer can decrypt:
- **Encryption:** the pack's frames are **AES-256-GCM** encrypted (the existing
  `Aes256Gcm` frame encoding / `encryption` feature — [2.3](02-file-format/2.3-frame-layout.md),
  [3.4](03-core-subsystems/3.4-framestore.md)). The registry stores only ciphertext
  ([row-52](05-features/row-52-hub.md): "registry only stores ciphertext"). Without the key
  the frames are unreadable — recall returns nothing.
- **Entitlement / license:** on purchase, the shop issues the buyer a **content key**
  wrapped to the buyer's identity (e.g. the content key encrypted to the buyer's device/
  account key, or a short-lived license token the client exchanges for the content key).
  The key lives in the user's keystore (`~/.said/licenses/`), NOT in the pack file.
- **Mount path:** Gate 1 (format) → Gate 2 (identity, MODE=Locked) → Gate 3 (signature +
  approved publisher) → **Gate 4: resolve the content key from the local license; if
  present + valid, decrypt-on-read and mount; if absent/expired, refuse + log "pack
  <name> requires a license."** Decryption is per-frame at read time (existing
  `aes_gcm::decrypt` path), so an unlicensed machine never holds plaintext.
- **Labeling:** a sold pack is therefore: `Locked` + `SIGN`ed (approved) + `Aes256Gcm`
  (encrypted) — the three marks that make it a sellable, approved, protected artifact.

So `apply.said` for sale = Portable + Locked + signed-by-our-publisher-key + AES-256-GCM
encrypted, distributed via the Hub, decryptable only with a purchased license.

## Discovery — convention over config ("just works")

On startup the orchestration layer resolves a skills search set (first present wins as a
root; multiple roots union):
```
1. --skills <file|dir> ...          explicit (highest precedence; always allowed)
2. $SAID_SKILLS_DIR                  env override
3. <repo-root>/.said/skills/*.said  PROJECT-LOCAL packs — the default for now (in SAID-BUILD)
4. ~/.said/skills/*.said            user-global packs — future production default (shop downloads)
```
For now the canonical location is the **project-root `.said/skills/`** (e.g.
`g:/development/said-build/.said/skills/`), so packs live with the repo while we build.
`~/.said/skills` stays the documented production default (where the shop installer drops
purchased packs) and is wired but secondary until then.

For every `*.said` found: run Gate 1 → 2 → 3. Mount the survivors read-only. Report a
one-line inventory (`mounted: code-js (Portable, 1 coding-fix) ✓`).

**The download flow becomes:**
1. Download `javascript-algorithms.said` from the shop (signed by the shop's publisher key).
2. Drop it in `~/.said/skills/` (the shop/CLI installer does this; or the user does).
3. Next run: discovered → format-valid → Portable coding-fix pack → signature verifies
   against the approved shop key → **mounted read-only, federated into recall.**
4. Zero config. A tampered or unsigned-in-strict-mode file is refused and logged.

## Linking into recall (the resolver)

```
recall(task) =
    top-k from PRIMARY (--brain, RW)
  + top-k from each mounted PACK (RO)
  → merge → dedup (BLAKE3 frame id) → rank by score → overall top-5
  → warm-first inject on strong match (the skill "loads"); else cold + recall-on-fail
PRIMARY wins ties. Gate verifies. learn-on-green writes to PRIMARY ONLY (packs stay RO).
```

## What to build (sequence)

1. **Discovery + multi-brain recall (read-only mount).** ✅ **BUILT (commit 857c684).**
   `said-orchestrate` auto-discovers packs from `--skills` / `$SAID_SKILLS_DIR` /
   `<repo>/.said/skills` / `~/.said/skills`, opens each via `SaidFile::open` (Gate 1
   format check skips junk), mounts read-only, and `recall::best_iterations_federated`
   merges primary + packs (Gate 1 + 2). Writes go to primary only. Proven E2E: empty
   primary + auto-discovered `code-js.said` lifted gpt-oss-20b (recall from the pack,
   0.78–0.94). "Download → drop in folder → works" for local/trusted packs is live.
2. **Gate 3 (signing/approval).** ✅ **BUILT, DETACHED (commits this session).**
   `sca_core::pack_sign` (feature `pack-sign`): Ed25519 sign over `BLAKE3(pack bytes)`,
   written as a `<pack>.said.sig` sidecar; verify-at-mount in `said-orchestrate` against
   the `~/.said/publishers/*.pub` allowlist (`$SAID_PUBLISHERS_DIR` too). Policy: a pack
   WITH a sidecar must verify + be allowlisted or it's REFUSED; a pack WITHOUT a sidecar
   is allowed unless `SAID_SKILLS_STRICT=1`. Proven E2E (sign → verify → allowlist
   accept/reject → tamper detected).
   **⚠️ OUTSTANDING FOR LAUNCH:** fold the signature INTO the file as row-52 §1.2's in-file
   `SIGN` section (no sidecar). Same crypto; the sidecar is the interim, lower-risk form.
   See [12-roadmap.md](12-roadmap.md) "Outstanding for launch". Also pending: `said pack`
   CLI (keygen/sign/verify) and `BrainMode::Locked` at publish.
3. **Gate 4 (sell / encrypt / license) = the commercial layer.** Publish paid packs as
   `Locked + SIGNed + Aes256Gcm`; build the license issuance (shop wraps the content key to
   the buyer) + the client keystore (`~/.said/licenses/`) + decrypt-on-mount. This is the
   piece beyond row-52's "public/curated" Tier-1 — selling `apply.said`.
4. **Shop CLI.** `said skills add <name>` / `list` / `remove` wraps download → verify
   (Gate 3) → resolve license (Gate 4) → place in `~/.said/skills`.

Every gate degrades safe: a pack that fails any gate is skipped/refused, never mounted; the
build/test gate remains the final truth on anything a mounted learning proposes.

## Reuses (no new crypto model — all building blocks exist)
- Header/format gate: [2.1](02-file-format/2.1-v7_1-header.md), `SaidFile::open`.
- MODE identity + `Locked` published mode: [row-37](05-features/row-37-brain-mode.md),
  [row-52-hub.md](05-features/row-52-hub.md).
- Ed25519 `SIGN` block + registry publisher allowlist (approval): [row-52-hub.md](05-features/row-52-hub.md) §1.2–1.3.
- AES-256-GCM per-frame encryption (sold-pack IP protection): [2.3](02-file-format/2.3-frame-layout.md),
  [3.4](03-core-subsystems/3.4-framestore.md).
- BLAKE3-chained AUDT + per-frame BLAKE3: [3.7](03-core-subsystems/3.7-audit-log.md).
- Skill model + write-to-primary-only: [17-skills-and-skill-packs.md](17-skills-and-skill-packs.md).

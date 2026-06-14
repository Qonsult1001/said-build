# Hub — public catalog of curated `.said` brains

Status: **planning** — spec drafted, no implementation yet.
Owner: TBD
Last updated: 2026-05-04

## Why this exists

The user's vision: a "Docker Hub for knowledge." Anyone can publish a curated
`.said` brain — `wiki.said`, `medical.said`, `legal-uk.said`, `python-stdlib.said`,
`react-docs.said`, etc. — and any user can pull them down and instantly
have that knowledge usable in their local agent. No API key, no rate limit,
no per-query cost. Just download once, then ask questions offline forever.

This is genuinely different from a vector database hosted in someone else's
cloud. The brain runs locally; only the bytes need to travel once. It's
also genuinely different from the current `.said` "your private file"
positioning — these brains are public, curated, signed.

The product surface this unlocks:

- **Subject-matter brains**: pull `medical.said` and your local agent
  answers PubMed-grade questions
- **Documentation brains**: pull `react-docs.said`, `next-docs.said`,
  `tailwind-docs.said` and your coding agent has authoritative answers
- **Reference brains**: `legal-uk.said`, `tax-us.said`, `wikipedia.said`
- **Domain brains**: `cooking.said`, `birding.said`, `chess-openings.said`
- **Curated personal collections**: someone's research bibliography,
  a hobby community's accumulated knowledge

## Existing surfaces this builds on

The brain format already supports everything we need at the bytes level:
chunks, lineage, tags, audit, BLAKE3 content addressing, encryption hooks.
The hub story is **policy + distribution** layered on top:

- **A new brain mode** (`Locked`) that engine-level refuses writes
- **Signing** in the file header so users can verify publisher identity
- **A registry** that lists available brains + lets users discover them
- **A catalog format** describing each brain's metadata
- **Client UI** to browse, download, manage

## Tier 1 — minimum viable hub

These are required to ship a working hub at all.

### 1.1 `BrainMode::Locked` — engine-level read-only mode

**Today**: `BrainMode` has `Portable` and `Enterprise`. Both are writable —
Enterprise just refuses content-embedding ingests in favor of pointers.

**Needed**: third variant `Locked`. Every mutating call (`remember`, `delete`,
`edit`, `restore`, `compact`, `dream`, `consolidate`) returns
`Err("brain is locked — published hub artifacts are read-only")`. Set at
brain create time; cannot be flipped after publish.

**Why engine-level not UI-level**: a hostile user can clone the file and
write to it via CLI — UI hiding wouldn't stop that. The mode flag in the
on-disk header makes the engine itself refuse, regardless of who's calling.

**Compliance hook**: this is what makes a hub artifact *immutable in the
hands of the consumer*. Critical for trust.

**Effort**: ~half a day. Add variant, gate every mutator, surface in the
header layout, propagate through wasm/CLI/MCP.

### 1.2 Ed25519 signature in the SAID header

**Today**: brains are unsigned. Anyone can publish a brain claiming to be
"wiki.said" with malicious or wrong content.

**Needed**: at publish time, the publisher signs `BLAKE3(FTOC || DICT ||
BLKT || data_section)` with their ed25519 secret key. Signature stored in
a new `SIGN` section at the end of the file (16-byte magic + 64-byte sig
+ 32-byte pubkey + 4-byte sig_alg = 116 bytes). On load, the consumer:

1. Reads the SIGN section
2. Computes the same hash over current file bytes
3. Verifies sig with the embedded pubkey
4. Cross-references the pubkey against a known publisher allowlist
   (downloaded from the registry)

If verification fails OR pubkey is unknown, the brain loads in
**view-only quarantine mode** (cannot be marked Locked, cannot be added
to user's "trusted hub vaults" list).

**Why store pubkey in the file**: it's self-describing. The user can
verify a brain offline against a list of trusted pubkeys cached locally.
No registry call required for verification.

**Why cross-reference the registry**: the pubkey alone proves *some* key
signed the brain — not that the key belongs to "the official Wikipedia
publisher." The registry is the trust root.

**Compliance hook**: any compliance regime that lets users rely on
third-party data needs proof of provenance. ed25519 is the boring,
correct answer.

**Effort**: 1-2 days. Sign/verify CLI commands + Rust crypto integration
+ registry pubkey allowlist client.

### 1.3 Registry — where to publish, where to discover

**Today**: nothing.

**Needed**: a static-file registry, hosted at `https://hub.said.app/` or
similar. Architecture:

```
hub.said.app/
├── registry.json         # { brains: [ { name, slug, latest_version, ... } ] }
├── publishers.json       # { publishers: [ { name, pubkey, verified, ... } ] }
├── brains/
│   ├── wiki/
│   │   ├── meta.json     # full catalog entry for this brain
│   │   ├── 2026.04.01/   # versioned releases
│   │   │   ├── wiki.said
│   │   │   └── meta.json
│   │   └── 2026.05.01/
│   │       ├── wiki.said
│   │       └── meta.json
│   └── medical/...
```

**Static is the right call for v1**: zero infrastructure, easy to mirror
(GitHub Pages, S3, IPFS, any CDN). A user behind a proxy can fork the
whole registry to a private mirror and point their client at it.

**Future**: mutable registry with publisher accounts, user reviews,
download counts. Treat as a separate product line.

**Effort**: 2 days for the static-site generator + GitHub Action that
validates submissions + publishes the catalog.

### 1.4 Catalog format — meta.json schema

Every hub brain ships with a `meta.json` describing it:

```json
{
  "name": "Wikipedia (English, 2026-05-01)",
  "slug": "wiki",
  "version": "2026.05.01",
  "publisher": {
    "name": "Said Foundation",
    "pubkey": "ed25519:...",
    "verified": true,
    "url": "https://said.app/publishers/foundation"
  },
  "description": "Every English Wikipedia article as of 2026-05-01.",
  "long_description": "Markdown-formatted full description shown in the UI.",
  "language": "en",
  "license": "CC-BY-SA-4.0",
  "topics": ["general-knowledge", "encyclopedia", "reference"],
  "size_bytes": 10737418240,
  "compressed_size_bytes": 2147483648,
  "memory_count": 6800000,
  "blake3": "...",
  "ed25519_signature": "...",
  "released_at": "2026-05-01T00:00:00Z",
  "supersedes": ["wiki/2026.04.01"],
  "min_engine_version": 7,
  "preview_queries": [
    "What is the speed of light in vacuum?",
    "When was the Treaty of Westphalia signed?",
    "Who composed The Magic Flute?"
  ]
}
```

**`preview_queries`** are pre-vetted "this brain answers these well"
examples. The UI shows them as click-to-try in the brain's detail page —
gives users immediate proof the brain is useful.

### 1.5 Client UI — browse, download, manage

**Hub panel** in the WASM UI (Connected veil → Hub tab, or a separate
veil entirely):

- **Browse view**: cards laid out as in [row-43-admin-ui.md](row-43-admin-ui.md)'s
  vault grid — name, publisher, size, topics, "Pull" button
- **Detail view**: long description, preview queries, version history,
  publisher info, signature status
- **Download flow**: shows progress (~2 GB compressed for wiki.said is
  noticeable on slow connections), verifies signature, persists to OPFS
  / IndexedDB / FileSystemFileHandle depending on the storage strategy
  decided in [row-53-storage.md](row-53-storage.md) (separate doc TBD)
- **Manage view**: list of installed hub brains, version, last-updated,
  "check for updates" / "uninstall" actions
- **Auto-update toggle**: per-brain or global "auto-pull when registry
  publishes a new version"

**Indicator on Connected vault cards**: hub brains get a "HUB" badge so
the user sees they're external. Different from `PRIMARY` (which they
own + can write).

**Effort**: 1-2 days for the panels + download progress + signature UI.

## Tier 2 — needed for serious adoption

### 2.1 Differential updates

A 10 GB wiki.said takes ages to redownload monthly. Need delta updates:
publisher generates `wiki/2026.05.01/delta-from-2026.04.01.zst` containing
only the new + changed frames. Client applies delta to the existing
brain. Rough order of magnitude: deltas should be 1-5% the size of the
full brain for monthly cadence.

**Effort**: 3-4 days. New CLI commands `said hub publish-delta` and
`said hub apply-delta`.

### 2.2 Federated discovery

User can configure additional registries beyond the default. e.g. a
company hosts an internal registry with proprietary brains:

```
$ said hub registry add company https://hub.company.internal/
$ said hub list                  # shows public + company
$ said hub pull company:knowledge-base
```

Standard pattern from npm/cargo/docker: lets enterprises mirror.

**Effort**: 1 day. Just a list of registry URLs in client config.

### 2.3 Privacy-respecting download counts

Publishers want to know "is anyone using my brain?" Users don't want a
panopticon. Compromise: anonymous opt-in telemetry. Client periodically
pings registry with `{ brain_slug: "wiki", version: "2026.05.01" }` —
no user id, no IP correlation, no query content. Per-IP rate-limited
on the registry side.

**Effort**: half a day client + 2 days registry-side aggregation.

### 2.4 Curated topic lists

Registry exposes `/topics/medical.json` returning ranked brains tagged
`medical`. Users browse by topic, not just by keyword. Same pattern as
npm's `/-/search?keywords=foo` but specifically curated.

**Effort**: 1 day. Static-site generator extension.

### 2.5 Multi-language search

A user with `wiki-en.said` + `wiki-de.said` + `wiki-ja.said` should be
able to ask in any language and get hits from all three. Currently the
WASM agent uses a single embedding model; multi-language requires either
a multilingual embedding (M3-Embedding, BGE-M3) or per-vault model
swapping.

**Effort**: 1 week. Material engine work; treat as a separate roadmap
item `row-53-multilang-search.md`.

## Tier 3 — premium / pitchable differentiators

### 3.1 Personal brain publishing

User has their personal `journal.said` and wants to share it with friends
or family. `said hub publish --private journal.said` uploads it to a
private space on the registry, sharable via a magic link or invite list.
End-to-end encrypted; the registry only stores ciphertext.

### 3.2 Verified-author brains with bio + portfolio

Author claims a name + GitHub identity, registry verifies (DNS TXT or
GitHub gist), brains by verified authors get a checkmark. Same pattern
as Twitter blue-check / npm verified maintainers.

### 3.3 Brain composition / overlays

User pulls `wiki.said` and `wiki.said.community-corrections.overlay`
which adds a small set of frames on top of the base wiki, applied at
load time. Lets community-curated patches layer on official releases
without forcing a fork.

### 3.4 Signed read receipts

For regulated industries: when a brain is consulted by an agent, the
brain returns a signed "this content was retrieved at time T from
brain B version V" receipt. Critical for audit trails of LLM-driven
decisions.

### 3.5 Streaming brains

Brains too big to download in full (10 TB encyclopedia of all books?)
get streamed: client downloads only the BRAN + index + dictionary up
front, frames are fetched on demand. Range-request friendly. Trades
offline-everywhere for unbounded brain size.

## Suggested sequencing

| Phase | Tickets | Effort | Outcome |
|------|---------|--------|---------|
| **A — Spec lock-in** | This doc + decisions on registry host, signing scheme, catalog schema | ~3 days | Frozen API surface for all parties |
| **B — Engine baseline** | 1.1 Locked mode, 1.2 ed25519 sign/verify | ~3 days | Brain format ready to publish |
| **C — Static registry** | 1.3 registry repo, 1.4 catalog format, GitHub Action | ~3 days | Anyone can publish via PR |
| **D — Client UI** | 1.5 Hub panel, browse, download, signature verify | ~2 days | First wiki.said pull works end-to-end |
| **E — First brain** | Ingest English Wikipedia → `wiki.said` v2026.05.01 | ~1 week ops | Live demo: pull wiki, ask "treaty of westphalia", get answer offline |
| **F — Polish** | 2.1 deltas, 2.2 federation, 2.3 telemetry, 2.4 topics | ~2 weeks | Ready for public launch |

Total: ~5 weeks of focused work for hub MVP. Phase A+B alone is ~6 days
and gives us a defensible "we have a hub story" pitch even before the
registry exists.

## Open questions for product/legal

1. **Hosting**: hub.said.app on Cloudflare Pages? GitHub Pages + Cloudflare
   in front? Self-hosted on said.app domain? Cost vs. control.
2. **Trust root**: who verifies "this is the real Wikipedia publisher"?
   Manual review by said.app team? Trust-by-domain (verify pubkey via
   DNS TXT on `wikipedia.org`)? GitHub identity?
3. **Licensing**: what licenses are allowed in the registry? CC-BY-SA?
   Public domain only? Commercial brains via paid hub?
4. **Content moderation**: who takes down a brain that's discovered to
   contain harmful content? Said Foundation? Community-reportable?
5. **Brain size limits**: cap at 10 GB per brain? 100 GB? Use deltas
   when over a threshold?
6. **Naming collisions**: `wiki` is generic. Does Said Foundation own
   the namespace, or do publishers get scoped slugs (`said/wiki`,
   `wikimedia/wiki`)?

## Out of scope here

- **Storage strategy** (OPFS vs FileSystemFileHandle vs hybrid) — separate
  doc. Hub brains land via whichever local strategy the client uses.
- **LLM integration** — hub brains feed the same `ask_fused` retrieval
  as user vaults; no new LLM plumbing needed beyond fan-out (already
  planned in Connected panel γ-2).
- **Encryption at rest** — independent feature. Hub brains may or may
  not be encrypted; trust comes from signature, not cipher.

## References

- [row-37-brain-mode.md](row-37-brain-mode.md) — current `BrainMode`
  enum (Portable / Enterprise) — Locked is the third variant.
- [row-42-admin.md](row-42-admin.md) — admin surface (export, rotate,
  legal hold) — hub publishers use a similar surface plus `hub publish`.
- [row-43-admin-ui.md](row-43-admin-ui.md) — Connected panel where the
  Hub tab will live.
- [row-51-enterprise-audit.md](row-51-enterprise-audit.md) — enterprise
  audit roadmap; signed read receipts are a Tier 3 hub item that
  borrows from there.
- [02-file-format/](../02-file-format/) — current `.said` byte layout;
  the SIGN section needs to be added.

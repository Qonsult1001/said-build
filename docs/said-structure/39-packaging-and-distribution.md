# 39 — Packaging & distribution (installers for Linux / macOS / Windows)

How a `.said` build becomes an installable product on someone else's machine. The rule: **GitHub Actions
builds and packages everything on a tag push** — no local packaging toolchains, no per-developer setup.
You push `vX.Y.Z`, CI cross-compiles + packages + publishes a GitHub Release.

## The pipeline (`.github/workflows/build-binaries.yml`)

One workflow, two jobs:

1. **`build`** — a matrix of **4 bundles × 3 OSes = 12 jobs**. Each job:
   - cross-compiles `said` (CLI) + `said-mcp` for its `{bundle, os}` (`--no-default-features --features
     <bundle>`; the encoder is embedded via `embed-model`, so every binary is self-contained);
   - **packages a native installer** for its OS (see below);
   - uploads `dist/*` (raw binaries + the installer) as an artifact.
2. **`release`** (only on a `v*` tag) — downloads all artifacts, zips each bundle-target, lifts the
   native installers + the one-line install scripts to the Release root, and publishes them with
   `softprops/action-gh-release`.

Targets (the matrix, authoritative in the workflow): `linux-x64` (ubuntu), `windows-x64`, `macos-arm64`.

## The three install paths a user gets

Ranked easiest-first (documented for users in `production/brain/cli/doc/how-to-install-said.md`):

| Path | Linux / macOS | Windows |
|---|---|---|
| **A. One-line** | `curl -fsSL …/install.sh \| sh` | `irm …/install.ps1 \| iex` |
| **B. Native installer** | `.deb` (`sudo apt install ./said_*.deb`) · `.pkg` (open) | `said-setup-*.exe` (double-click) |
| **C. Manual zip** | download `said-brain-*-x64.zip`, unzip, add to PATH | same |

All three resolve to the SAME GitHub Release the workflow produces. The install scripts and native
packages both just place `said` + `said-mcp` on PATH — no other difference.

## Packaging assets (`packaging/`)

Small, self-contained, invoked by the workflow:

- **`install.sh`** — POSIX one-liner: detects OS/arch → downloads the matching Release zip → installs to
  `~/.local/bin` → PATH hint. `SAID_BUNDLE=coding` overrides the tier (default `brain`).
- **`install.ps1`** — Windows equivalent: installs to `%LOCALAPPDATA%\Programs\said`, adds to the USER
  PATH, no admin.
- **`build-deb.sh`** + **`deb/control.template`** — builds `said_<ver>_amd64.deb` with `dpkg-deb`
  (installs to `/usr/bin`, no PATH edit needed). Run on the Linux matrix job.
- **`build-macos-pkg.sh`** — builds `said-<ver>-arm64.pkg` with `pkgbuild` (installs to
  `/usr/local/bin`). Run on the macOS matrix job.
- **`windows/said.iss`** — Inno Setup script: builds `said-setup-<ver>-x64.exe`, per-user install,
  optional "add to PATH" task, registers an uninstaller. Compiled with `iscc` on the Windows matrix job.

## Cutting a release

```bash
# from a clean main with the version bumped in crates/said-cli/Cargo.toml
git tag v0.11.1
git push origin v0.11.1
# → GitHub builds all 12 targets, packages the brain installers, and publishes the Release.
```

`workflow_dispatch` also runs the build (artifacts only, no Release) for testing without tagging.

## Deferred: signing & notarisation (needs certs — CIRCLE_PLAN distribution step)

The packages CI produces today are **UNSIGNED**. That's fine for the binaries themselves (they're
self-contained), but the OS trust prompts differ:

- **macOS** — an unsigned downloaded `.pkg` triggers Gatekeeper; the user must right-click → **Open**
  once. The real fix (per [CIRCLE_PLAN.md](CIRCLE_PLAN.md) §Distribution) is an **Apple Developer ID
  signature + notarisation** (`.dmg`, notarised) → then a plain double-click works and Homebrew cask
  distribution becomes possible.
- **Windows** — an unsigned `said-setup-*.exe` shows SmartScreen "unknown publisher". The fix is an
  **Authenticode (EV) code-signing cert** applied to the installer in CI.
- **Linux** — `.deb` is fine unsigned for direct install; a signed `AppImage` + `.rpm` + an apt repo are
  the follow-ups CIRCLE_PLAN lists.

These are a **credentials/secrets task**, not an engineering one — add the certs to GitHub Actions
secrets and a signing step to the workflow when the certs exist. Until then, the one-line install script
(Option A) is the smoothest path because it bypasses the download-a-binary Gatekeeper/SmartScreen prompt
(the script, not a binary, is what the user runs).

## See also

- [35-production-build.md](35-production-build.md) — the native-fast *speed* compile (LTO, target-cpu).
  Distinct from packaging: #35 is "make it fast", #39 is "make it installable".
- [CIRCLE_PLAN.md](CIRCLE_PLAN.md) §Distribution — the full signed/notarised/auto-update MVP plan.
- `production/brain/cli/doc/how-to-install-said.md` — the user-facing install guide (the three options above).

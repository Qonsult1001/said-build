# `said edit` — Surgical-Edit Feature Spec

**Purpose:** Give `.said` a first-class way to apply a *precise, anchored* code change — insert,
replace, or delete a small region — **instead of rewriting whole files**. This is the durable fix for
the failure where an LLM was told to return "full new file content" and silently deleted the rest of
`Program.cs` (157 lines → 27), gutting the app. `.said` already knows every symbol's exact
`file:start_line–end_line` (it returns this from `sym`), so it is the right place to own surgical edits.

Target repo: `SAID-ECHO/crates/said-cli` (the `said` binary). Add a new subcommand `edit`. Must build
for **Linux x86-64** (the Advisory API container runs Linux) and ship in `dist-binaries/said-linux-x64/said`.

---

## 1. CLI surface

```
said edit --path <Advisory.said> --file <relative/path> <MODE> [anchor args] [--content <text> | --content-file <f>] [--json] [--dry-run]
```

`--file` is the repo-relative path of the source file to change (e.g. `src/Advisory.Api/Program.cs`).
The edit is applied to the **file on disk** (in the caller's working dir / clone), NOT inside the
`.said` frame store — `.said` is used to *resolve the anchor location*, the bytes are written to the
real file. (After editing, the caller can `said reindex <file>` to refresh the brain.)

### Modes (one required)

| Mode | Args | Effect |
|------|------|--------|
| `insert-after-symbol`  | `--symbol <name>`              | Insert `--content` on the line(s) **after** the symbol's `end_line`. |
| `insert-before-symbol` | `--symbol <name>`              | Insert **before** the symbol's `start_line`. |
| `replace-symbol`       | `--symbol <name>`              | Replace the symbol's whole `start_line..=end_line` range with `--content`. |
| `insert-after-text`    | `--anchor <exact substring>`   | Insert `--content` after the **first line** containing the exact anchor text. |
| `insert-before-text`   | `--anchor <exact substring>`   | Insert before the first line containing the anchor. |
| `replace-text`         | `--anchor <exact substring>`   | Replace **only the matched substring** (first occurrence) with `--content`. |
| `delete-symbol`        | `--symbol <name>`              | Remove the symbol's `start_line..=end_line` range. |

`--content` (inline) or `--content-file <path>` (read from file — preferred for multi-line code).

### Resolution rules
- For `--symbol`, resolve via the **same lookup `said sym` uses** (exact → prefix → case-insensitive
  contains) **scoped to `--file`** (so `Program` resolves to the symbol in that file, not elsewhere).
  Use the symbol's `start_line` / `end_line` from the index. **Error if 0 or >1 matches** in that file
  (don't guess). For multi-result, require an explicit disambiguator or fail with the candidate list.
- For `--anchor`, match the **exact substring** in the on-disk file. **Error if 0 matches.** Use the
  first match only; report the line number used.
- Preserve the file's existing newline style (LF vs CRLF) and indentation context.

### Output
- `--json`: `{ "ok": true, "file": "...", "mode": "...", "anchor": "...", "applied_at_line": N, "lines_added": A, "lines_removed": R }`
  On failure: `{ "ok": false, "error": "symbol 'Program' not found in src/.../Program.cs" }` (non-zero exit).
- `--dry-run`: resolve + compute the change and print the diff / target line, but **do not write**. Used
  by callers to preview before committing.

---

## 2. Safety requirements (these are the point of the feature)

1. **Never replace the whole file.** There is no "replace file" mode. The largest single op is
   `replace-symbol`, bounded to that symbol's line range.
2. **Anchor must exist.** If the symbol/anchor can't be resolved uniquely, **fail with a clear error
   and write nothing** — never fall back to appending or rewriting.
3. **Bounded change size.** Reject (error) if `replace-symbol`/`replace-text` would remove more than a
   configurable max (default 200 lines) unless `--allow-large` is passed — a guard against a prompt that
   accidentally selects a huge range.
4. **Idempotent-friendly:** `insert-after-text` with content already present on the next line should be
   a no-op-with-warning (optional, nice-to-have), so re-runs don't duplicate.
5. **Atomic write:** write to a temp file and rename, so a crash can't leave a half-written source file.
6. **Path safety:** reject `--file` containing `..` or absolute paths outside the working dir.

---

## 3. How Advisory will call it (the consumer)

`GroqCycle` (C#, `src/Advisory.Api/Agents/GroqCycle.cs`) currently asks Groq for *full file content* and
writes whole files — **that is what gutted Program.cs**. After `said edit` exists, the flow becomes:

1. Recall context (already works): `said grep MapGet`, `said sym <X>` for the anchor location.
2. Ask Groq for a **structured surgical change**, not a file. New JSON contract:
   ```json
   {
     "summary": "Add GET /api/host",
     "edits": [
       { "file": "src/Advisory.Api/Program.cs",
         "mode": "insert-after-text",
         "anchor": "app.MapGet(\"/api/pid\"",
         "content": "app.MapGet(\"/api/host\", () => Results.Ok(new { host = Environment.MachineName })).AllowAnonymous();" },
       { "file": "tests/Advisory.Tests/HealthTests.cs",
         "mode": "insert-before-text",
         "anchor": "// --- end of endpoint tests ---",
         "content": "[Fact]\npublic async Task Host_returns_200() { ... }" }
     ]
   }
   ```
3. For each edit, run:
   ```
   said edit --path <clone>/Advisory.said --file <edit.file> <edit.mode> \
     (--symbol <s> | --anchor <a>) --content-file <tmp> --json
   ```
   in the **cloned repo** working dir. If any edit returns `ok:false`, abort the whole change set
   (don't open a partial PR).
4. `said reindex <file>` for each changed file (optional, keeps the clone's brain fresh).
5. Then the existing build → test → commit → push → PR steps run unchanged.

**Net effect:** Groq can only *insert/replace at a named anchor* — it physically cannot delete the rest
of a file, because there is no full-file-write path anymore.

---

## 4. Build + delivery (what to hand back to Advisory)

- Build for Linux x86-64 with the existing feature set the brain needs:
  ```
  cargo build --release -p said-cli --features "code,docs"   # target/release/said
  ```
  (The Advisory brain was rebuilt with this binary; `said sym/grep/get` already return correct ranges,
  so the index already has what `edit` needs to resolve anchors.)
- Deliver the new Linux binary to `G:\development\said-build\dist-binaries\said-linux-x64\said`
  (same path the current one came from). Advisory bakes it into the API image at
  `tools/said/said-linux` → `/app/said` (see `Dockerfile`).
- Bump `said --version` so we can confirm the new binary is live in the container.

---

## 5. Acceptance tests (prove it before shipping)

Run against `Advisory.said` + a checkout of `src/Advisory.Api/Program.cs`:

1. `said edit --file src/Advisory.Api/Program.cs insert-after-text --anchor 'app.MapGet("/api/pid"' --content '<one MapGet line>' --dry-run`
   → reports the correct target line, **does not** modify the file.
2. Same without `--dry-run` → Program.cs gains exactly the new line; **all other lines unchanged**
   (`git diff` shows a single-line insertion, line count 161 → 162, NOT 161 → 27).
3. `said edit ... insert-after-symbol --symbol DoesNotExist` → `ok:false`, non-zero exit, file untouched.
4. `replace-symbol` on a 5-line method → only those 5 lines change.
5. `--anchor` matching nothing → `ok:false`, file untouched.

If test 2's `git diff` is anything other than a clean single-region change, the feature isn't done.

---

## 6. Why this matters (context for whoever builds it)

The Advisory mutation engine lets agents (Claude/Groq/Cursor) open PRs that fix tickets. The
container-native Groq path worked end-to-end (ticket → plan → approve → PR → merge) but used full-file
rewrites, which **silently deleted the rest of `Program.cs` and broke `main` for several PRs** before it
was caught. `said edit` makes destructive whole-file writes *impossible* by construction, which is the
right guarantee for an autonomous code-changer. It's also reusable by any other `.said` consumer (the
Claude/Cursor worker cycles, IDE tools, future agents).

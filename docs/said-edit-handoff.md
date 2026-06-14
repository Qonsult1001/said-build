# `said edit` — Handoff for the Linux box / Advisory

**Date:** 2026-06-14 · **Version:** `said 0.7.0` (both `said` CLI and `said-mcp`)
**Artifact:** `said-full-linux-x64`

> **Live-binary check:** `said --version` must print **`said 0.7.0`**. If it
> prints anything lower, the old binary is still baked in — rebuild the image.

### New in 0.7.0 — C# class/ctor disambiguation (resolves the live blocker)

In C# every class shares its name with its constructor, so
`append-into-symbol --symbol <Class>` was ambiguous for essentially every real
class. Fixed three ways:

| Fix | Behavior |
|-----|----------|
| **`--line <N>`** | When a `--symbol` matches multiple spans in `--file`, `--line N` selects the span starting at line N. The ambiguity error and `--explain` both list the candidate start lines. |
| **`append-into-symbol` defaults to the largest span** | When ambiguous, it picks the **enclosing class** (not the 1-line constructor) automatically — so "add a member to ClassName" works with **no extra args**. `--line` overrides. |
| **`--explain` returns `line` + `kind`** | Each `valid_anchors` entry now includes `"line": 10, "kind": "class_declaration"`, so the menu is directly actionable: read it, issue `append-into-symbol --symbol X --line 10` (or just rely on the largest-span default). |
| **`--help` lists all 11 modes** | `append-into-symbol` + the three `*-context` modes are now in the `--help` Modes line and usage. |

Verified end-to-end on a real C# class+constructor name clash: `append-into-symbol`
with no `--line` lands the member at class scope (valid C#); explicit `--line`
selects the exact span; `--explain` returns the actionable line/kind.

This is the durable fix for the production failure where the Groq cycle did a
**full-file rewrite** and silently deleted most of `Program.cs` (161 → 27 lines),
breaking `main` for several PRs. `said edit` makes that **impossible by
construction** — there is no whole-file-write path.

---

## 1. What changed in this build

| Change | What it means for you |
|--------|------------------------|
| **New `said edit` subcommand** | Surgical, anchored insert/replace/delete on a source file. No mode can rewrite a whole file. |
| **New MCP `edit` tool** | Same capability exposed to MCP clients (Claude/Groq via the MCP server). Identical behavior + safety. |
| **AST chunker bug fixed** | Short functions (<3-line body) used to vanish from the symbol index and the previous symbol's range over-extended — which made `replace-symbol` eat the next function. Now every named definition has an exact range. Makes symbol-mode edits safe. |
| **Version** | `said --version` → `said 0.7.0` (current). Use this to confirm the new binary is live in the container. |
| **Feature bundles** | Binaries are now built as bundles. The one you want is **`full`** (= code + docs + OCR + LSP, with the encoder baked in). |

### New in 0.3.0 — world-class safety upgrades

| Upgrade | What it gives you |
|---------|-------------------|
| **Context anchors** | New modes `insert-after-context` / `insert-before-context` / `replace-context`. The `--anchor`/`anchor` is a (possibly multi-line) block that must occur **exactly once** — errors on 0 (missing) or >1 (ambiguous). Use this when a short string repeats and a plain text anchor would be ambiguous. **Most robust mode for autonomous edits.** |
| **Post-edit syntax check** | After every edit, the file is re-parsed with tree-sitter; if the edit would leave it with a syntax error (unbalanced braces/parens, malformed code), the edit is **rejected and the file is left unchanged**. On by default for code files. CLI escape hatch: `--no-verify`. Unknown file types skip the check (can't verify → don't block). |
| **Transactional edit sets** | When applying multiple edits, if any one fails the whole set fails — no half-applied change reaches disk. |

**Net effect for an autonomous agent:** it cannot delete the rest of a file, cannot land an edit in the wrong place when text repeats, and cannot leave the file un-compilable. Three independent guarantees.

### New in 0.4.0 — recall-correctness + transactional edits

| Upgrade | What it gives you |
|---------|-------------------|
| **Universal parse-check** | The post-edit syntax gate now covers **all 24 bundled languages** (Rust, Python, JS, TS, Go, Java, C#, C, C++, Ruby, PHP, Kotlin, Swift, Lua, Markdown, JSON, YAML, TOML, XML, Bash, HCL/Terraform, PowerShell, Perl) — not just the original 7. |
| **Anchor-drift detection** | Before a symbol-mode edit, the file on disk must still match what the brain indexed; if it changed since the last `init`/`reindex`, the edit is **refused** with "run `said reindex`" rather than editing against a stale brain. Whitespace-insensitive (formatting noise isn't drift). |
| **Symbol-span correctness** | `replace-symbol`/`delete-symbol` now use the brain's stored content as the authoritative span (the symbol index `end_line` was off-by-one on the closing brace), so the **whole construct incl. its closing `}` is replaced cleanly** — no orphan brace. |
| **`edit_batch` (MCP)** | Apply a SET of edits **all-or-nothing**. Every edit is resolved+applied+syntax-verified in memory first; files are written only if every edit succeeds. If any fails, nothing is written — no half-applied change set on disk. Use for multi-file changes (endpoint + its test) so they land together or not at all. |

> **For the Groq cycle:** prefer the MCP `edit_batch` tool for a multi-file change set — it gives the cross-file all-or-nothing guarantee the loop-and-abort approach lacked. Still: this catches *parse* breakage, not *type/compile* errors — the in-clone `dotnet build`/`test` (SDK in the container) remains the required backstop before a PR is mergeable. Do not auto-merge drafts that haven't built in-clone.

### New in 0.5.0 — scope-aware editing + structured repair menus

| Upgrade | What it gives you |
|---------|-------------------|
| **`append-into-symbol` mode** | Insert a new member at the END of a named scope's body (just before its closing brace). "Add a test method to this class" always lands at class scope — it **cannot** nest inside an existing method. This is the by-construction fix for the CS0106 ("'public' not valid here") class of failures. |
| **Structured repair menus** | When an edit is rejected (syntax-break), the error now carries a `valid_anchors` array of **copy-paste-ready** `said edit` argument sets, computed live from the AST at the landing line. Model-agnostic, all 24 languages. A repair loop parses these and feeds the LLM a one-shot correction instead of prose. |

Rejection payload shape (CLI `--json` and MCP both):

```json
{
  "ok": false,
  "error": "edit would leave cs with a syntax error — edit rejected, file unchanged",
  "valid_anchors": [
    { "mode": "append-into-symbol", "symbol": "HealthTests",
      "note": "add a sibling member at the end of `HealthTests`'s body (class scope)" },
    { "mode": "insert-after-symbol", "symbol": "Pid_test",
      "note": "insert after `Pid_test` (same scope as that method)" }
  ]
}
```

**For the repair loop:** on a rejection, read `valid_anchors`, pick one (the
`append-into-symbol` entry is the safe default for "add a member"), and re-issue
the edit with those exact args. One-shot correction, no prose to interpret. The
menu is derived from the real AST, so it works for any LLM and any language.

### New in 0.6.0 — pre-validate + ergonomics (integration feedback)

| Upgrade | What it gives you |
|---------|-------------------|
| **`said edit --explain`** | Pre-validate WITHOUT editing: returns the `valid_anchors` menu for a `--symbol` or `--anchor` up front, so the LLM picks the right move on the FIRST try (not after a failed edit). `mode` is optional with `--explain`. CLI and MCP (`"explain": true`). |
| **Auto-indent** | `append-into-symbol` now indents the inserted member to match the scope's existing body indentation — a method added to a class lands at the siblings' indent, not column 0. |

**`--explain` payload** (CLI `--json`):

```json
{ "ok": true, "explain": true, "file": "src/HealthTests.cs", "at_line": 1,
  "valid_anchors": [
    { "mode": "append-into-symbol", "symbol": "HealthTests",
      "note": "add a sibling member at the end of `HealthTests`'s body (class scope)" } ] }
```

**Exit-code contract (confirmed):**
- Success (`"ok": true`) → exit **0**.
- Any failure including a syntax-reject-with-`valid_anchors` (`"ok": false`) → exit **non-zero (1)**.
- `--explain` always succeeds (exit 0) — it's a query, not an edit.

So: branch your repair loop on the exit code (non-zero = failed), then read
`valid_anchors` from the JSON to construct the one-shot retry.

**Ambiguous symbols (confirmed):** `--symbol` is resolved **scoped to `--file`**.
If the same symbol name exists in multiple files, `--file` disambiguates; if it
appears more than once *within the same file*, the edit errors (never guesses).

### Known boundary (honest)

`said edit` syntax-verify catches **structural/parse** breakage (the PR-#93 class). It does **not** catch **type/semantic** errors (wrong type, missing `using`, undefined symbol) — those parse fine but don't compile. A real compiler (dotnet/cargo/tsc) is still needed for that, and that belongs in the cycle's in-clone build step, not in `.said`.

---

## 2. Install on the Linux box

1. Download the **`said-full-linux-x64`** artifact from the CI run.
2. It contains two binaries:
   - `said`      → CLI (this is what has `said edit`)
   - `said-mcp`  → MCP server (also has the `edit` tool)
3. Place them where the container expects (per the existing Dockerfile):
   ```
   tools/said/said-linux  →  /app/said
   ```
4. Confirm the new binary is live:
   ```bash
   /app/said --version          # must print: said 0.7.0
   ```
   If it says anything below `0.7.0`, the old binary is still baked in — rebuild the image.

> **Shell note (carried over from build.md):** under Git Bash, prefix
> `docker exec` calls with `MSYS_NO_PATHCONV=1` or `/app/said` gets rewritten to
> `C:/Program Files/Git/app/said`. Not needed from PowerShell/WSL/native Linux.

---

## 3. How to use `said edit`

```
said edit --path <Advisory.said> --file <relative/path> <MODE> \
          [--symbol <name> | --anchor <exact text>] \
          [--content <text> | --content-file <f>] [--json] [--dry-run] [--allow-large] [--no-verify]

# Pre-validate (no edit) — get the valid moves up front:
said edit --path <Advisory.said> --file <relative/path> --explain (--symbol <name> | --anchor <text>) --json
```

`.said` resolves *where* (a symbol's exact line range, or the first line
containing an exact substring); the bytes are written to the **real file on
disk** in the caller's working dir (the clone), NOT into the `.said` store.
`--symbol` is always resolved **scoped to `--file`** (errors on >1 match in that
file; `--file` disambiguates a name shared across files).

### Modes (exact CLI arg form)

| Mode | Required arg | Effect |
|------|--------------|--------|
| `insert-after-symbol`   | `--symbol <name>` | Insert after the symbol's last line. |
| `insert-before-symbol`  | `--symbol <name>` | Insert before the symbol's first line. |
| `replace-symbol`        | `--symbol <name>` | Replace the symbol's whole line range. |
| `delete-symbol`         | `--symbol <name>` | Remove the symbol's line range (no `--content`). |
| `append-into-symbol`    | `--symbol <name>` | **Insert a new member at the END of the scope's body** (before its closing brace), auto-indented to match siblings. Use for "add a method/test to this class" — cannot nest inside another method. |
| `insert-after-text`     | `--anchor <text>` | Insert after the first line containing the exact text. |
| `insert-before-text`    | `--anchor <text>` | Insert before that line. |
| `replace-text`          | `--anchor <text>` | Replace only the matched substring (first occurrence). |
| `insert-after-context`  | `--anchor <block>` | `--anchor` is a (multi-line) block that must occur **exactly once**; insert after it. Errors if 0 or >1. |
| `insert-before-context` | `--anchor <block>` | Same uniqueness rule; insert before the block. |
| `replace-context`       | `--anchor <block>` | Replace the unique block. |

Exact example (the common "add a test" case):
```
said edit --path Advisory.said --file tests/HealthTests.cs \
  append-into-symbol --symbol HealthTests --content-file new_test.txt --json
```

### Output & exit codes

- `--json` success: `{"ok":true,"file","mode","anchor","applied_at_line","lines_added","lines_removed","dry_run"}` → **exit 0**.
- `--json` failure (incl. syntax-reject): `{"ok":false,"error":"...","valid_anchors":[...]}` → **exit non-zero (1)**, file untouched.
- `--explain`: `{"ok":true,"explain":true,"file","at_line","valid_anchors":[...]}` → **exit 0**, no write.
- `--dry-run`: resolve + report the target line, but **write nothing**.

Branch the repair loop on the **exit code** (non-zero = failed), then read
`valid_anchors` from the JSON for a one-shot retry.

### Safety guarantees (the point of the feature)

1. **No whole-file write** — largest op is `replace-symbol`, bounded to that symbol's range.
2. **Anchor must resolve uniquely** — 0 or >1 matches → error, nothing written.
3. **Bounded change size** — replace/delete > 200 lines is rejected unless `--allow-large`.
4. **Atomic write** — temp file + rename, so a crash can't leave a half-written file.
5. **Path safety** — `--file` must be relative; no `..`, no absolute paths.
6. **Newline preserved** — LF/CRLF and trailing newline kept intact.

---

## 4. How the Groq cycle should use it (the change to GroqCycle.cs)

**Stop asking the LLM for full file content.** Ask for a structured edit set
using the **0.6.0 recommended modes** (see §4 recommendation below), and apply
them transactionally. Example — add an endpoint + its test:

```json
{
  "summary": "Add GET /api/host",
  "edits": [
    { "file": "src/Advisory.Api/Program.cs",
      "mode": "insert-after-text",
      "anchor": "app.MapGet(\"/api/pid\", () => Results.Ok(pid));",
      "content": "app.MapGet(\"/api/host\", () => Results.Ok(new { host = Environment.MachineName })).AllowAnonymous();" },
    { "file": "tests/Advisory.Tests/HealthTests.cs",
      "mode": "append-into-symbol",
      "symbol": "HealthTests",
      "content": "[Fact]\npublic async Task Host_returns_200() { /* ... */ }" }
  ]
}
```

Notes on the example:
- **Endpoint = `insert-after-text`** — fine for a single, clearly-unique line.
  The `anchor` must be a **complete line** (the full `app.MapGet(...);` statement,
  not a prefix) so the new line lands *after* it, never inside it. If the line
  might repeat, use `insert-after-context` with a unique multi-line block.
- **Test = `append-into-symbol --symbol HealthTests`** — adds the `[Fact]` method
  at class scope, auto-indented, so it can never nest inside another method
  (the CS0106 class of failures is impossible here by construction).

Apply the set **all-or-nothing**. Two equivalent ways:
- **MCP:** call the `edit_batch` tool with the `edits` array — it computes +
  syntax-verifies every edit in memory and writes only if all pass.
- **CLI:** run `said edit ... --json` per edit; on any `ok:false` (non-zero exit)
  **abort the whole set** and don't open a PR. On a rejection, read `valid_anchors`
  from the error and retry that edit once with a suggested move. Use `--explain`
  first if unsure where a member should go.

After applying, optionally `said reindex <file>` to refresh the clone's brain.

**Net effect:** the LLM can only insert/replace at a named anchor or append into
a named scope — it physically cannot delete the rest of a file or nest a member
inside another method.

### Recommendation (0.6.0) — the patterns that maximize first-try success

Updated guidance. In priority order for an autonomous cycle:

1. **Adding a new member (method/test/field) → use `append-into-symbol`** with
   the enclosing class/scope name. It always lands at the right scope (never
   nested inside another method) and auto-indents. This is the single biggest
   first-try-success win and removes the CS0106 class of failures entirely.
2. **Pre-validate with `--explain`** before a non-obvious edit: ask "where can I
   add to class X?" and get the `valid_anchors` menu up front, so the cycle picks
   the correct move on the first attempt rather than learning it from a failure.
3. **Editing an existing region → context anchors** (`insert-after-context` /
   `replace-context`): a unique multi-line block, so the edit can't land in the
   wrong place when a short string repeats. Plain `*-text` is fine for a clearly
   unique single line.
4. **On any rejection** (`ok:false`, non-zero exit): read `valid_anchors` from the
   error and re-issue with those exact args — one-shot correction, no prose.

`replace-symbol`/`delete-symbol` are safe (drift-checked + authoritative span),
but for *adding* members, `append-into-symbol` is strictly better. The C#/Groq
consumer passes args programmatically, so it's unaffected by shell
quote-stripping (which can mangle `"`-containing anchors on a raw command line) —
prefer `--content-file` for multi-line content regardless.

---

## 5. Acceptance behavior (what "working" looks like)

- `insert-after-text` on a real file → exactly one line added, every other line
  unchanged (`git diff` shows a single-region change — 161→162, never 161→27).
- Missing symbol/anchor → `{"ok":false}`, exit 1, **file untouched**.
- `replace-symbol` on a 5-line method → only those 5 lines change; neighbors survive.

Full reference: [`docs/said-structure/07-cli-reference/edit.md`](said-structure/07-cli-reference/edit.md)
(CLI) and [`docs/said-structure/08-mcp-reference/edit.md`](said-structure/08-mcp-reference/edit.md) (MCP).

# `said edit` — Handoff for the Linux box / Advisory

**Date:** 2026-06-14 · **Version:** `said 0.4.0` (both `said` CLI and `said-mcp`)
**Artifact:** `said-full-linux-x64`

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
| **Version bumped to 0.2.0** | `said --version` → `said 0.2.0`. Use this to confirm the new binary is live in the container (old one was `0.1.0`). |
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
   /app/said --version          # must print: said 0.2.0
   ```
   If it still says `0.1.0`, the old binary is still baked in — rebuild the image.

> **Shell note (carried over from build.md):** under Git Bash, prefix
> `docker exec` calls with `MSYS_NO_PATHCONV=1` or `/app/said` gets rewritten to
> `C:/Program Files/Git/app/said`. Not needed from PowerShell/WSL/native Linux.

---

## 3. How to use `said edit`

```
said edit --path <Advisory.said> --file <relative/path> <MODE> \
          [--symbol <name> | --anchor <exact text>] \
          [--content <text> | --content-file <f>] [--json] [--dry-run] [--allow-large]
```

`.said` resolves *where* (a symbol's exact line range, or the first line
containing an exact substring); the bytes are written to the **real file on
disk** in the caller's working dir (the clone), NOT into the `.said` store.

### Modes

| Mode | Anchor | Effect |
|------|--------|--------|
| `insert-after-symbol`  | `--symbol` | Insert after the symbol's last line. |
| `insert-before-symbol` | `--symbol` | Insert before the symbol's first line. |
| `replace-symbol`       | `--symbol` | Replace the symbol's whole line range. |
| `delete-symbol`        | `--symbol` | Remove the symbol's line range. |
| `insert-after-text`    | `--anchor` | Insert after the first line containing the exact text. |
| `insert-before-text`   | `--anchor` | Insert before that line. |
| `replace-text`         | `--anchor` | Replace only the matched substring (first occurrence). |

### Output

- `--json` success: `{"ok":true,"file","mode","anchor","applied_at_line","lines_added","lines_removed","dry_run"}`
- `--json` failure: `{"ok":false,"error":"..."}` + **non-zero exit code**, file untouched.
- `--dry-run`: resolve + report the target line, but **write nothing**.

### Safety guarantees (the point of the feature)

1. **No whole-file write** — largest op is `replace-symbol`, bounded to that symbol's range.
2. **Anchor must resolve uniquely** — 0 or >1 matches → error, nothing written.
3. **Bounded change size** — replace/delete > 200 lines is rejected unless `--allow-large`.
4. **Atomic write** — temp file + rename, so a crash can't leave a half-written file.
5. **Path safety** — `--file` must be relative; no `..`, no absolute paths.
6. **Newline preserved** — LF/CRLF and trailing newline kept intact.

---

## 4. How the Groq cycle should use it (the change to GroqCycle.cs)

**Stop asking the LLM for full file content.** Instead ask for a structured edit
set and apply each via `said edit` in the cloned repo:

```json
{
  "summary": "Add GET /api/host",
  "edits": [
    { "file": "src/Advisory.Api/Program.cs",
      "mode": "insert-after-text",
      "anchor": "app.MapGet(\"/api/pid\"",
      "content": "app.MapGet(\"/api/host\", () => Results.Ok(new { host = Environment.MachineName })).AllowAnonymous();" }
  ]
}
```

For each edit, run in the clone's working dir:
```bash
said edit --path <clone>/Advisory.said --file <edit.file> <edit.mode> \
  (--symbol <s> | --anchor <a>) --content-file <tmp> --json
```
If any edit returns `ok:false`, **abort the whole change set** (don't open a
partial PR). Optionally `said reindex <file>` after to refresh the clone's brain.

**Net effect:** the LLM can only insert/replace at a named anchor — it physically
cannot delete the rest of a file.

### Recommendation: prefer **text anchors** over symbol modes for autonomous use

`replace-symbol`/`delete-symbol` depend on the symbol index's line ranges. The
chunker fix makes these accurate now, but text anchors (`insert-after-text`,
`replace-text`) don't depend on the index at all — they match the exact bytes in
the on-disk file. For an autonomous code-changer, text anchors are the most
robust. The C#/Groq consumer passes args programmatically, so it is unaffected by
shell quote-stripping (which can mangle `"`-containing anchors on the command line).

---

## 5. Acceptance behavior (what "working" looks like)

- `insert-after-text` on a real file → exactly one line added, every other line
  unchanged (`git diff` shows a single-region change — 161→162, never 161→27).
- Missing symbol/anchor → `{"ok":false}`, exit 1, **file untouched**.
- `replace-symbol` on a 5-line method → only those 5 lines change; neighbors survive.

Full reference: [`docs/said-structure/07-cli-reference/edit.md`](said-structure/07-cli-reference/edit.md)
(CLI) and [`docs/said-structure/08-mcp-reference/edit.md`](said-structure/08-mcp-reference/edit.md) (MCP).

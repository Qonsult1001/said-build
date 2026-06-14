# `said edit` — surgical, anchored source edits

`said edit` applies a **precise, bounded** change to a source file on disk — insert, replace, or
delete a small region anchored at a named symbol or an exact text match. **There is no whole-file
rewrite path**, so an autonomous caller (e.g. an LLM mutation cycle) physically cannot delete the
rest of a file. This is the durable fix for the production failure where an LLM asked to return
"full new file content" silently deleted most of `Program.cs` (161 → 27 lines), breaking the app.

`.said` resolves *where* the change goes (a symbol's exact `start_line..end_line` from the symbol
index, or the first line containing an exact substring); the bytes are written to the **real file on
disk**, not into the `.said` frame store. After editing, run `said reindex <file>` to refresh the brain.

## Usage

```
said edit --file <relative/path> <MODE> [--symbol <name> | --anchor <text>] \
          [--content <text> | --content-file <f>] [--json] [--dry-run] [--allow-large]
```

## Modes

| Mode | Anchor arg | Effect |
|------|-----------|--------|
| `insert-after-symbol`  | `--symbol` | Insert content on the line after the symbol's `end_line`. |
| `insert-before-symbol` | `--symbol` | Insert before the symbol's `start_line`. |
| `replace-symbol`       | `--symbol` | Replace the symbol's whole `start_line..=end_line` range. |
| `delete-symbol`        | `--symbol` | Remove the symbol's line range. |
| `insert-after-text`    | `--anchor` | Insert after the first line containing the exact anchor substring. |
| `insert-before-text`   | `--anchor` | Insert before the first line containing the anchor. |
| `replace-text`         | `--anchor` | Replace only the matched substring (first occurrence). |
| `insert-after-context` | `--anchor` | `--anchor` is a (multi-line) block that must occur **exactly once**; insert after it. |
| `insert-before-context`| `--anchor` | Same uniqueness rule; insert before the block. |
| `replace-context`      | `--anchor` | Replace the unique block. Errors on 0 or >1 matches. |
| `append-into-symbol`   | `--symbol` | Insert at the END of the named scope's body (before its closing brace), auto-indented to match siblings. "Add a member to this class" lands at class scope, never nested inside another method. |

**`--explain` (pre-validate):** `said edit --file X --explain (--symbol S | --anchor A) --json`
returns the `valid_anchors` menu for that location **without editing** (exit 0,
no write), so a caller picks the right move up front. `mode` is optional with `--explain`.

**Structured repair menu:** when an edit is rejected by the syntax check, the
`--json` error carries a `valid_anchors` array of copy-paste-ready `said edit`
argument sets (computed live from the AST at the landing line) — so an
autonomous caller can pick a correct move and retry in one shot:

```json
{ "ok": false, "error": "...syntax error — edit rejected, file unchanged",
  "valid_anchors": [
    { "mode": "append-into-symbol", "symbol": "HealthTests", "note": "...class scope" },
    { "mode": "insert-after-symbol", "symbol": "Pid_test", "note": "...same scope" } ] }
```

**Context vs text anchors:** `*-text` uses the *first* line containing the substring;
`*-context` requires the (possibly multi-line) anchor to be **unique** and errors if it
repeats. Prefer `*-context` for autonomous edits where a short string might appear twice.

**Syntax verification:** after every edit, code files are re-parsed with tree-sitter; an
edit that would introduce a syntax error is **rejected and the file is left unchanged**.
Pass `--no-verify` to skip. Unknown file types skip the check automatically.

Provide new content with `--content <inline>` or `--content-file <path>` (preferred for multi-line code).
`delete-symbol` needs neither.

## Resolution rules

- `--symbol` resolves via the **same lookup `said sym` uses**, **scoped to `--file`**. Errors if 0
  matches (not found) or >1 (ambiguous — never guesses; the error lists the candidates).
- `--anchor` matches the exact substring in the on-disk file; errors if 0 matches. Uses the first match
  and reports the line number.
- The file's newline style (LF vs CRLF) and the trailing-newline state are preserved.

## Safety guarantees

1. **No whole-file write.** The largest single op is `replace-symbol`, bounded to that symbol's range.
2. **Anchor must exist.** Unresolvable symbol/anchor → clear error, **nothing written**.
3. **Bounded change size.** `replace`/`delete` spanning more than 200 lines is rejected unless
   `--allow-large` is passed (guards against a prompt selecting a huge range).
4. **Atomic write.** Writes a temp file and renames over the target, so a crash can't leave a
   half-written source file.
5. **Path safety.** `--file` must be relative and may not contain `..` or be absolute.

## Output

`--json` success: `{ "ok": true, "file", "mode", "anchor", "applied_at_line", "lines_added", "lines_removed", "dry_run" }`
`--json` failure: `{ "ok": false, "error": "..." }` with a non-zero exit code.
`--dry-run` resolves and computes the change, prints the target, but writes nothing.

## Examples

```bash
# Preview an endpoint insertion — does NOT modify the file
said edit --file src/Advisory.Api/Program.cs insert-after-text \
  --anchor 'MapGet("/api/pid"' \
  --content 'app.MapGet("/api/host", () => Results.Ok(Environment.MachineName));' --dry-run --json

# Apply it — single-line insertion, every other line untouched
said edit --file src/Advisory.Api/Program.cs insert-after-text \
  --anchor 'MapGet("/api/pid"' --content-file new_endpoint.txt --json

# Replace one method by name, scoped to its file (bounded to its line range)
said edit --file src/Advisory.Api/Program.cs replace-symbol --symbol Configure \
  --content-file new_configure.txt --json
```

> **Shell-quoting note:** anchors containing quotes (`MapGet("/api/pid"`) can be mangled by the shell
> (PowerShell/Git Bash strip embedded `"`). Prefer a quote-free substring for the anchor, use
> `--content-file`, or pass arguments programmatically (the C#/Groq consumer does this and is
> unaffected). This is the same class of issue that turns `/app/said` into `C:/Program Files/Git/app/said`
> under Git Bash — see `docs/build.md` (`MSYS_NO_PATHCONV=1`).

## How the Advisory Groq cycle uses it

Instead of asking the LLM for whole-file content, the cycle asks for a structured edit set
(`{ file, mode, anchor, content }[]`) and runs `said edit` per item in the cloned repo. If any edit
returns `ok:false`, the whole change set aborts (no partial PR). The LLM can only insert/replace at a
named anchor — it has no way to delete the rest of a file.

## Note on symbol-range accuracy (chunker fix)

`replace-symbol` / `delete-symbol` are only as precise as the symbol ranges in the index. A bug in the
AST chunker used to **merge any function with a <3-line body into its predecessor** — the short
function vanished from the symbol index and the predecessor's `end_line` over-extended to swallow it,
which made `replace-symbol` destroy the short neighbour. Fixed: only anonymous fragments fold into a
predecessor; **every named definition stays a distinct chunk with an exact line range**. For
fully autonomous edits, text anchors (`*-text`) remain the most robust because they don't depend on
index ranges at all.

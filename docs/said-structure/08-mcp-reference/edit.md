# MCP tool: `edit`

Surgical, anchored edit of a source file on disk — the MCP equivalent of the
[`said edit`](../07-cli-reference/edit.md) CLI command. Both share one
implementation (`sca_core::edit`), so the safety guarantees are identical:
**there is no whole-file-rewrite path**, so an LLM driving this tool cannot
delete the rest of a file.

## When to use it

Prefer `edit` over rewriting a whole file. Recall the anchor location first
(`sym <name>` for a symbol range, `grep <text>` to find the exact line), then
apply a bounded change. This is the durable fix for the failure where an agent
asked for "full new file content" silently deleted most of a file.

## Parameters

| Field | Required | Meaning |
|-------|----------|---------|
| `file` | yes | Repo-relative path to change (no `..`, not absolute). |
| `mode` | yes | One of the 7 modes below. |
| `symbol` | for `*-symbol` modes | Symbol name, resolved via the symbol index scoped to `file`. |
| `anchor` | for `*-text` modes | Exact substring; the first matching line is the anchor. |
| `content` | all but `delete-symbol` | New text to insert/replace. |
| `dry_run` | no | Resolve + compute the change but do NOT write. |
| `allow_large` | no | Permit a replace/delete spanning > 200 lines. |

### Modes

`insert-after-symbol` · `insert-before-symbol` · `replace-symbol` ·
`delete-symbol` · `insert-after-text` · `insert-before-text` · `replace-text`

## Result

JSON: `{ "ok": true, "file", "mode", "anchor", "applied_at_line", "lines_added", "lines_removed", "dry_run" }`.
On failure: `{ "ok": false, "error": "..." }` and nothing is written.

## Safety (identical to the CLI)

1. No whole-file write — largest op is `replace-symbol`, bounded to that symbol's range.
2. Anchor must resolve uniquely (0 or >1 → error, nothing written).
3. Bounded span (200-line default; `allow_large` to override).
4. Atomic write (temp + rename).
5. Path safety (relative only, no `..`).
6. Newline style (LF/CRLF) preserved.

## Example call

```json
{
  "name": "edit",
  "arguments": {
    "file": "src/Advisory.Api/Program.cs",
    "mode": "insert-after-text",
    "anchor": "MapGet(\"/api/pid\"",
    "content": "app.MapGet(\"/api/host\", () => Results.Ok(Environment.MachineName));",
    "dry_run": true
  }
}
```

Symbol-mode accuracy depends on the symbol index ranges; for fully autonomous
edits, text/context anchors are the most robust (they don't depend on index
ranges). See the [CLI reference](../07-cli-reference/edit.md) for the full
rationale and the chunker-accuracy note.

## `edit_batch` — transactional multi-edit

Apply a **set** of edits all-or-nothing. Every edit is resolved + applied +
syntax-verified **in memory first**; the files are written only if *every* edit
succeeds. If any fails, nothing is written — you can never get a half-applied
change set on disk. Send at most one edit per file per batch.

```json
{
  "name": "edit_batch",
  "arguments": {
    "edits": [
      { "file": "src/Program.cs", "mode": "insert-after-context",
        "anchor": "app.MapGet(\"/api/pid\", () => Results.Ok(pid));",
        "content": "app.MapGet(\"/api/host\", () => Results.Ok(host));" },
      { "file": "tests/HealthTests.cs", "mode": "insert-before-text",
        "anchor": "// --- end of endpoint tests ---",
        "content": "[Fact] public async Task Host_200() { /* ... */ }" }
    ],
    "dry_run": true
  }
}
```

Result: `{ "ok": true, "edits": [...per-edit summaries...], "files_changed": N }`,
or `{ "ok": false, "error": "edit K of N (file) failed: ... — NO files written" }`.

**Use `edit_batch` for any change that spans more than one file** (e.g. an
endpoint and its test) so they land together or not at all.

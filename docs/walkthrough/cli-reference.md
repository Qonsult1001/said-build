# Command reference

Every `said` command you need for personal-memory use, in one place. Each command takes
`--path <file>.said` to pick which brain to use — or set a default once (see
[How to set a default brain file](how-to-set-a-default-brain.md)) and drop `--path`.

## The everyday commands

| Command | What it does | Example |
|---------|--------------|---------|
| `create <file>.said` | Make a new, empty brain file. | `said create my-brain.said` |
| `add "<note>" --id <name>` | Store a memory. `--id` is a short name you can use to fetch it later (optional — auto-named if omitted). | `said add "Wifi is sunflower-42" --id wifi` |
| `ask "<question>"` | Find memories by meaning, in plain English. **The main command** — use it for almost everything. | `said ask "what's the wifi password"` |
| `get <id>` | Show one memory's exact text by its `--id`. | `said get wifi` |
| `delete <id>` | Remove a memory by its `--id`. | `said delete wifi` |
| `stats` | Show how many memories the brain holds. | `said stats` |
| `use <file>.said` | Set the default brain so you can skip `--path`. | `said use my-brain.said` |

## How many answers does `ask` give back?

`ask` leads with the **best memory** for your question. If a few of your memories might
fit, it shows those too — so it never hides the right one just because it wasn't 100%
sure. Seeing two or three results means it's being careful, not confused; the top one is
the best memory it found.

Each result line looks like this:

    1. [0.55][semantic] wifi
        The wifi password is sunflower-42.

- The first number is a confidence score (higher = better match).
- `wifi` is the memory's id (use it with `get` or `delete`).
- The line below is the memory itself.

## Useful flags

- `--path <file>.said` — which brain to use (on any command). Skip it if you've run `use`.
- `--top <N>` on `ask` — ask for more or fewer results (default is plenty for everyday use).
- `--id <name>` on `add` — give a memory a memorable name so `get`/`delete` can find it.

Run `said <command> --help` to see all options for any command.

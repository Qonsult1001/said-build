# How to set a default brain file

> Goal: stop typing `--path my-brain.said` on every command. This assumes you've already created a brain
> file; if not, do the [tutorial](tutorial-your-first-brain.md) first.

`said` decides which brain to use in this order: an explicit `--path`, then a single `.said` file in the
current folder, then a saved default. You can lean on either of the last two.

## Steps

1. Set a saved default once:

       said use my-brain.said

   You should see:

       Default .said file set to: my-brain.said

2. Now run commands **without** `--path` — they use the default:

       said ask "what is the wifi password"

   You should see a normal answer, e.g.:

       Ask: "what is the wifi password"  (1 results in 1.11ms)

         1. [0.55][text] wifi
             Wifi password is sunflower-42.

3. Decide which "no `--path`" behaviour you want going forward:

   - **If you work in one folder that holds exactly one `.said` file** → you don't even need `use`.
     `said` auto-detects the single `.said` file in the current directory. Just run `said ask "…"` there.
   - **If you switch between several brains** → keep using `use` to point at the active one, or pass
     `--path` explicitly when you want a different brain for a single command (it overrides the default
     for that command only).
   - **If you run from a folder with *no* `.said` file and no default set** → `said` will stop and tell
     you to create one, pass `--path`, or run `use`.

## Result

Everyday commands are now short — `said ask "…"`, `said add "…"` — and target your chosen brain
automatically. The default is remembered across terminal sessions (stored in your user config dir).

## See also

- Day-to-day storing and recalling → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- Full options for `use` → the [CLI reference](../said-structure/07-cli-reference/).

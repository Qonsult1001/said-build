# How to track versions of a memory and undo a change

> Goal: see how a memory changed over time and roll it back to an earlier version. This
> assumes you have a brain with some memories in it; if not, do the
> [tutorial](tutorial-your-first-brain.md) first.

Every time you save a memory under an id you've used before, `said` keeps the old version
instead of throwing it away — so you can look back and undo a change.

## Steps

1. Update a memory by adding it again with the **same id**. The new text becomes current;
   the old one is kept as a past version.

       said --path my-brain.said add "Mom's birthday is June 14." --id bday
       said --path my-brain.said add "Mom's birthday is June 15." --id bday

2. See the versions of that memory:

       said --path my-brain.said history bday

   You'll see each version, newest last:

       versions: 2

         v0   [past] ...  Mom's birthday is June 14.
         v1   [HEAD] ...  Mom's birthday is June 15.

   `[HEAD]` is the current version; `[past]` versions are kept and restorable.

3. Roll back to an earlier version by its number from the list:

       said --path my-brain.said checkout bday --version 0

   You should see:

       Checked out frame_id=0 as new HEAD for bday

4. Confirm the memory is back to the earlier text:

       said --path my-brain.said get bday

   You should see:

       Mom's birthday is June 14.

   - **If you change your mind** → roll forward the same way: `said history bday` to see
     the list again (rolling back adds a new entry, so nothing is ever lost), then
     `checkout` the version you want.

## Result

You can review the full history of any memory and restore any earlier version. Rolling
back is itself recorded, so you can always go forward again.

## See also

- Storing and updating memories → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- Accidentally deleted a memory? → [How to recover a deleted memory](how-to-recover-a-deleted-memory.md)
- All commands → [Command reference](cli-reference.md)

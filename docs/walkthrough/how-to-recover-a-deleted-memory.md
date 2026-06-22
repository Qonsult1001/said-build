# How to recover a deleted memory

> Goal: bring back a memory you deleted. This assumes you have a brain you've been using;
> if not, do the [tutorial](tutorial-your-first-brain.md) first.

Deleting a memory is reversible — `said` keeps it in a recycle bin until you permanently
clear it. So a delete you regret is easy to undo.

## Steps

1. (For context) deleting a memory removes it from answers:

       said --path my-brain.said delete wifi

   You should see:

       Deleted: wifi

   After this, `ask` and `get` no longer return it — but it isn't gone for good yet.

2. See what's in the recycle bin (deleted memories you can still recover):

       said --path my-brain.said admin list-tombstones

   You should see the deleted memory listed:

       Tombstoned frames (1):
         wifi (frame #0, 34 bytes, ...)

3. Recover it by its id:

       said --path my-brain.said admin restore wifi

   You should see:

       ✓ Restored doc_id 'wifi' as frame #0.

4. Confirm it's back:

       said --path my-brain.said get wifi

   You should see the original text:

       The wifi password is sunflower-42.

   - **If `restore` says "no tombstone found"** → that memory was already permanently
     cleared (see below), so it can't be recovered. Check what's recoverable with
     `admin list-tombstones`.

## Permanently clearing deleted memories

Deleted memories stay recoverable until you explicitly purge them to reclaim space:

    said --path my-brain.said compact --drop-history --all

After this, deleted memories are gone for good and can no longer be restored. Use it when
you're sure you won't need them — it's the only thing that makes a delete permanent.

## Result

A deleted memory is recoverable from the recycle bin until you `compact --drop-history`.
Everyday `delete` is safe to undo.

## See also

- Storing and removing memories → [How to store and recall personal notes](how-to-store-and-recall-notes.md)
- Rolling back a *change* (not a delete) → [How to track versions of a memory](how-to-track-versions-of-a-memory.md)
- All commands → [Command reference](cli-reference.md)

## B-BACKUP-`prune`-DELETED-THE-BACKUP-EVERY-BACKUP-IT-KEPT-DEPENDED-ON (lane C, 2026-08-16) — FIXED

**In short:** `backup prune --keep-last 3` said it kept your three newest
backups. Two of those three could not be restored afterwards, and the data was
physically deleted from disk. The three newest are usually *incremental* —
each stores only what changed since the one before, so restoring any of them
needs the older full backup at the start of the chain. Pruning by age deleted
exactly that one, then deleted its file contents as unreferenced, printed
"Prune complete" and exited 0. You would find out the next time you tried to
restore. Fixed: pruning now keeps the whole chain a kept backup depends on, and
says so.

### The bug

`compute_retention` in `apps/backup/src/main.rs` selected purely by age —
`keep_last`, `keep_daily`, `keep_weekly`, `keep_monthly` — and knew nothing
about `parent_id`. For the standard usage pattern (one full backup, then
incrementals forever) that is precisely the wrong selection: the newest N are
all incrementals, and the one thing they all need is the oldest.

Then `cmd_prune`'s garbage collector removed every blob no *surviving* manifest
referenced. The full backup's blobs — which is to say, the actual file contents
— were referenced only by the manifest just deleted.

The two halves compound. Deleting the base manifest alone would be recoverable
in principle, since the blobs would still be there. Deleting the blobs too is
not recoverable by anything.

### A second, independent way the same GC destroyed data

```rust
if let Ok(manifest) = load_manifest(&opts.dest, &meta.id) {
    for entry in &manifest.files { referenced_hashes.insert(entry.hash.clone()); }
}
```

A manifest that failed to load contributed **no hashes**, so every blob only it
referenced was collected as an orphan. One unreadable manifest — a transient
I/O error, a permissions problem, a partial write — silently destroyed the file
contents of a backup that was being *kept*. The `if let Ok` reads as caution;
it is the opposite.

### The fix

`compute_retention` now returns a `Retention` struct, and runs `keep_ancestors`
over the policy's selection: walk `parent_id` from every selected backup and
hold everything on the way to the root. A `parent_id` naming a backup not
present is deliberately **not** invented — keeping a phantom id would report
that we are holding a backup that does not exist. It is returned as a broken
chain and reported to the user, because its child cannot be restored either
way and that is worth saying out loud rather than keeping a corpse.

`insert` returning false is also what terminates a `parent_id` cycle in
hand-edited or corrupt metadata.

The manifest read in the GC is now propagated, with a message naming the
consequence:

> refusing to collect unreferenced blobs: cannot read the manifest for backup
> {id}: {e}. Blobs it references would be deleted as orphans.

Refusing to prune is recoverable; deleting blobs is not. That asymmetry is the
whole argument.

`prune` reports both `Keeping N older backup(s) that newer ones are built on
top of:` and any broken chains. For a purely linear chain this can make `prune`
a near no-op — which is the correct behaviour, and is now visible rather than
silent.

### Verification

Five tests. Neutralising `keep_ancestors` turns exactly the four chain tests
red out of 63, and only those. `independent_full_backups_are_pruned_as_the_policy_says`
stays green, as it must: with no chains the two implementations agree, which is
exactly why this survived — anyone testing prune with standalone full backups
sees correct behaviour.

Also covered: a `parent_id` cycle does not hang the prune, and a parent that is
already gone is reported rather than invented.

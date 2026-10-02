## B-SYSTEMRESTORE-EXPORT-IMPORT-FLATTENED-THE-SNAPSHOT-TREE (lane C, 2026-08-16) — FIXED

**In short:** System Restore keeps snapshots in a family tree — a big "full"
snapshot, and under it a chain of small "incremental" ones that each record
only what changed since the one before. Exporting the snapshots to a file and
importing them back threw the family away: every snapshot came back as an
unrelated orphan. Since an incremental snapshot is meaningless without knowing
which snapshot it was taken against, importing a backup gave you a pile of
deltas nobody could replay. Fixed: the import now rebuilds the tree.

**Where:** `apps/systemrestore/src/main.rs`, `SnapshotManager::import_snapshots`.

**The bug.** Every snapshot was added with `parent_id = None`, under a comment
saying "Parent relationships from export are not guaranteed to exist". The
parents do exist — they are right there in the same file. What is not
guaranteed is that they appear *before* their children, and `add_snapshot`
only accepts a parent that is already in the tree. So a real ordering problem
was resolved by discarding the data instead of by reordering the work.

**The fix.** Two passes. Pass 1 adds every snapshot detached and records
`old id -> new id` in a remap (exported ids cannot be reused — they can
collide with snapshots already in the tree). Pass 2 walks the imported
snapshots again and re-links each one through the remap. A parent that is not
in the file — an export of one sub-tree, or a hand-edited file — leaves that
snapshot as a root rather than failing the import: losing a snapshot's
position in the tree is recoverable, refusing the import loses the snapshot
itself.

**A hang, found on the way.** Re-linking needs a re-parenting operation, so
`SnapshotTree::set_parent` is new — and re-parenting is what makes cycles
reachable. A cycle here is not a wrong answer, it is a frozen application:
`depth_of` and `ancestry_chain` follow parent links until they reach a root,
and both run inside the GUI event loop. `set_parent` rejects a cycle at the
mutation point, and both walks are *additionally* bounded by the snapshot
count, so a tree corrupted by some other route (a future importer, a
hand-edited file) still cannot hang the app. Defence in depth on purpose: the
bound alone would give a wrong answer silently, and the rejection alone trusts
that `set_parent` stays the only writer.

**Also.** The two `let _ =` discards on `lock_snapshot` / `add_tag` in the same
function became `?`. Neither can fail today (both only return `NotFound(id)`
for an id `add_snapshot` just returned), so this is not a live bug — but a
discarded result is how a later change to that guarantee would silently unlock
a snapshot the user marked as protected, and the lock is the only thing
stopping retention from pruning it.

**Not a bug, checked and retracted:** `size_bytes` looked like it was lost too,
since the export carries it and the import never sets it. It is not —
`Snapshot::new` derives it from `components`, which *are* passed through, so it
is reconstructed identically. Only the parent link was genuinely lost.

**Tests:** `an_import_preserves_the_snapshot_chain`,
`an_import_preserves_locks_and_tags`,
`an_import_links_a_child_that_appears_before_its_parent`,
`an_import_of_a_subtree_without_its_parent_keeps_the_snapshot`,
`set_parent_refuses_to_create_a_cycle`,
`set_parent_moves_a_snapshot_between_parents`, `set_parent_rejects_unknown_ids`.
Injection-verified: disabling pass 2 fails exactly the two chain tests and
nothing else.

**Sweep note.** Third find of the `apps/**` sweep, and the third instance of
the same shape as the explorer and PTY bugs — the app had the right answer in
hand and dropped it. Worth noting how it differed, though: the explorer and
PTY bugs were `let _ =` on a call, which greps. This one was an argument —
a literal `None` passed where a value was available — which does not grep at
all. It was found by reading the comment that justified it. A comment
explaining why a value is being discarded is a reliable marker for this bug
class, and a better one than the discard syntax, because it is written at
exactly the moment the author knew they were cutting a corner.

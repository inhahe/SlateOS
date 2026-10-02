### TD-DIFFCORE-AN-MTIME-RECORDED-IN-THE-SAME-TICK-AS-ITS-OWN-WRITE-WAS-TRUSTED. `FileSync::changed` reported an external edit as `Unchanged`, so the next save would have silently overwritten it — 2026-08-12 — ✅ FIXED 2026-08-12

**Where:** `apps/diffcore/src/lib.rs::FileSync::changed` (the mtime fast path)
and `FileSync::record`/`touch` (which captured the timestamp). Consumers:
`apps/editor/src/main.rs::Document::disk_changed` and the identical method in
`apps/markdowneditor/src/main.rs` — both feed the external-change prompt that
offers a three-way merge.

**How it surfaced.** The full-workspace run after commit `a6e286332` failed
`diffcore`'s own `tests::test_filesync_detects_modify_and_delete` with
"expected Modified, got Unchanged". It reads as a timing flake and is not one:
the test writes a file, records it, rewrites it, and asks. It passed or failed
depending on whether the two writes landed in the same filesystem timestamp
tick — i.e. on machine speed.

**The bug.** `changed()` treated "the file's current mtime equals the one I
recorded" as proof the file was untouched. A filesystem timestamp is coarse
(FAT: 2 s; ext3: 1 s; NTFS is nominally 100 ns but is stamped from a system
clock that ticks in ~15.6 ms steps), so a write landing inside the same tick as
the recording carries the *same* mtime. The pre-filter then short-circuits to
`Unchanged`, the editor never raises the external-change prompt, and the next
save writes the buffer over the external edit with no merge and no warning —
silent data loss, not a missed notification.

**Why "compare `SystemTime::now()` when checking" does not fix it.** Two wrong
fixes were tried first. Re-deciding raciness at check time is unsound: the
aliasing write happened *inside* the granularity window, which has already
closed by the time anyone asks, so no amount of later waiting makes the
timestamp more informative. Deleting the fast path outright is sound but throws
away a real optimisation (an untouched file is not re-read).

**The fix.** Git's "racily clean" rule from `read-cache.c`. `record`/`touch`
now go through a private `stat()` that captures the mtime *and* decides, at
that moment, whether it is trustworthy: `SystemTime::now() - mtime <
MTIME_SETTLE` (2 s, sized for the coarsest granularity we could plausibly be
asked to watch) sets `FileSync::mtime_racy`, and `changed()` refuses the fast
path while it is set. A too-large `MTIME_SETTLE` only costs a content read; a
too-small one loses an edit.

**Known cost, accepted.** The flag is sticky until the next `record`/`touch`,
so a file recorded immediately after a save is content-compared on every check
until the next save. Git accepts the same cost (it only smudges a racily-clean
entry when it rewrites the index). The refinement — clear the flag from
`changed` once a content comparison came back identical *and* the mtime has
outlived `MTIME_SETTLE` — would force `changed` to take `&mut self` and so
would stop read-only UI code from asking. Revisit if a caller is shown to poll
hot; `check_external_change` currently has no production caller at all.

**Regression tests.** `test_filesync_same_tick_rewrite_is_not_missed` (written
with no `sleep` on purpose — a sleep would hide the very adjacency being
tested) and `test_filesync_settled_mtime_is_trusted` for the other half of the
trade.

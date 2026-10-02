## B-EXPLORER-RECYCLE-BIN-IDS-COULD-COLLIDE-AND-DESTROY-A-FILE (lane C, 2026-08-16) — FIXED

**In short:** When you delete a file, the file explorer moves it into a recycle
bin folder under a made-up name. The name was built from the file's own name
plus the current time — so two files that share a name and are deleted at the
same moment got the *same* made-up name, and the second one landed on top of
the first. Selecting `projA/README.md` and `projB/README.md` and pressing
Delete could therefore leave you with one of them, permanently, with no error
message and nothing in the bin to undo it from. Fixed: the bin now asks the
filesystem for a name nobody else has, instead of assuming the clock provided
one.

**Where:** `apps/explorer/src/fileops.rs`, `RecycleBin::recycle` / `make_id`.

**The bug.** `make_id` returned `{file_name}_{hash}` where
`hash = timestamp ^ (name.len() * K)` — for two files with the same name and
the same length, the timestamp is the *only* thing that differs. `recycle`
then did `fs::create_dir_all(&entry_dir)?`, which **succeeds when the
directory already exists**. So a colliding id did not fail; it silently
returned an existing entry's directory. `recycle` proceeded to overwrite that
entry's `meta.txt` and then `move_path`'d its data onto the existing `data`
file, and `fs::rename` overwrites. Net effect: the earlier file's contents and
the record of where it came from were both gone.

**The fix.** `create_entry_dir` uses `fs::create_dir`, which returns
`AlreadyExists` rather than succeeding, and on collision tries `{base}-1`,
`{base}-2`, … up to a bound. Uniqueness becomes something the filesystem
enforces rather than something the clock is trusted to supply — which is also
the only version that holds when two explorer processes share one bin, a case
no clock-derived id can cover. The retry is bounded (4096) so a pathological
bin cannot hang the explorer; exhausting it fails the delete, which leaves the
file where it is, which is the recoverable outcome.

**On reachability — stated plainly, because the first two tests I wrote
passed.** This does *not* reproduce by calling `recycle` twice in a row: each
call does enough filesystem I/O (create dir, write meta, rename data) to
advance even a coarse clock, so consecutive ids differ in practice. Measured
on this host, `SystemTime::now()` ticks at 100 ns and ~70% of back-to-back
readings are identical — but "back to back" with a file rename in between is
not back to back. So the guard against this bug was *timing*, not logic.

That is still worth fixing rather than filing as theoretical, for three
reasons: a coarser clock removes the guard entirely (some VMs expose ~15 ms
granularity through the same API); two explorer processes sharing a bin remove
it regardless of clock; and the failure mode is silent, unrecoverable
destruction of a file the user only asked to put in the bin. A safety property
that holds by accident is worth converting into one that holds by
construction, especially when the conversion is six lines.

**Test seam.** Because the accident hides the bug, `recycle_at(path, ts)` takes
the clock as a parameter and `recycle` calls it with `now_nanos()`. The two new
tests recycle at one fixed instant, which is what makes them able to fail.

**Tests:** `two_same_named_files_recycled_together_both_survive`,
`a_burst_of_same_named_files_keeps_every_one`. Injection-verified: restoring
`create_dir_all` collapses 64 deleted files into a single bin entry and fails
exactly these two.

**Sweep note.** Fourth find of the `apps/**` sweep, and the first that is not
the "computed the answer and threw it away" shape. This one is a different
recurring shape worth naming: **an API that is permissive where the caller
needed it to be strict.** `create_dir_all` succeeding on an existing directory
is correct and documented; it was simply the wrong function to reach for when
the whole point of the call was to claim a name. Related instance already
fixed in this crate: `thumbs.rs`'s
`two_names_differing_only_in_undecodable_units_do_not_share_a_cache_entry`.
Worth grepping the rest of `apps/**` for `create_dir_all` on a path that is
meant to be freshly claimed, and for `fs::rename` onto a path not verified to
be free.

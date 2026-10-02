## B-CP-R-COULD-NOT-REPLACE-AN-EXISTING-SYMLINK (lane B, 2026-08-30) — **FIXED 2026-08-30**

**In short:** `cp -r src dst` failed, with `File exists`, whenever the tree
being copied contained a symlink at a name where the destination tree already
had one. Copying a tree onto a copy of itself — the ordinary way anyone updates
a backup directory — therefore failed on the second run and succeeded on the
first. GNU replaces the link and says nothing. Fixed in
`userspace/coreutils/src/bin/cp.rs`.

**The mechanism.** `copy_entry`'s symlink arm called `symlink(target, at)`
directly. `symlinkat(2)` has no truncate-or-replace mode: it fails with `EEXIST`
if anything at all already exists at the name, a working link included. The
regular-file arm never had this problem because `open(…, O_WRONLY|O_TRUNC)`
opens an existing file happily, and the directory arm never had it because
`create_dir` on an existing directory is tolerated by the surrounding code. The
symlink arm was the one kind of member whose creation call has no "already
there" story, and it was the one arm that had not been given one.

**Why it stayed hidden.** Every recursive test in the file, and every recursive
case in `scripts/cp-diff.sh` before section 16, copied into a destination that
did not yet exist. That is the natural way to write a copy test and it exercises
`new_dst` on every member, so the replace path was never entered. It took a
fixture built specifically to be copied *twice* — `dst2`, "a second copy of the
same tree" — to reach it.

**The fix.** The symlink arm now unlinks an existing non-directory destination
before creating, which is what GNU's `copy_internal` does for the same reason
(it removes the destination when the source is a symlink and the destination is
not a directory). A destination *directory* is still refused, because replacing
a directory with a symlink is not something `cp` does without
`--remove-destination`.

**Certified by** `a_recursive_copy_replaces_an_existing_symlink` in `cp.rs`, and
by `scripts/cp-diff.sh` section 16's recursive sub-group, which copies the same
tree into a destination that already holds it under all four overwrite policies.

**The general point, again.** Section 16 was written to certify `-f`,
`-n` and `--remove-destination` — three options this `cp` did not have. It found
a bug in the code that was already there, in a path none of those options touch.
That is the third time in a week (see the two entries above) that measurement
written for feature A found a defect in feature B, and it is the argument for
writing the harness section *before* the feature rather than alongside it.

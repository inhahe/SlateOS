## B-MVS-CROSS-DEVICE-COPY-WRITES-THROUGH-THE-DESTINATION — FIXED 2026-09-01

**In short:** moving a file onto an existing name, across a filesystem boundary,
overwrites *every* name that file had, not the one that was asked for. If `g` and
`g2` are two names for one file and you run `mv /other/fs/f g`, `g2` should still
hold what it always held — a rename replaces a directory entry, it does not write
through it — but ours rewrites the shared inode, so `g2` silently becomes a copy
of `f`. A file the user never mentioned is destroyed.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `copy_across_devices`'s plain-file
arm, which reaches the destination with `fs::copy(src, target)`. `fs::copy`
opens the destination with `O_CREAT|O_WRONLY|O_TRUNC`; when the destination
exists, that truncates and rewrites the inode already there and every other link
to it sees the new bytes and the new mode.

**What GNU does.** `copy.c:2870`, inside the `EXDEV` block and *before* the copy
is started, unlinks the destination outright:

```c
      /* Remove any existing destination file so that a cross-device `mv'
         acts as if it were really using the rename syscall.  */
      if (unlinkat (dst_dirfd, drelname, S_ISDIR (src_mode) ? AT_REMOVEDIR : 0)
          != 0 && errno != ENOENT)
```

so the copy always creates a fresh inode. Note the failure path: it prints
`inter-device move failed: %s to %s; unable to remove target` and returns
*without* running `un_backup`, which is the one place in `copy.c` where a backup
already taken is deliberately left in place.

**The proper fix.** Unlink the destination before copying, exactly there in the
sequence, with `ENOENT` treated as success. Two orderings matter and neither is
free: the unlink must come *after* whatever refuses the move (this `mv` cannot
copy a directory, and clearing the destination and then refusing would destroy a
file for a move that never happened), and its own failure must not undo a backup.

**How it is caught.** `scripts/mv-diff.sh` §22, the case whose fixture is
`printf XXXXXXXXXX > g; chmod 606 g; ln g g2`. Measured before the fix, ours left
both names at five bytes and mode 644 and still hard-linked to each other; GNU
left `g2` at ten bytes, mode 606, and unlinked from `g`. Note what the case's
fixture does *not* carry: no set-user-ID bit and no pinned time, because a case
that also measured the copy's attribute losses would have gone on differing after
this fix and so could never have reported it.

**Fixed** by `move_one`'s `clear_destination` call, placed exactly where GNU's
`unlinkat` sits — after the directory refusal (hoisted above it, since this `mv`
cannot copy a directory and clearing-then-refusing would destroy a file for a
move that never happened) and before the `copied` line. `ENOENT` is the only
excused error, and the failure path deliberately does not `un_backup`, matching
`copy.c:2884`. Four unit tests cover `clear_destination` directly — the
hard-link case, the absent destination, a symlink destination that must not be
followed, and a destination that will not go — and the §22 case is now a plain
`run_case`.

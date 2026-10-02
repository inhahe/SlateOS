## B-MVS-CROSS-DEVICE-FALLBACK-DOES-NOT-PRESERVE-HARD-LINKS — FIXED 2026-09-01

**In short:** `mv /other/fs/a /other/fs/b dir/` moves two names for one inode
and produces two independent files. A rename would have kept them one file, and
`mv` promises to be indistinguishable from a rename: `cp_option_init`
(`mv.c:119`) sets `preserve_links` (`mv.c:135`) unconditionally, with no option
to turn it off. The bytes are all correct and nothing is lost, so this is invisible until
someone writes to one of the names and the other does not change — or until a
directory that fitted on the disk because of its links no longer does.

**Split out of** `B-MVS-CROSS-DEVICE-FALLBACK-THROWS-AWAY-THE-TIMES-AND-THE-OWNER`,
which is now FIXED for the times, the owner and the mode. This is the one part
of that entry the fix did not reach.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `copy_across_devices`. It is
called once per source with no memory of the sources before it, so there is
nowhere for the "I have already copied this inode" table to live. The refusal is
structural rather than a missing line.

**What GNU does.** `copy.c` keeps `src_to_dest`, a hash of (`st_dev`, `st_ino`)
to the destination name already created, consulted in `copy_internal` before
anything is copied. A source found in it is `link`ed to the name that is already
there instead of being copied again. The table is per-invocation, which is why
two separate `mv` commands correctly produce two separate files.

**The proper fix.** The same table, keyed on `coreutils::fileid::FileId`, owned
by the loop over the operands rather than by `copy_across_devices`, and only
consulted for a source with `nlink > 1` — both of which `fileid` already
provides for `cp`, which has exactly this table for `-l` and `--preserve=links`.
It should land with the recursive directory fallback
(`B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED`) rather than before it: a walk
that recreates a tree needs the identical table, and building it twice is how the
two would come to disagree.

**How it is caught.** `scripts/mv-diff.sh` §22, one `xfail_case` naming this
entry: `mv -v @FAR@/a @FAR@/b d` with the fixture `printf hello > a; ln a b`. The
`links{...}` column is the whole case — the tree and the bytes agree either way,
which is exactly why the harness grew that column.

**FIXED 2026-09-01.** `mv` now carries GNU's one `src_to_dest` table, as
`coreutils::fileid::Copied` on the `Job`, and links a repeat inode to where the
first name landed with `coreutils::hardlink::force_link` — gnulib's
`force_linkat`, moved out of `cp.rs` so that the two utilities cannot disagree
about what "replace" means. `scripts/mv-diff.sh` goes from 334/0/12 to **339
passed, 0 differed, 11 differ on purpose**; the `xfail_case` above is now a
`run_case`, and four more cases joined it.

**Two things this entry got wrong, corrected here rather than silently.** Both
were found by reading `copy.c` and measuring GNU 9.4, not by reasoning:

1. *"only consulted for a source with `nlink > 1`"* — **no.** GNU consults it for
   every non-directory source, `remember_copied` when the count is above one and
   a bare `src_to_dest_lookup` when it is exactly one (`copy.c:2673`). The
   lookup arm is not an optimisation and leaving it out is fatal: by the time the
   *last* of a set of links is reached the earlier ones have been removed and its
   count is back down to 1, so a rule spelled the way this entry spelled it would
   never fire on the operand that needs it most. `mv far/a far/b far/c d` is the
   case; it is in §22 now.
2. *"it should land with the recursive directory fallback rather than before
   it"* — the dependency runs the other way round. The table is a prerequisite of
   the walk, not a co-requisite: it now exists, is exercised, and is in the
   library where the walk will find it. Landing them together would have meant
   writing both at once with only the walk's cases to measure the table by.

**And one thing nobody had noticed.** The table is consulted *before* the rename
that is allowed to replace (`copy.c:2662`), not down beside the copy — so it
changes what a move that never leaves the disk **says**. Measured, GNU 9.4, one
filesystem, with `d/a` and `d/b` already present and `a`/`b` two names for one
inode:

```text
$ mv -v a b d
renamed 'a' -> 'd/a'
removed 'd/b'
removed 'b'
```

The second operand is linked and then unlinked, where this `mv` renamed it and
said `renamed` twice. The resulting tree is identical either way — only the `-v`
transcript can tell — which is why it survived every same-filesystem case in the
harness until the table was put in GNU's position rather than the obvious one.
Two `run_case`s now pin it: that transcript, and the free-destination twin where
the rename succeeds, nothing is recorded, and both lines really are `renamed`.

**Still not covered, and deliberately.** Directories stay out of the table.
Upstream's first arm handles them under `x->recursive` and produces `warning:
source directory %s specified more than once`. This paragraph used to say the
arm "has nothing to protect yet" because a cross-device directory move was
refused outright; that refusal was lifted on 2026-09-03, so the gap is real now
and is tracked as `B-MV-NEVER-WARNS-ABOUT-A-TWICE-NAMED-SOURCE-DIRECTORY` —
which also records how narrow the trigger turned out to be when measured.

The `--update` skip path (`copy.c:2380`), which records and links a skipped
destination —
"we currently replace DST_NAME unconditionally, even if it was a newer separate
file", in upstream's own words — and is tracked separately as
`B-MVS-UPDATE-SKIP-DOES-NOT-LINK-A-REPEATED-INODE`.

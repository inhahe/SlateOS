## B-MVS-UPDATE-SKIP-DOES-NOT-LINK-A-REPEATED-INODE — FIXED 2026-09-01

**In short:** `mv --update a b dir/`, where `a` and `b` are two names for one
file and `dir/a` and `dir/b` already exist and are newer, leaves `dir/a` and
`dir/b` as the two separate files they were. GNU leaves them as one file — it
hard-links `dir/b` to `dir/a` even though it just decided to skip both. That is
as odd as it sounds, and upstream says so itself; it is nonetheless what the
reference does, and this `mv` is measured against the reference.

**Jargon, once.** *Hard link* — two directory entries naming one file, so writing
through either changes both. *`--update`* — skip a destination that is not older
than the source. *Skip* — leave the destination exactly as it was found.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `move_one`. The `earlier_file`
block that consults `Job::copied` sits *after* `refuse_overwrite_checks`, so a
`Verdict::Skipped` returns before it is reached. GNU's equivalent is inside the
`--update` comparison itself (`copy.c:2380`), above the skip.

**What GNU does**, verbatim from `copy.c:2375`:

> However, we still must record that we've processed this src/dest pair, in case
> this source file is hard-linked to another one. In that case, we'll use the
> mapping information to link the corresponding destination names.

and then, on the second name:

> Note we currently replace DST_NAME unconditionally, even if it was a newer
> separate file.

So the skip records into `src_to_dest`, and a later operand naming the same inode
is linked over its destination — the one thing `--update` was asked not to touch.

**Why it is not fixed with the entry above.** Reaching it needs
`refuse_overwrite_checks` to distinguish an `--update` skip from a `-n` skip and
from an `-i` "no": `abandon_move` (the `-n`/`-i` path) deliberately does *not*
record, so the two skips are not interchangeable here. That is a change to the
`Verdict` enum, which every refusal in the file flows through, and it wants its
own commit and its own cases rather than riding along with the table.

**How it would be caught.** `scripts/mv-diff.sh` §22 has no case for it yet. The
shape is `TREE='mkdir d; printf new > d/a; printf new > d/b; touch -d "2030-01-01"
d/a d/b'`, `FAR='printf hello > a; ln a b'`, `mv -uv @FAR@/a @FAR@/b d` — and the
`links{...}` column is again the whole case, since the bytes and the tree agree
either way.

**FIXED 2026-09-01.** `refuse_overwrite_checks` step 2 now does the
`remember_copied` and the `create_hard_link` before returning `Verdict::Skipped`,
which is `copy.c:2380` in place. `scripts/mv-diff.sh` 339 → **341 passed, 0
differed, 11 differ on purpose**; two new `#[cfg(unix)]` tests in `mv.rs`.

Three things the entry above got wrong or did not know, all found by running the
reference rather than reading it:

* *"needs `refuse_overwrite_checks` to distinguish an `--update` skip from a `-n`
  skip … a change to the `Verdict` enum"* — **no.** The two skips were already
  distinguishable: `--update`'s is step 2 and `-n`/`-i`'s is step 3
  (`abandon_move`), separate branches that merely happen to return the same
  verdict. Nothing about `Verdict` had to change and nothing did. The entry
  talked itself into a refactor it never needed, and deferred a ten-line fix
  behind it.
* **`remember` is unconditional here**, with none of the `nlink`-count sorting
  the main `earlier_file` block does (`copy.c:2380` calls `remember_copied`
  outright). It can be: the source is not moving, so its count is whatever it
  always was.
* **The neighbouring case matters as much and was not in the entry.** With `d/a`
  newer but `d/b` *older*, the first operand is skipped-and-recorded and the
  second is not skipped at all — it falls through to the ordinary `earlier_file`
  block, links to `d/a` (a destination this command never wrote), and *removes*
  its source, which the skipped operand did not. One command, one inode, two
  routes into one table, two different answers about whether the source lives.
  Both are now harness cases and both are pinned by a test.

Measured against GNU 9.4 (`d/a`=`newer`, `d/b`=`newer2`, both stamped 2030;
`a`/`b` two names for one older inode):

```text
$ mv -uv a b d
removed 'd/b'
$ cat d/b            # was "newer2"
newer
```

Both sources survive, as the skip promised — and `d/b`, the file `--update`
was asked to protect, is gone, replaced by a second name for `d/a`. Upstream
flags this itself, in a comment twelve lines below the one that justifies the
recording: *"Note we currently replace DST_NAME unconditionally, even if it was
a newer separate file."* It is a real defect in the reference; matching it is
nonetheless the contract this utility is written to.

**Not a divergence, though it looks like one.** When `d/b` does *not* exist, the
speculative rename succeeds, and `copy.c:2663` short-circuits the whole table on
`rename_errno == 0` — so `b` is renamed, the pair stays linked because a rename
kept it so, and nothing is recorded. Ours already did this. It is written down
because reasoning from the code alone suggested the opposite (the first operand
*had* recorded `d/a`, so a table consulted unconditionally would have linked
`d/b` to a file with nothing to do with `b`), and only running both binaries
settled it.

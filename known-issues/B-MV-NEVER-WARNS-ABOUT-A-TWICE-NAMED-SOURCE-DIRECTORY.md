## B-MV-NEVER-WARNS-ABOUT-A-TWICE-NAMED-SOURCE-DIRECTORY — OPEN 2026-09-03

**In short:** naming the same directory twice on one `mv` command line can make
GNU print `mv: warning: source directory 'd' specified more than once` and skip
the repeat. This `mv` prints nothing and tries the move a second time. Nothing is
destroyed either way — the second attempt copies the tree over the copy it
already made — but the transcript differs, and the second attempt is work that
GNU knows to skip.

**Jargon, once.** *Operand* — one of the file names on the command line.
*Cross-device* — source and destination on different filesystems, so `mv` has to
copy and delete rather than rename.

**How narrow this is — measured against GNU 9.4, not reasoned.** Three things
must hold at once, and the third is what makes it rare:

1. the same directory named as two operands, and
2. the move must be cross-device (a same-device move renames, and GNU records
   nothing for a rename that succeeded — `copy.c:2662`), and
3. **the first attempt must have copied the tree and then failed to remove the
   source.** A first attempt that *succeeded* takes the source away, so the
   second operand gets `No such file or directory` from both `mv`s alike; a
   first attempt that failed earlier calls `forget_created`, which takes the
   entry back out of the table, so there is nothing for the second to find.

The reproduction is therefore:

```text
$ mkdir -p FAR/d/sub; printf hello > FAR/d/f; mkdir dest; chmod 555 FAR
$ mv -v FAR/d FAR/d dest
created directory '.../dest/d'
created directory '.../dest/d/sub'
copied '.../FAR/d/f' -> '.../dest/d/f'
removed '.../FAR/d/f'
removed directory '.../FAR/d/sub'
mv: cannot remove '.../FAR/d': Permission denied
mv: warning: source directory '.../FAR/d' specified more than once
$ echo $?
1
```

Ours prints everything down to the `cannot remove`, then copies the tree a
second time instead of printing the warning. The exit status is 1 either way.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `move_one`, the `src_id` block —
`file_id` is asked only for a non-directory, so a directory never enters
`Copied`. The comment there says the same thing.

**What GNU does.** `copy.c:2664` records a directory operand too, under
`x->recursive && S_ISDIR (src_mode) && command_line_arg`, into the same
`src_to_dest` table the hard-link machinery uses. The `earlier_file` block below
it then has a directory arm with two sentences, of which this is one. The other
is `cannot copy a directory, X, into itself, Y`, which this `mv` cannot reach
either — but that one needs source and destination nested, which is a same-device
`rename` returning `EINVAL` and is already reported by the ordinary path with
GNU's own wording.

**The proper fix.** `Copied` starts holding directories, keyed the same way, and
`move_one` asks for a `file_id` unconditionally. The reason it is not done here
is that `Copied::forget` then means something new on every other path: today
"this inode's destination was not created after all" is a claim about a *file*,
and a directory whose subtree is half-copied is not the same claim. That wants
its own stage with its own cases, not a rider on the one that made directories
movable at all.

**Cost of leaving it.** A duplicated directory operand does redundant work and
prints a different transcript. It cannot lose data — the second pass writes the
same tree over the same destination. Nothing else is blocked on it.

**How it is caught.** Not yet. `scripts/mv-diff.sh` §22 cannot express it as it
stands: the fixture needs the *far* directory made read-only after its contents
are built, and the harness's `FAR` hook runs before the move rather than between
the two operands. Teaching the harness that is part of the fix, not of this
entry.

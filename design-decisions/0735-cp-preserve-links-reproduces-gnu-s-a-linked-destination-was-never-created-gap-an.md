## 735. `cp --preserve=links` reproduces GNU's "a linked destination was never created" gap, and the harness grew a column that can see hard links at all

**Date:** 2026-08-30
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `cp --preserve=links a b d` notices that `a` and `b` are two
names for one file and makes `d/b` a second name for `d/a` rather than a
second copy of the bytes. GNU has a small hole in that: a destination it
created by *linking* is not written down in its list of "files this command
just made", so a later operand can silently overwrite it, where the same
command without the option stops and says `will not overwrite just-created`.
We copy the hole. Separately, the differential harness could not previously
see a hard link at all — two names for one file look identical to two copies
in every column it printed — so a new column lists which paths share an inode.

### The gap, measured

GNU's `copy_internal` announces the copy, then reaches the `earlier_file`
branch (`copy.c:2655`), calls `create_hard_link`, and **returns** at
`copy.c:2746`. `record_file (dest_info, dst_name, …)` is 470 lines further
down at `copy.c:3217` and never runs. So `dest_info` — the set that powers the
`will not overwrite just-created` refusal — has an entry for every destination
that was *copied* and none for a destination that was *linked*.

Measured against coreutils 9.4, with `a` and `b` hard-linked and `o/b` a
different file:

| command | result |
|---|---|
| `cp --preserve=links a b o/b d` | exit 0, silence, `d/b` holds `o/b`'s bytes |
| `cp a b o/b d` | `cp: will not overwrite just-created 'd/b' with 'o/b'`, exit 1 |

The option changes whether a third operand is allowed to destroy the second
one's work, which is not something `--preserve=links` is about.

### Why reproduce it

- **The harness is the specification.** `scripts/cp-diff.sh` compares stdout,
  stderr, exit status, the resulting tree, the file bodies and now the link
  groups. A destination that GNU overwrites and we refuse differs in all but
  one of those. Every deliberate divergence has to be argued for in a
  `xfail_case` line, and this one would be arguing that our `cp` is safer than
  the `cp` every script on the system was written against.
- **The "safer" direction is not obviously safer.** Recording the linked
  destination turns a silent overwrite into an exit-1 failure *in the middle of
  a multi-operand copy*, after some operands have already landed. A script that
  today ends with `d/b` holding `o/b` would instead end with a non-zero status
  and the same `d/b`, because the refusal happens before the write but after
  the earlier operands. That is a different partial state, not a complete one.
- **It is a gap, not a policy, and upstream may close it.** If GNU moves
  `record_file` above the `earlier_file` return, the harness reports the case
  as a difference on the next run and we follow. Diverging now would mean
  carrying a permanent `xfail` that hides that signal.

The cost is a `Placed` enum with three variants where a `bool` would do:
`copy_one` records a destination in `Seen` for `Placed::Copied` and not for
`Placed::Linked`, so the variant exists solely to carry this distinction. It is
documented at the type with the transcript above, because a future reader who
"simplifies" it back to a boolean would silently change behaviour and the unit
test `a_linked_destination_is_not_recorded_as_created` is what would catch it.

### The harness column

`snapshot` prints each path's mode, size, symlink text and bytes. None of those
differ between two names for one file and two copies of it, so before this
change every `--preserve=links` case would have passed against a `cp` that
ignored the option outright — the harness would have certified the option
without testing it.

`hardlinks` prints one line per hard-link group, listing that group's member
paths. Not the inode numbers: the two sides run in two directories and have two
disjoint sets of inodes, so the numbers are the one fact about a hard link that
cannot be compared. The membership is the same on both sides whenever the two
programs agree, and differs whenever they do not.

It is empty for every case that creates no hard links, which is every case
outside section 18 — so the column costs nothing where it says nothing, and the
other 400-odd cases were unaffected by its introduction.

### What was rejected

- **Recording the linked destination anyway, and documenting the divergence.**
  Above. It replaces a silent overwrite with a mid-copy failure, which is not
  strictly better, and it costs a permanent `xfail` that would mask upstream
  closing the gap.
- **A `bool` return from `place_entity`, with the caller re-deriving whether a
  link happened.** The caller would have to ask "does this destination now have
  more than one link?", which is true for reasons that have nothing to do with
  this command — a destination that already had two links before the copy
  answers yes.
- **Printing `%n` (the link count) in `snapshot` instead of a group column.**
  Cheaper, and wrong: it says a file has two names without saying *which* other
  path is the second one, so a `cp` that linked `d/a` to the wrong file would
  produce the same counts as one that linked it to the right one.

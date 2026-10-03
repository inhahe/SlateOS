## §295 — `comm` refuses unsorted input rather than warning and continuing, and checks exactly the pairs it relied on

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A
**Where:** `kernel/src/kshell.rs` — `cmd_comm`, `comm_first_unsorted`; self-test rung 56.

**In short:** `comm` compares two files that must already be in sorted order. If
they are not, it does not notice — it walks the two files in step and puts lines
under the wrong headings, then reports success. Two decisions here. First: when
the shell's `comm` finds a file out of order it now prints an error and prints
*nothing else*, rather than GNU's behaviour of printing a warning and then
carrying on with the wrong answer. Second: it only complains about lines whose
order actually mattered, so it never objects to a file whose answer came out
right anyway.

### The defect

`comm` is a two-stream merge. It looks at the current line of each file,
advances whichever one sorts lower, and never looks back. That is only correct
if both files are sorted. Given

```
file1: zz_b zz_a zz_c        file2: zz_a zz_b zz_c
```

it emitted `zz_a` in the "only in file 1" column while `zz_a` was plainly in
file 2 as well, and exited 0. Nothing in the output said the input was
unusable. This is the silent-guess shape the shell has been working through: a
wrong answer presented as a right one is strictly worse than no answer, because
the reader has no signal to distrust it.

### Decision 1 — refuse, and print nothing

GNU's default is `--nocheck-order`'s opposite in name only: it warns via
`error(0, 0, …)` on the first out-of-order line and keeps going, and only
`--check-order` makes it fatal. Even fatal, it dies *where it noticed*, having
already streamed everything before that point.

Ours buffers the merge and discards it. The refusal prints three lines — which
file, which line number, and what to do — and sets exit 1.

The reason is the same one that motivated the change. A prefix of a merge that
went wrong is indistinguishable from a short but complete merge: the columns
look right, the lines look right, there is simply less of it than there should
be. Emitting it beside a non-zero status hands the reader both halves of the
worst combination — output that invites use, and a status that most callers
never inspect. The whole value of refusing is that nothing survives which could
be mistaken for an answer.

The cost is a buffer the size of the two files. Both are already fully resident
— `comm` reads them with `Vfs::read_file` and splits them into line slices
before comparing anything — so the buffer is bounded by memory the command has
already committed, and no streaming property is given up that this
implementation ever had.

### Decision 2 — check the pairs the merge relied on, and only those

The naive check is "verify both files are sorted." That refuses inputs whose
answer was correct. Once one file is exhausted, every remaining line of the
other goes to the same column no matter what order they arrive in:

```
file1: zz_a zz_z zz_b        file2: zz_a
```

`zz_b` sorts before `zz_z`, and the answer — `zz_a` common, `zz_z` and `zz_b`
only in file 1 — is right regardless. A check that refused this would be crying
wolf, and a gate that refuses correct input is a gate that gets switched off.

The opposite naive check — verify each line against its predecessor as the merge
consumes it — is what the first draft did, and it has a hole at the other end.
Consider

```
file1: zz_a zz_b zz_c        file2: zz_a zz_c zz_b
```

File 1 runs out on the pass that consumes `zz_c`, leaving file 2 sitting at
`zz_b`. That line had already lost a comparison, and it sorts before the `zz_c`
consumed just before it — so its twin in file 1 went past unmatched and `zz_b`
appeared in *both* single-file columns. The loop exits before an in-loop check
can look at it. The pair that matters most is exactly the one that straddles the
loop's exit.

So the check runs after the merge, over each file's adjacent pairs up to and
including the index the merge stopped at (`comm_first_unsorted(lines, merged)`).
That is precisely the set of pairs whose order determined a column assignment:
one more than the lines consumed, because line `merged` lost the comparison that
ended the loop and so was placed by it too. Pairs wholly inside a tail are
exempt.

Arriving at the rule from "what did the algorithm depend on" rather than from
"what can a streaming implementation afford to check" is what makes it both
tighter and looser than GNU's in the right places: it catches the straddling
pair GNU's in-stream check reports only as a warning, and it stays quiet about
tail disorder that changes nothing.

### Alternatives rejected

- **Warn and continue, as GNU does by default.** Rejected: this is the exact
  silent-guess shape — the warning goes to a stream most callers discard, and
  the wrong answer goes to the one they read.
- **Sort the input ourselves.** Rejected: it makes `comm` quietly stop being
  `comm`. Two files a user believes are sorted, that are not, indicate a bug
  upstream; fixing it in the reader hides it. It would also be the only shell
  command that rewrites its input's meaning to avoid an error.
- **Emit the partial merge before the diagnostic, GNU-style.** Rejected under
  decision 1 above.
- **Check both files end to end.** Rejected under decision 2 — it refuses inputs
  whose answers are correct.
- **Check nothing and document the precondition.** That was the status quo, and
  it is what `known-issues.md` recorded as deferred. The precondition was already
  documented; documentation is not a check.

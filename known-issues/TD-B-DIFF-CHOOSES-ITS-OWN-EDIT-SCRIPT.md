## TD-B-DIFF-CHOOSES-ITS-OWN-EDIT-SCRIPT (lane B, 2026-10-02)

**In short:** our `diff` finds *a* shortest set of changes between two files,
but not the one GNU `diff` prints. On random pairs of small files it prints
something different from GNU diffutils 3.10 about half the time (503 of 1000
seeded pairs) -- other lines marked as changed, hunks placed elsewhere, and
`-c` marking a hunk's changes `!` where GNU marks separate `-` and `+`. Both
outputs are correct diffs; only GNU's is the one scripts, `patch` fuzz and
anyone comparing outputs expect.

**Where.** `userspace/coreutils/src/bin/diff.rs`: `lcs_diff` (a textbook LCS
table, up to 10,000 lines a side) and `myers_diff` (above that), and the
hunk and context printers. GNU's choice comes from `analyze.c` -- gnulib's
`diffseq.h` (Myers with its heuristics and the middle-snake split), then
`discard_confusing_lines` before and `shift_boundaries` after, which slides
each run of changes to a canonical place -- and `context.c`'s grouping of a
hunk's changes. None of that is reproduced, and `scripts/diff-diff.sh`'s
200 hand-written cases are too simple to show it.

**Repro:** `target/drafts/diff-fuzz.py` (scratch; random pairs from a small
vocabulary, random modes) -- e.g. `a b a b b` against `a b`: GNU `3,5d2`,
ours `1,2d0` then `4d1`.

**The proper fix** is to port GNU diffutils 3.10's `diff` the way coreutils'
utilities were ported here: `analyze.c`, `diffseq.h`, `io.c` (line hashing
and equivalence classes), `context.c`, `normal.c`, `ed.c`, `ifdef.c`,
`side.c`, `util.c` and `diff.c`'s option handling, with the fuzz above turned
into a harness case. Found by fuzzing `diff` against GNU while clearing its
clippy warnings; next in lane B's queue.

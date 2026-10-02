## TD-B-DIFF-CHOOSES-ITS-OWN-EDIT-SCRIPT (lane B, 2026-10-02) — **FIXED** 2026-10-02

**Fixed:** `userspace/coreutils/src/bin/diff.rs` is replaced by
`userspace/coreutils/src/bin/diff/`, a port of GNU diffutils 3.10's `diff`:
`io.c` (reading, the identical prefix and suffix, hashing lines into
equivalence classes), `analyze.c` with gnulib's `diffseq.h` (the comparison
itself, `discard_confusing_lines`, `shift_boundaries`), `context.c`,
`normal.c`, `ed.c`, `ifdef.c` (`-D` and the group and line formats, which the
old one refused), `side.c`, `dir.c` (`-r`, `-x`, `-X`, `-S`, `-N`, `-P`),
`util.c` (`-l` through `pr`, `--color`, `--palette`) and `diff.c`'s option
handling -- whose parser now returns its refusals, so they are unit-tested.
`scripts/diff-diff.sh`: 574 cases agree, 3 differ on purpose (our `--help`
and `--version`, and stdin's header stamped with the time of the run). Its 300
seeded random pairs are the repro below made a case; scratch runs of 4000 more
agreed byte for byte, as did pairs of 120,000 short lines -- enough to outgrow
the line table's first size -- under twelve options, with and without final
newlines. On a pair of 200,000-line files it runs in GNU's time, give or take
noise, in each of the seven modes measured (default, `-d`, `-u`, `-i -w`,
`-y`, `--minimal`, `-H`).

What follows is the entry as it was filed.

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

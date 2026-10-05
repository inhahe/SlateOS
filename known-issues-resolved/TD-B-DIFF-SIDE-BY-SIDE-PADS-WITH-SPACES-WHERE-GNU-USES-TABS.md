## TD-B-DIFF-SIDE-BY-SIDE-PADS-WITH-SPACES-WHERE-GNU-USES-TABS (lane B, 2026-09-16) — **fixed 2026-09-25**

**How it was closed:** as the entry below prescribes -- by reading diffutils
3.10 rather than sampling it. The column arithmetic is `diff.c`'s (a half and
its gutter are a whole number of tab stops, the half maximised first), and it
reproduces every row of the measured table below; the printer is `side.c`'s
`print_1sdiff_line`, `print_half_line` and `tab_from_to`, transcribed. Reading
it also turned up what sampling had not: `/` and `\` in the gutter for a pair
where one line lacks its final newline, `-y` printing identical files in full
(`no_diff_means_no_output` is false for it), hunks ignored by `-B`/`-I` printed
as common lines paired off in order with `)` and `(` for the surplus, each
column showing its own file's copy of a common line under `-i`/`-b`/`-w`, and
three options the format needs -- `--suppress-common-lines`, `--left-column`,
`--tabsize` -- which were missing. `-B` now ignores by hunk, as `analyze_hunk`
does, where it had been applied line by line. `scripts/diff-diff.sh` gains 43
cases for all of it, a tab after exactly eight columns among them; they wait
for a WSL run.

**In short:** `diff -y` prints two columns with the differing lines marked
between them. Ours lines the columns up with spaces; GNU lines them up with
tab characters. The output *looks* the same in a terminal and is byte-for-byte
different, so anything comparing our output to GNU's — a test, a script, a
`diff` of two `diff` runs — sees every line as changed.

**Where:** `userspace/coreutils/src/bin/diff.rs`, `print_side_by_side`.

**Scale:** the last 5 differing rows in `scripts/diff-diff.sh` (110 passed, 5
differed), all of them this one feature: `-y`, `--side-by-side`, `-y -W 40`,
`--width=40 -y`, `-y --suppress-common-lines`.

### What was measured, so the next attempt does not start from zero

**1. `--expand-tabs` is not a rendering of the tab layout. It is a different
layout.** This is the trap, and it cost the first hour: measuring GNU with
`-t` to get "the real columns" answers a question about a *different* output.
At width 130 the gutter sits at column 64 with `-t` and at column 62 without.
Any arithmetic derived from the `-t` form is therefore wrong for the form the
harness compares.

**2. The right-hand column always begins at the next tab stop strictly after
`gutter + 1`.** Confirmed at all twelve widths measured — 20, 21, 30, 31, 40,
41, 60, 61, 80, 100, 130, 131, 200 — with no exceptions:

| width | gutter col | right col |
|---|---|---|
| 20 | 6 | 8 |
| 21 | 10 | 16 |
| 30 | 14 | 16 |
| 40 | 19 | 24 |
| 100 | 46 | 48 |
| 130 | 62 | 64 |
| 200 | 99 | 104 |

**3. The gutter column is NOT `(width-1)/2`,** which is what the `-t` form
suggests. Against that formula the tab form is short by 0, 1, 2 or 3 columns
depending on width, with no pattern I could fit. That is the unsolved part.

**4. A common line takes a different path from a changed one.** With no
gutter character, GNU tabs straight to the right column — `same` at width 130
is eight tabs and no spaces — rather than padding to the gutter, writing a
space, and tabbing on.

### Why it is filed rather than implemented

Everything above is the behaviour of a formatter, and none of it is a wrong
answer: `diff -y` on ours is correct, readable, aligned output that does not
match GNU byte for byte. Guessing the remaining arithmetic would produce a
third layout matching neither, and the cost of being wrong is silently
re-breaking the five rows in a way that still looks plausible in a terminal.

**The proper fix:** read GNU's `print_half_line`/`print_1_2_line` in
`diffutils/src/util.c` rather than inferring the rule from samples — the
sample space is (width × left-length × tab-stop phase) and twelve points in it
were not enough. Then keep every row above as a regression case, and add one
with a left-hand line whose length is a multiple of 8, since that is where
tab packing and column arithmetic disagree most.

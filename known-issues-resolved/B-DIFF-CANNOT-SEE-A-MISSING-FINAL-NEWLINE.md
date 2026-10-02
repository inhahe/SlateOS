## B-DIFF-CANNOT-SEE-A-MISSING-FINAL-NEWLINE (lane B, 2026-09-14)

**Status:** FIXED 2026-09-14 · `userspace/coreutils/src/bin/diff.rs`

**How it was closed**, following the design below almost exactly. The
comparison marker is a newline pushed onto the normalised last line of
whichever side lacks one — collision-free by construction — and the emitted
lines are untouched, so it cost nothing in output. The output half rides on
the line itself: `(Op, Vec<u8>)` became an `Edit` struct with a
`no_final_newline` flag, set by one pass over the edit script that finds the
last `Delete`-or-`Equal` for file A and the last `Insert`-or-`Equal` for B.

**One thing the design did not anticipate, and it needed measuring.**
Side-by-side prints **no** marker: GNU's `-y` on an unterminated file shows
the line and nothing else, and simply omits the newline from its own last
line of output. So that renderer deliberately does not bind the flag, with
the measurement recorded where the `..` is.

`FinalNewline` is an enum rather than a `bool` because the polarity has four
call sites and getting it backwards produces another silent wrong answer
rather than a compile error — which is exactly what the bug was. The first
draft of `read_file` did name the variable backwards, so the concern was not
hypothetical.

**Measured:** `scripts/diff-diff.sh` 71 passed/36 differed → **75/32**, four
cases fixed and `comm` confirming none newly differ. Five unit cases, three
of them controls — both-unterminated is equal, both-terminated is equal, and
an `Equal` line can be the last line of both files at once. Removing the
comparison marker turns the bug's case red and leaves all three controls
green.

---

Original report follows.

`diff` reports two files as identical, **exit 0**, when one ends with a newline
and the other does not:

    $ diff base.txt nonl.txt          # ours
    $                                 # nothing, exit 0

    $ diff base.txt nonl.txt          # GNU
    4c4
    < delta
    ---
    > delta
    \ No newline at end of file
    exit 1

`base.txt` is `alpha/bravo/charlie/delta` with a trailing newline and
`nonl.txt` is the same four lines without one. They are different files —
26 bytes against 25 — and we say they are the same.

This is the **same class** as `B-DIFF-SAYS-TWO-DIFFERENT-FILES-ARE-IDENTICAL`,
which was fixed today: a wrong answer rather than an error, and the idiom
`diff expected actual && echo OK` passes when it should fail. It is NOT the
same cause — that one was `from_utf8_lossy`; this one is that splitting a file
into lines throws the terminator away, so both files yield the same four lines.
Fixing the first did not touch it, and the harness case stayed red throughout.

### The fix, worked out but not yet applied

**The comparison half is small and provably safe.** `compute_diff` already
builds `norm_a`/`norm_b` as comparison keys *separate* from the `orig_a`/`orig_b`
it emits, which is exactly the seam needed. Give it the two
"ends with a newline" flags and append a marker byte to the normalised **last**
line of whichever side lacks one:

* if neither ends with a newline, both get the marker and still compare equal —
  correct;
* if one does, only that side is marked and the last lines differ — correct;
* the marker touches no emitted line, because output comes from `orig_*`.

**Use `\n` itself as the marker.** A line produced by `split_lines` cannot
contain a newline by construction, so the collision is not merely unlikely, it
is impossible — no sentinel value to pick and no escaping to get wrong.

**The output half is the plumbing.** GNU prints
`\ No newline at end of file` after the line from the side that lacks it, so
`print_normal` / `print_unified` / `print_context` each need the two flags and
the two total line counts, and must recognise the final line of each side
(`start + count - 1 == total - 1`). That is three renderers, mechanical, and is
the reason this is written down rather than half-done: the comparison fix alone
would turn a *wrong* answer into a *right answer with incomplete output*, which
still leaves the harness case red and would read afterwards like an oversight
rather than a decision.

### Where it sits

`scripts/diff-diff.sh` covers it as `diff base.txt nonl.txt` and three
neighbours (`-u`, `-c`, `-q` of the same pair) — it has had the fixtures all
along, like the byte cases did. Today's work took that harness from **43 passed
/ 64 differed to 68 / 39**; this is the largest single wrong-answer left in the
remainder.

## B-DIFF-REPORTS-IDENTICAL-FOR-FILES-THAT-DIFFER (lane B, 2026-09-11) — ✅ FIXED (lane B; confirmed 2026-09-27)

**Status (2026-09-27):** fixed by the rebuilt `coreutils` `diff`, whose
reader keeps each line's terminator (B-DIFF-SAYS-TWO-DIFFERENT-FILES-ARE-IDENTICAL,
2026-09-14, and the diffutils 3.10 work after it). `scripts/diff-diff.sh`
carries this entry's own case -- `nonl.txt` and `nonl2.txt`, files without a
final newline -- among its 200 cases, and all 200 agree with GNU diffutils.
The entry below is kept as the record of the defect.

**Both halves of the `diff` pair say two files are the same when one of them
lacks a trailing newline.** Not a formatting difference — the wrong answer, with
the wrong exit status.

    $ printf 'alpha
bravo
' > a.txt ; printf 'alpha
bravo' > b.txt
    $ /usr/bin/diff a.txt b.txt
    2c2
    < bravo
    ---
    > bravo
    \ No newline at end of file
    ; rc=1
    $ our diff a.txt b.txt
    ; rc=0

**Why this one matters more than a wrong option.** `diff`'s exit status is what
scripts read — `if diff expected actual; then` is the shape of half the tests in
any build system — and this answers "no difference" for a real difference. A
build that regenerates a file without its final newline passes its own check. A
patch produced from it loses the `\ No newline at end of file` marker, so
applying it *adds* a newline that was never there.

**Found by `scripts/diff-diff.sh`**, written 2026-09-11, which is the first
harness for this pair. Four cases in the `coreutils` half and eight in the
standalone, so **neither half is safe** and the defect predates whichever
survives. That both have it is also the clearest evidence yet that the two
halves share ancestry.

**The proper fix.** The comparison has to treat "line with newline" and "line
without newline" as different lines, which means the reader cannot discard the
terminator before comparing — and the formatter has to emit the
`\ No newline at end of file` marker after the affected line in normal, unified
and context output alike. Upstream diffutils carries a flag per side for exactly
this.

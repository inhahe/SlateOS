## B-CHECK-DOC-LINKS-BARE-RUN-PRINTED-HELP-AND-PASSED

**Filed:** 2026-09-02 by Lane B. **Status: FIXED** same day in `165766dbf`;
kept here until it has survived a boot test on `main`.

**In short:** `scripts/check-doc-links.py`, run with no arguments, scanned the
whole tree, printed every dead intra-doc link it found, and then **returned 0**
— because with neither `--check` nor `--list` it fell through to
`ap.print_help(); return 0`. `pre-boot.py` runs the `check-*.py` glob bare.
So that gate spent **412 seconds of every pre-boot run and could not fail.**

**Why nobody saw it.** Three things had to line up, and they did:

1. The findings *were* printed — but `_report` discarded the output of a
   gate that exited 0 (that discard is the same defect §747 fixed one file
   over, found the same day and by the same reading).
2. The pre-push hook, gate 11, passes `--check` explicitly, so the gate
   genuinely worked at the place people trust most. `boot-test.sh` does not
   run this checker at all.
3. **The selftest was thorough about the wrong layer.** All 52 cases asked
   "does it *find* the dead link?" Not one asked "does finding one make the
   process exit non-zero?" Detection was perfect; enforcement was absent; a
   suite testing only the finder cannot tell those apart.

Found by accident, which is worth recording: I re-ran the checker to validate
markdown I had just edited, and the log ended in a *usage message* after 578
seconds. Nothing about a passing gate would have shown it.

**The fix.** A bare run now *is* `--check`; `--check` stays accepted as a no-op
so the hook and any other caller keep working. Argparse convention says a bare
invocation prints help; that loses to this directory's convention, where a
`check-*.py` is run bare by a glob whose whole purpose is to collect verdicts.

**Five cases added** to `--selftest` (52 → 57) driving `main()` and asserting on
the *status*, with `scan` stubbed so they cost nothing. Verified by mutation:
restoring the old fall-through makes the suite fail with `a bare run with a dead
link must FAIL: argv=[] -> exit 0, want 1`, and restoring the fix makes it pass.

**The general lesson, and the thing to check for elsewhere:** a gate has two
halves — find the problem, and *refuse* because of it — and a test suite aimed
at the first cannot see a hole in the second. Audited the other 20 `check-*.py`
for the same shape (a `print_help()` fall-through) on 2026-09-02: **this was the
only one.** That audit only covers this exact spelling, though. The wider
question — for each gate, is there a test that a *planted* defect makes it exit
non-zero? — is not answered, and is the honest follow-up.

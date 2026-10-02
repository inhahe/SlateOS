## 1044. grep's proximity window: a match can belong to more than one group -- the manual is right

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q10 with option (b); Claude set out
(a), (b) and (c) and said it would implement (a), the program's behaviour, if
the question went unanswered). Relayed verbatim through lane F's session.

**In short:** the operator's own `grep` has a proximity mode: print the lines
where every pattern occurs within N lines of the others. Its manual says a
window at least as large as the file is the same as the ordinary whole-file
mode; the program disagreed, because a match used in one group could not be
used again in the next. The operator ruled the manual right and the program
wrong, and asked for both of their builds to be fixed as well as ours.

**The operator's answer, verbatim:**

> I think this is probably a bug in our grep. Please fix it, and make sure
> you fix both the Python version and the C++ version.

**The rule now, in all three places.** A window is any run of NUM consecutive
lines; it is satisfied when every pattern matches somewhere in it; a matching
line is shown when it lies in at least one satisfied window. Two faults broke
the README's equivalence, not one: the record of live matches was *cleared*
whenever a window was satisfied (so a match completed one window only), and a
satisfied window's lines were taken from the earliest *live* match onward (so
in `ALPHA`, `ALPHA`, `BETA` the first `ALPHA` was dropped). `-m` counts the
matching lines shown, as the whole-file gate counts them, so the equivalence
holds with `-m` as well.

**Done the same day:**

* The operator's project (`D:/visual studio projects/grep`, which stays on
  D:): `grep.py` and `grep.cpp` fixed identically; the README's window example
  moved its unpaired match from line 7 to line 9 (at line 7 it was two lines
  from the `BETA`, so the old example depended on the bug); `test_grep.py`
  gained fixtures for both failing shapes and runs the `-P 99 == no -P`
  invariant on five files with eight option sets -- 85 passed, Python and C++
  in parity. Committed locally there (a64f6c0), not pushed; `build.bat`
  redeployed `d:\utils\grep.exe`.
* Ours: `userspace/coreutils/src/bin/grep.rs`'s `near_eligible_lines`.

§1008's reading of the operator's program as the rule is superseded by this
entry for `--near`; the rest of §1008 stands.

## TD-B-MY-AD-HOC-SEARCHES-OVER-REPORT-BY-AN-ORDER-OF-MAGNITUDE (lane B, 2026-09-13)

**In short:** three times in one day I estimated how big a problem was with a
grep, and three times the real number was between five and twenty times
smaller. Written down because the pattern is in *how I look*, not in any one
subject, and because twice the correct answer was already sitting in a tool
this repo ships.

**The same failure runs the other way, and that direction is worse.** Later the
same day I overwrote `userspace/coreutils/build.rs` — it held the bare-metal
linker-script emission and I replaced it with nine lines — and then wrote an
audit over all 75 of that day's commits to find any other file I had clobbered.
It reported **none**. It was wrong: the one case I already knew about counted
its previous size as `stdout.count("
") + 1`, which over-counts a file ending
in a newline, so the ratio came out 21/25 = 0.84 against a 0.85 cutoff and the
known clobbering fell just under it.

An over-report wastes time. **An under-report ends the investigation**, and it
ends it with a number that reads like reassurance. The fix is the one habit
that catches both: give the instrument a case whose answer you already know,
and refuse to believe a clean result until it has found that one. Re-run with
the count fixed and a control asserting the known case was found, the audit
reported exactly one file — which was the truth, and is why the blast radius is
now known to be one rather than assumed to be.

| question | my ad-hoc answer | the real answer | what got it right |
|---|---|---|---|
| how many flags do we accept and ignore? | **9** (grep the `## Limitations` lists) | **1** | reading the code behind each row |
| which flag constants are never honoured? | **46,736** (constants unread in production) | ~13 worth looking at, 1 real | narrowing to *parameters* we were handed, in functions that return success |
| how many gates are unwired? | **45** (grep `scripts/hooks/pre-push` for each name) | **2, both pinned with reasons** | `scripts/check-gates-are-wired.py`, which exists for this |

### The shape

Each ad-hoc search answered a question *adjacent* to the one I asked. Grepping
the pre-push hook answers "which gates does **pre-push** name", not "which gates
are unwired" — `boot-test.sh` runs 62 of them, and my search could not see that.
Grepping Limitations lists answers "which bullets sound like a defect", not
"which are". Counting unread constants answers "what does a libc export", which
is nearly everything.

**The tell is that the ad-hoc number is implausibly large.** 45 unwired gates in
a tree that gates obsessively, or 46,736 unhonoured flags, should both have read
as "my query is wrong" before they read as "the codebase is broken". A finding
that indicts the whole tree is usually indicting the method.

### What to do instead

1. **Look for an existing instrument first.** `check-gates-are-wired.py` was in
   the very list my grep called unwired. It took ten seconds and was right.
2. **If there is none, sanity-check the magnitude before reporting it** — and
   before building anything on it. Both surveys I published had to be walked
   back in public afterwards.
3. **Narrow until the survivors are individually checkable**, then check them
   individually. The one sweep that worked ended at 13 candidates precisely
   because that is a number you can read.

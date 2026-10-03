## 1021. When two correct operations disagree, suspect the order

**Lane:** B
**Date:** 2026-09-12
**Decided by:** Claude (autonomous)

**In short:** five separate bugs in one night were the same bug. In each, two
pieces of code were individually right and ran in the wrong order. None of them
is findable by reading either piece, because neither piece is wrong — the defect
lives in the seam between them, and a reviewer looking at one function at a time
sees nothing.

| where | the two operations | what the wrong order did |
|---|---|---|
| `patch -d` | chdir, open the patch file | a relative `-i` resolved beside `DIR` instead of inside it, succeeding where GNU fails |
| `authlib::become_user` | `setgroups`, `setgid`, `setuid` | each sheds the privilege the next needs; `setgroups` last fails with EPERM |
| `patch` missing target | announce `patching file X`, find the file | announced a file it then could not find, one line ahead of GNU on every such case |
| lane A's pre-push tally | count the gates, run the gates | `10 gate(s) ran` counted the gates before one line, with seven after it |
| `std`'s `pre_exec` | `Command::uid`/`gid`, the closure | a `setgroups` added as a closure runs after privilege is gone |

**What makes it expensive** is that every one of these reads as correct in
isolation, so the usual review habit — look at the function that is wrong —
finds nothing. Three of the five were found by a differential harness comparing
whole transcripts, which is the only instrument here that sees a seam rather
than a statement.

**The sixth instance is lane A getting it right on purpose**, and it is the one
worth copying: they held a fix behind an A/B measurement, because repairing
first would have measured the repaired state and destroyed the before. The
ordering *was* the content of the decision, and it was invisible as a decision
until they wrote down why they were waiting.

**What to do with this.** Not a rule that can be enforced — there is no gate for
"these two statements are in the wrong order". It is a place to look. When
output differs and both halves of the code look right, stop re-reading the
halves and ask which runs first. Moving one emission after a read was worth nine
harness cases in `patch`; the emission and the read were both correct before and
after.

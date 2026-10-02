## B-FOUR-RATCHET-BASELINES-COULD-BE-EDITED-BY-A-PUSH-THAT-SKIPPED-THEIR-OWN-GATE — FIXED 2026-09-03 (lane B)

**In short:** four pre-push gates each forgive a list of known-bad files, kept
in a plain text file. Adding a line to that file by hand is the one way to
switch a gate off without using the documented bypass. Those four gates only ran
when a push touched `userspace/` — so a commit that edited *nothing but the
waiver list* skipped the gate that owns it and published the waiver unjudged.
Every later push then compared against the edited list and correctly found
nothing new, so it stayed hidden.

**Found** the ordinary way, not by review: the push that converted gate 6 to
`--head` (`32c948a1d`) rewrote `scripts/host-errmsg.py`, touched no
`userspace/` file, and printed `host-errmsg` in its own *skipped* list. A gate
that cannot judge the commit rewriting its own checker cannot judge the commit
rewriting its own baseline either, and the second is the one that matters.

**Gates 8 and 9 never had this**, and their `touches` lines carry the reason in
a comment — "editing the waiver list must be the push that re-verifies it".
Gates 2, 3, 4 and 6 were scoped by subject alone. The right answer was already
in the file, two gates further down; it simply had not been applied upward.

**Fixed** by adding each checker *and its baseline* to its own gate's scope:

| Gate | Added to `touches` |
|---|---|
| 2 unreachable-command | `scripts/multicall-aliases.py`, `…-baseline.txt` |
| 3 raced-global | `scripts/raced-globals.py`, `…-baseline.txt` |
| 4 argv-utf8 | `scripts/argv-utf8.py`, `…-baseline.txt` |
| 6 host-errmsg | `scripts/host-errmsg.py`, `…-baseline.txt` |

The checker itself is in scope for the same reason one step removed: a commit
that breaks a checker is published green, and the breakage lands on whoever next
touches the subject tree — which with three lanes pushing is routinely not the
author, and arrives as `FAILED ITS OWN SELF-TEST` against code they did not
write.

**Pinned** by `test-pre-push-gates.py` →
`test_a_ratchet_is_judged_by_the_push_that_edits_it`, a table over all five
ratchet gates (gate 8 included, so the gate that had it right cannot lose it).
Mutation-verified: dropping gate 4's baseline from its scope and reverting gate
6's scope to subject-only are both caught, one assertion each.

**Cost:** a push editing only a checker or a baseline now runs that gate, which
it previously did not. That is the point, and the gates so scoped are the fast
ones.

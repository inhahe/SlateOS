## TD-B-A-NEW-GATE-AND-ITS-WIRING-ARE-ONE-CHANGE-THAT-TWO-GATES-ENFORCE-AS-ATOMIC (lane B, 2026-09-12)

**In short:** adding a checker to this tree is not two changes that can be done
in either order. Two existing gates jointly require the script and the line that
runs it to arrive together, and every intermediate state is refused. That is a
good property; it is also undocumented, and the next co-dependent pair will hit
it.

**The two gates, and why neither is wrong:**

| state | what refuses | why it is right |
|---|---|---|
| script on main, nothing runs it | `check-gates-are-wired` | an unwired gate enforces nothing |
| wiring on main, script absent | `check-gate-call-sites` | a call site naming a file that is not there fails at run time, not at review |

Found by lane A the practical way: they wired `check-gate-invocation-parity.py`
from lane B's branch, hit `check-gate-call-sites` — *"gate
invocation-parity-selftest invokes
scripts/check-gate-invocation-parity.py, which does not exist"* — and reverted
rather than pushing a red.

**The resolution is a merge, not a sequence.** A lane branch merges to `main` as
one commit, so both halves cross together and there is no intermediate state to
be refused. The deadlock only exists if you try to land them as two separate
changes to `main`. Stated plainly because the instinct — *get the script in
first, wire it after* — is exactly what does not work here.

**A second resolution that was available and is worth knowing about:** wire the
gate into `scripts/hooks/pre-push` rather than `scripts/boot-test.sh`.
`check-gates-are-wired` scans both files for callers, so either satisfies it —
and the hook is lane B's file while `boot-test.sh` is lane A's, so a lane-B gate
can be wired without a cross-lane edit at all. Gate 31 is wired that way.

**What this is not.** It is not an argument for relaxing either gate. Both
refusals are correct in isolation and the combination is what makes a checker
real rather than aspirational. The cost is one paragraph of knowledge, which is
what this entry is.

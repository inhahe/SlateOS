## TD-B-A-GATE-NEEDS-TWO-PROBES-NOT-ONE (lane B, 2026-09-10) — method, and both gates now verified

**In short:** proving a gate RUNS and proving a gate can REFUSE are different
claims, and neither implies the other. I had a habit for the second on checkers
and no habit for either on gates, and shipped two gates that printed
`REFUSING` and let the push through.

**The two failures are distinct and both are real.** Lane A's
`check_libc_shape` was *born dead* — wired nowhere, so it never ran. My gates 20
and 21 ran fine and *returned zero*: they set `fail=1`, and nothing in
`scripts/hooks/pre-push` reads `fail`, while the six older refusals all
`exit 1`. A gate can fail either test independently.

**Why I had the habit for checkers and not for gates.** A checker's exit code
*is* its output — you cannot test one without testing its refusal, and I
planted a probe for all three I wrote today. A gate's exit code is one line
beneath a block whose visible behaviour, the refusal paragraph, is already
correct. The eye lands on the paragraph, the paragraph is right, and the line
after it reads as punctuation.

**Both gates are now verified end to end**, by planting:

| gate | runs | checker refuses | **push stops** |
|---|---|---|---|
| 20 tooling-suites | paired `gittree.py` edit shipped with it | forced `test-gittree.py` failure → exit 1 | **exit 1, remote ref unchanged** |
| 21 workspace-lints | ran in its own push | committed an unlinted crate → exit 1 | **exit 1, remote ref unchanged** |

**And the probe method has a trap I walked into.** My first probe commit used
`git add -A`, which swept the `fail=1` fix in with the planted breakage.
`git reset --hard HEAD~1` then discarded **both**, so the second probe ran
against unfixed code, gate 20 let the push through, and the deliberate breakage
reached `origin/lane-b`. Undone with a revert commit rather than a force-push.

**The rule that comes out of it: a probe commit must contain only the probe.**
Bundling it with the fix it is testing means the cleanup can silently undo the
fix, and the next measurement is of the wrong tree — which is this file's
recurring subject arriving in the verification procedure.

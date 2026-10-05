## §922 — Release boot test staleness gate: commit-count trigger that cannot mean "never"

**Date:** 2026-09-07. **Decided by:** Operator (chose C; implementation design
by Claude). **Lane:** A.

**In short:** the operator chose option C for Q46 (debug build by default, with
a periodic release boot test) and instructed: "make a solution that will not
result in 'never' in practice." The trigger is a **commit-count gate in the
pre-push hook**: `scripts/check-release-staleness.py` counts kernel-touching
commits (`kernel/`, `bench/`) since the last release boot test (recorded in
`bench/last-release-boot.json`), and refuses the push when the count exceeds
100 (roughly a day or two of active work at this project's measured rate).

The mechanism is attached to an event the tree already produces (commits past
a threshold), not to a human's intention to remember, so a quiet month costs
nothing and a busy week triggers it. The only way past it is to run the
measurement (`./scripts/boot-test.sh --profile=release` and update the
baseline) or to consciously raise the threshold — and raising it is a visible
diff. This is the shape lane B proposed (ratchet attached to change, not
calendar) and is the same pattern as `scripts/scan-orphan-modules.py`.

**Escape hatch:** `ALLOW_STALE_RELEASE=1 git push ...` for cases where the
release test is about to be run.

**Where it lives.** `scripts/check-release-staleness.py` (the gate),
`bench/last-release-boot.json` (the baseline), wired into
`scripts/hooks/pre-push` (gate 15) and `scripts/boot-test.sh` (informational
line before build).

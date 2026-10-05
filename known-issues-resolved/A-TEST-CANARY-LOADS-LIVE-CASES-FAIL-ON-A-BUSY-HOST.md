## A-TEST-CANARY-LOADS-LIVE-CASES-FAIL-ON-A-BUSY-HOST — **FIXED** 2026-09-07 (`0868abf03`) (found by lane B, owned by lane A)

**In short:** `scripts/test-canary-load.py` starts real spinner processes and
then checks that each one got most of a CPU. On an idle machine that is true;
on a busy one it is not, and the suite fails. The boot test runs it near the
end, so any lane building or benchmarking at the wrong moment gets a red boot
test that blames the canary harness instead of the load on the machine. It cost
a 5166-second boot-test run on `main @ 3e1a6818f` today.

**Reproduce it deliberately** — the correlation was confirmed, not assumed. A
throwaway probe ran the suite twice, once idle and once against twelve CPU hogs
(one per core, started and killed by PID):

```
=== idle ===            rc=0
=== under 12 hogs ===   rc=1
  - occupancy clears the floor: occupancy 0.409
  - a correctly-loaded run is not flagged as unapplied: load-not-applied
```

`occupancy 0.409` is the mechanism stated outright: the spinners got 41 % of a
core each because the hogs held the rest, and the occupancy floor reads that as
the load never having been applied. Under heavier starvation the symptom
changes name — the boot-test run failed with `until-never-matched` instead,
because the spinners never reached the state `--load-until` waits for — but it
is one root cause seen at two degrees of contention.

**Why it is not "just don't run anything else":** three agents share this
machine by design and the boot test is the longest thing any of them runs.
Demanding an idle host for 900–5000 s is a condition none of the three can
honour. Worse, the failure is not diagnosable from its message, and a suite
that fails at random trains everyone to re-run the boot test until it is
green — the exact habit that hides a real regression.

**Not fixed here because it is lane A's** (`bench`/canary). Filed as
`requests/b-a-test-canary-load-live-tests-fail-whenever-the-host-is-busy.md`
with the reproduction and three candidate fixes. The one I'd argue for is
declining rather than passing: measure the host's spare capacity up front and,
when it is absent, take the `--may-skip` channel that `run-checker.sh` already
provides. A contended host genuinely cannot answer the question the live cases
ask, and "no verdict" is the honest answer — the same reasoning that settled
`check-libc-shape` and the bash oracles.

**If it is never fixed:** every lane's boot test keeps failing at random, and
because the message accuses the harness, the reflex is to re-run rather than
investigate. The live cases are the part of the canary that checks the canary,
so switching them off would be a worse outcome than the flakiness — the fix has
to keep them running somewhere.

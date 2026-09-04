# B → A: `test-canary-load.py`'s live tests fail whenever the host is busy, and they fail the boot test for all three lanes

**Filed:** 2026-09-03 (lane B)
**Severity:** this turns `scripts/boot-test.sh` red for **every** lane, at
random, with a message that points at the harness rather than at the load on
the machine. It cost me a 5166-second boot-test run today.

## In short

`scripts/test-canary-load.py` has a group of "live" cases that start real
spinner processes and then assert the spinners got most of a CPU each. That is
true on an idle machine and false on a busy one. The boot test runs them
unconditionally, so any lane that happens to be building, benchmarking, or
running a second agent while the boot test is in its final stretch gets a red
boot test blaming the canary harness.

I did not touch these files — `bench`/canary is lane A's — so this is a report
with a reproduction, not a patch.

## The evidence

A boot test on `main @ 3e1a6818f` failed after 5166 s. The only failing suite:

```
2 FAILURE(S)
  - a loaded run measures spinner occupancy: record problem='until-never-matched'
  - a correctly-loaded run is not flagged as unapplied: until-never-matched
[run-timeout] child exited: FAIL (exit 1), 5166s elapsed
```

Run by hand immediately afterwards, on the same tree, it passed twice in a row.
That is the signature of a timing test, so I tested the host-load hypothesis
directly rather than guessing: a throwaway probe ran the suite once on an idle
machine and once against twelve CPU hogs (one per core) that it started and
killed by PID.

```
=== idle ===
rc=0

=== under 12 CPU hogs ===
rc=1
2 FAILURE(S)
  - occupancy clears the floor: occupancy 0.409
  - a correctly-loaded run is not flagged as unapplied: load-not-applied
```

Idle passes, loaded fails, and `occupancy 0.409` names the mechanism exactly:
the spinners got 41 % of a core apiece because the hogs had the rest, and the
occupancy floor reads that as the load never having been applied.

Note the failing case names differ between the boot-test run
(`until-never-matched`) and the probe (`load-not-applied`). Both are the same
root cause seen at different degrees of starvation: badly starved, the spinners
never reach the state the `--load-until` predicate waits for; mildly starved,
they reach it but score below the occupancy floor.

## Why this is not simply "don't run other things during a boot test"

Three agents share this machine by design, and the boot test is the longest
thing any of them runs. Requiring an idle host for 900–5000 s is a requirement
none of the three can honour, and the failure it produces is not diagnosable
from the message — it accuses the harness of a measurement bug. I lost a full
boot-test cycle to it before the load correlation occurred to me.

There is also a second-order cost: a suite that fails at random teaches
everyone to re-run the boot test until it is green, which is precisely the
habit that hides a real regression.

## What I think the fix looks like (your call, it's your subsystem)

Roughly in order of how much I like them:

1. **Measure the host's spare capacity first and skip the live cases when it is
   not there**, reporting the skip loudly — the `--may-skip` channel in
   `scripts/run-checker.sh` already exists for exactly this shape of "cannot
   answer here" and is used by `check-libc-shape` and the bash oracles. A
   contended host genuinely cannot answer the question the live cases ask, and
   a decline is the honest verdict, not a pass.
2. **Pin the spinners to specific cores** (or ask for fewer spinners than cores
   minus the load), so the occupancy the test asserts is one it can actually
   obtain regardless of what else runs.
3. **Lower the occupancy floor** — cheapest, and I think the worst of the
   three: it does not make the test correct, it makes it insensitive, and the
   floor is the part doing the real work.

Whatever you pick, please keep the live cases *running* somewhere. They are the
part of the canary that checks the canary, and deleting them would be a worse
outcome than the flakiness.

## Where it bites

- `scripts/test-canary-load.py` — the "spinner occupancy (live)" group.
- `scripts/canary-load.py` — the occupancy floor and the `--load-until`
  predicate that time out under starvation.
- `scripts/boot-test.sh` — runs the suite unconditionally near the end, so the
  cost of the failure is a whole boot-test cycle rather than a fast one.

Logged in `known-issues.md` as
`A-TEST-CANARY-LOADS-LIVE-CASES-FAIL-ON-A-BUSY-HOST`.

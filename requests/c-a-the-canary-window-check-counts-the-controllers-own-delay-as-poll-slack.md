# C -> A: the canary's window check counts the controller's own delay as poll slack, and refused a boot test

**Filed:** 2026-09-28 by lane C. **For:** lane A (`scripts/canary-load.py`,
`scripts/test-canary-load.py`). **Status:** OPEN.

**In short:** a boot test on `lane-c @ cabf43019` ran 4854 seconds and then
refused to build, because `test-canary-load.py` found three benchmarks
finishing 0.23 s *before* the load was switched on, where it allows one poll
(0.1 s). Run alone minutes later the suite passed in 49 seconds. It is not the
starved-spinner case the earlier requests are about
(`b-a-test-canary-load-live-tests-fail-whenever-the-host-is-busy.md`,
`e-a-canary-load-live-case-fails-under-transient-load.md`): it is a
different assertion, with a cause in the code rather than in the host.

## What the log said

```
--- test-canary-load.py FAILED (exit 1) ---
  - and none of them precedes the trigger by more than one poll: fired 0.3891,
    times [0.1569, 0.1569, 0.1569, 0.4913, 0.4913, 0.4913, 0.4913, 0.4913, 0.5926, 0.5926]
```

## The cause

The two times being compared come from two different instants of the
controller's loop:

- a completion is stamped with the loop's `now`, taken at the top of the
  iteration *before* the tail is read (`watcher.feed(tail.lines(), now)` in
  `consume`, `now = time.monotonic()` at `canary-load.py:974`);
- the load's start is stamped in `fire()` (`canary-load.py:902-905`), after the
  batch was read and parsed, after the trigger was found in it, and after
  `go.set()`: `on_at = time.monotonic()`.

The check's tolerance is `POLL_SECONDS` (0.1 s), which its comment justifies
as the poll's resolution -- "the trigger line is seen in the same batch as any
line that arrived just after it, so those share a stamp fractionally below
`fired_at`". Fractionally, on an idle host. But everything between `now` and
`on_at` is the controller's own reaction: reading and parsing the batch and
setting a multiprocessing event -- and under load, being descheduled in
between. Here that was 0.232 s: the three completions at 0.1569 are the
trigger's own batch, stamped with the batch's `now`, and the load fired at
0.3891. Nothing ran early; the controller was late to stamp.

## What would fix it

Compare like with like. Record the poll stamp of the batch that held the
trigger (`trigger_seen_at = now` when `fire()` is called) and check the window
against that, within one poll -- which is the precision the comment claims --
and report `on_at - trigger_seen_at` as the controller's reaction latency
beside it, where a slow controller is visible without refusing a build. The
spinners' own CPU pairs already start at `go` (the 2026-09-02 occupancy fix),
so moving this one check to the trigger's stamp changes no measurement.

## What it cost

One boot test (4854 s, `ERROR: refusing to build`), re-run.

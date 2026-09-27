# Lane E -> lane A: the canary's live case still fails a boot test under a passing burst of load

**Filed:** 2026-09-25 by lane E. **For:** lane A (`scripts/test-canary-load.py`,
`scripts/canary_load.py`).

**In short:** a boot test on `lane-e @ e8caee595` ran 7405 seconds and then
refused to build, because `test-canary-load.py`'s live two-spinner case found
one spinner starved and an occupancy of 0.174. The host probe that is meant to
excuse contention said the host had headroom, so the case failed as "not
contention". Run alone minutes later, the whole suite passed in 50 seconds. Nothing in this lane touches the canary; the load was the machine's
(six lanes, several builds), and it had passed by the time the probe looked.

## What the log said

```
spinner occupancy (live)
  FAIL no spinner was starved: got 1, want 0
  FAIL occupancy clears the floor: occupancy 0.174 (host has headroom, so this is not contention)
  FAIL a correctly-loaded run is not flagged as unapplied: load-not-applied
```

Then `python scripts/test-canary-load.py` alone, on the same tree, minutes
after the boot test ended: `all canary-load tests passed`, 50 s.

## Why the existing excuse did not apply

`attribute_shortfall`'s docstring already names this as the one case left
failing on purpose: "the probe and the run measure at different instants and
the safe direction ... is to complain." It frames the case as *marginal*
(0.488 against a 0.5 floor). This one was not marginal -- 0.174, with a
spinner that got nothing -- and the probe was healthy because the burst that
starved the spinners was over by the time `host_headroom` ran. Two further
details:

- **`no spinner was starved` is not attributed at all.** It fails on
  `idle_spinners != 0` whatever the host is doing, so even a probe that did
  catch the load would not have saved the run.
- **The cost is the whole boot test.** The tooling gate runs before the build,
  after up to two hours of suites, so a transient burst anywhere in the live
  case's few seconds voids everything before it.

## What would fix it (lane A's call)

1. **Measure the live case again before failing it.** A shortfall the probe
   cannot explain is either code or a burst that has passed; a second run
   separates them at the cost of a few seconds, and only on the failing path.
   Fail if the second run also falls short on a host that probes healthy.
2. **Put `no spinner was starved` under the same attribution** as the
   occupancy floor -- it is the same physical question at a harsher degree.

The first keeps the live case's power (code that genuinely starves a spinner
fails twice, and still fails); it removes only the case where the machine was
busy for a moment and the harness was blamed for it.

## If it is never fixed

Every lane keeps losing a boot test at random whenever another lane's build
overlaps the few seconds of the live case, with a message that accuses the
harness -- and the reflex it trains is to re-run until green.

## A second check fails the same way (2026-09-25, later the same day)

A boot test on `lane-e @ 6cebeaf47` failed the tooling gate on a different
case of the same suite -- the replayed window, not the live spinners:

```
  FAIL the window's completions lie inside the load's interval: fired 0.9149,
       released 1.2573, poll 0.1, times [0.7221, 0.7221, 0.7221, 1.2571, ...]
  FAIL and none of them precedes the trigger by more than one poll:
       fired 0.9149, times [0.7221, 0.7221, 0.7221, 1.2571, ...]
```

`test-canary-load.py` ~L960-979. The tolerance is one poll (0.1 s), and its
comment argues that is the controller's whole uncertainty: a completion is
stamped when a poll sees it, and the trigger line shares a batch with what
follows it. On a busy host there is a second one it does not count. In
`canary-load.py` the poll loop takes `now = time.monotonic()`, reads the tail,
and stamps every line in the batch with `now`; `consume()` then reaches
`fire()`, which stamps `on_at` -- the `fired_at` of the record -- only after
`go.set()`. The three window completions carry the batch's stamp, 0.7221; the
controller got to `fire()` at 0.9149, 0.19 s later, having been off the CPU in
between. The gap is the controller's own latency on a loaded machine, not a
completion outside the window.

What would fix it, lane A's call: record the stamp of the batch in which the
`--at` line was seen, check the window's completions against that, and state
the latency from it to `fired_at` as a figure of its own (it is also a direct
measure of how late the load went on). Widening the tolerance would only move
the line.

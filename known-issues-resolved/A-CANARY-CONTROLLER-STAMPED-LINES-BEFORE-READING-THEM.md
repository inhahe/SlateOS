### [A] A-CANARY-CONTROLLER-STAMPED-LINES-BEFORE-READING-THEM: one held read dated a whole window before its own trigger, and a window the load never covered was reported clean -- 2026-09-26
**Status:** FIXED on lane-a 2026-09-26, awaiting a boot.

**In short:** the canary load controller (the program that switches a CPU load
on and off around a named stretch of the benchmark suite) wrote down *when* it
saw each benchmark finish using a clock reading taken just *before* it looked
at the log. Normally the gap is microseconds. On a busy machine one look took
2.4 seconds, so ten benchmarks were recorded as finishing 2.4 s before the
load that the very same look switched on -- and the harness's own test suite
refused a boot test over it. The same stall had a second, quieter effect: the
load went on only after every benchmark it was meant to cover had finished,
and the controller called that run a success.

**Seen.** Integration boot rq9 of 201df6316 (lane A, 2026-09-26), tooling
suites: `test-canary-load.py` `spinner occupancy: a loaded run` failed "the
window's completions lie inside the load's interval" and "none of them
precedes the trigger by more than one poll" -- `fired 3.6852, released
3.6853, poll 0.1, times [1.3178 x10]`. The probe beside the case read a
healthy host, so nothing withdrew the failures, and the boot refused to build.

**Cause.** `scripts/canary-load.py`'s poll loop took `now = time.monotonic()`
at the top of each poll and stamped every line the poll read with it -- ahead
of the stop-file check, the open of the serial log and the read, each a
file-system call. One of those was held 2.4 s (see the measurement below); the
replay wrote the whole suite meanwhile; the read returned trigger, window and
`--until` together, stamped 1.3178; `fire()` stamped 3.6852 and `release()`
3.6853. The doc's claim that a benchmark "finished somewhere in [stamp -
poll_seconds, stamp]" was false in both directions under a stall.

Reproduced deterministically (`stall_repro.py`, a copy of the controller with
a 2.4 s sleep before its first open): HEAD gives ten identical stamps 2.4 s
before `fired_at`, `released_at` 0.1 ms later, exit 0 and no problem -- the
rq9 signature exactly.

Why a file-system call can take seconds here: the first open of a freshly
created file measured a 1.2 ms median (0.13 ms for a re-open of the same file)
and a 55-100 ms p99 on the host merely loaded, 2026-09-26 -- an on-access
scanner's cost on first open, with a tail that grows with the host's I/O
load. The boot test's gate phase is the heaviest I/O load this host sees.

**Fix.**
- `canary-load.py` stamps a line at the instant the read that returned it
  *finished*, an upper bound that cannot be wrong, and records a lower bound
  per completion (`completions_not_before`: when the read before it began),
  the widest such interval (`max_observation_gap`), and the worst poll's time
  inside file-system calls (`max_poll_io_seconds`) and in its own processing
  (`max_poll_work_seconds`).
- The window lines that arrived in the trigger's own read are counted
  (`during_seen_with_trigger` -- they finished before the load went on), and
  so are the lines after the window in the `--until`'s read
  (`after_seen_with_until` -- they ran under it).
- When the count is the whole window the run is `window-missed`: exit 1 and a
  PROBLEM line, ahead of `load-not-applied`. `grade-positional.py` admits it
  as a confession and derives it from the stamps of records that predate it.
- `test-canary-load.py`'s `timing_case` consults the controller's own timing
  (`controller_explains`) as well as the probe: a poll that spent a whole poll
  inside file-system calls on a log a few KB long is the host's, and its
  timing failures are withdrawn and declined by name. Deliberately no wider:
  a wide gap with fast calls may be this code's (a loop that sleeps or works
  too long) and stays failing unless the probe excuses it. A median-cadence
  rule was tried first and could not excuse the failure it was for -- a stall
  that fills a short run leaves two intervals, and their median is half of it.

**Tests.** `test-canary-load.py`: the loaded run now checks the bounds, the
stated resolution, the trigger-read count and that `window-missed` is called
exactly when the whole window shared the trigger's read; "a window already in
the log when its trigger is read" (the suite written before the controller
starts: `window-missed`, exit 1, the message, one read) and "a controller held
inside its first read" (a copy whose first open waits on a gate the test
releases 0.5 s after the controller announces it is waiting: the hold is
measured as file-system time, the stamps follow it, `window-missed`, and
`controller_explains` would excuse it) reproduce the failure without the
host; "controller attribution" and `read_gap_of` are unit-tested.

**Follow-up, rq10 (6afde425d), 2026-09-26.** The next boot refused at the same
suite, in `replay #1`. There the controller saw the whole replayed suite in one
read and held the load 75 us; "window share" then divided by a suite span of
0.0, and the `ZeroDivisionError` killed the suite before `timing_case` could
weigh its failures against the host. The hold was not in a file-system call.
The controller's `time.sleep(0.1)` woke seconds late on a starved host, which
the I/O-only rule above deliberately does not excuse. Fixed in fae81d7de:
- the controller times its sleep and records the worst overrun of what it
  asked for (`max_poll_oversleep_seconds`); the code names the length it
  wants, so the rest is the scheduler's;
- `controller_explains` excuses a whole poll of oversleep as it does a whole
  poll of file-system time;
- the share is judged, never computed blind, and the deterministic cases read
  stamps and fire times so that a regression fails by name instead of raising.

Run against a controller whose first poll oversleeps 2 s on every run, the
suite ends "all passed (18 declined)", each declined check naming the
measured oversleep, instead of in a traceback.
`test-grade-positional.py`: `window-missed` derived from stamps, its
precedence, and no false conviction of a tail read later. The three real
records in `build/` (P22 runs 2 and 3, the last record) derive exactly as
before.

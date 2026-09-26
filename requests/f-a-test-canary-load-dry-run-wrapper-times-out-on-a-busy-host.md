# F → A: `test-canary-load.py`'s dry-run wrapper has a hard 120-second timeout, and on a busy host its `TimeoutExpired` fails the boot test

**Filed:** 2026-09-26 (lane F)
**Status:** open

## In short

A boot test of lane F's `158964f26` refused to build after 9985 s because
`scripts/test-canary-load.py` failed. The failing case was not one of the
live-load cases `0868abf03` taught to stand down on a busy host (lane B's
`b-a-test-canary-load-live-tests-fail-whenever-the-host-is-busy.md`). It was a
dry run: `run_wrapper` gives `canary-load-test.sh --dry-run` a fixed
`timeout=120`, the dry run took longer, and `subprocess.TimeoutExpired`
escaped uncaught, taking the whole suite -- and so the boot test -- down.

## The evidence

From `scripts/boot-test.sh`'s tooling-suite stage, on `os-lane-f-boot` at
`158964f26`, while this host was also running a lane-F pre-push and cargo
builds (and, likely, other lanes' boot tests):

```
File "...\scripts\test-canary-load.py", line 1424, in <module>
    rc, out = run_wrapper(["--at", "io_ring_nop", "--until", "crypto_poly1305_1KiB"])
File "...\scripts\test-canary-load.py", line 1252, in run_wrapper
    proc = subprocess.run(
...
subprocess.TimeoutExpired: Command '['C:\\Program Files\\Git\\bin\\bash.exe',
'scripts/canary-load-test.sh', '--at', 'io_ring_nop', '--until',
'crypto_poly1305_1KiB', '--dry-run']' timed out after 120 seconds
```

Every other check in the suite passed before it (the replay, window and
suite-marker groups), so the harness itself was behaving; only the wall-clock
bound failed.

## What would fix it (yours to choose)

* Catch `TimeoutExpired` in `run_wrapper` and report it the way the live
  cases now report host contention -- a skip naming the load, not a failure
  blaming the wrapper; or
* derive the bound from `host_headroom()` rather than a constant; or
* find what in a `--dry-run` takes long enough to hit 120 s under load (a
  dry run that only parses its arguments should not), and make it not.

The second and third keep the check meaningful; the first alone would at
least stop a slow host from voiding a three-hour boot test.

## What lane F did

Nothing to the file (`scripts/**` outside an additive change is not mine to
edit). Retried the boot test on the next head.

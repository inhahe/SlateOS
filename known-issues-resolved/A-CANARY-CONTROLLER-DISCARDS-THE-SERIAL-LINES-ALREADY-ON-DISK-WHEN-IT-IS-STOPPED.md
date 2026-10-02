### A-CANARY-CONTROLLER-DISCARDS-THE-SERIAL-LINES-ALREADY-ON-DISK-WHEN-IT-IS-STOPPED. A `--until` on the last benchmark of a suite voids the whole run as `until-never-matched` — 2026-09-02 — **Status: ✅ FIXED 2026-09-02** (lane A)

**In short:** the load controller tails the boot's serial log and turns the
load off when a named benchmark finishes. When it is told to stop, it stopped
*without reading the log one last time* — throwing away lines that were
already written to the file. If the benchmark that closes the window was in
that unread remainder, the controller reported that it had never seen it and
the run's data was discarded.

**Found the expensive way.** A `--bench` boot test was refused after **2517 s**
of pre-build gates, before QEMU ever started, with:

```
ERROR: refusing to build.  1 tooling test suite(s) failed:
    test-canary-load.py
```

and inside it:

```
spinner occupancy (live)
  FAIL a loaded run measures spinner occupancy: record problem='until-never-matched'
  FAIL a correctly-loaded run is not flagged as unapplied: until-never-matched
```

The suite then passed **twice in a row** on a quiet host. That pattern — fails
under three-lane load, passes when idle — reads as a flake, and the standing
advice in the entry above was to retry and move on. It was not a flake.

**Cause.** `canary-load.py`'s poll loop checked the stop-file *before* reading
the tail, and `break`ed on it:

```python
if args.stop_file and os.path.exists(args.stop_file):
    record["outcome"] = "stopped"
    break                      # <-- tail.lines() never called again
if tail.opened():
    seen = watcher.feed(tail.lines(), now)
```

So every line written since the previous poll was dropped unread. Not lines
that had yet to arrive — lines **already on disk**. The other replay tests all
hand the controller 0.3 s of quiet before stopping it, which is three poll
intervals, so they pass whether or not the final read happens; the live
occupancy test uses a 0.6 s window and two real CPU spinners, and under
concurrent builds the controller's own 0.1 s poll can slip past the gap.

**Why it is a production bug and not a test artefact.** `canary-load-test.sh`
writes the stop-file immediately after `wait "$BOOT_PID"` returns — QEMU has
exited and the serial log is complete and closed. So the *last* benchmark of a
suite is separated from the stop by microseconds, and `--load-until` naming it
loses every time the poll lands wrong. The whole boot's canary data is then
voided over a line sitting in the file. This is the failure mode
`canary-load.py`'s own module docstring says lost the second attempt.

**Fix.** The poll body is factored into `consume(now, final=False)`, and the
stop-file path calls it once more before breaking. The stop-file is what makes
that read correct rather than racy: it is written only after the producer has
exited, so the read cannot miss a line and cannot see a partial one. A
`timeout` exit deliberately does **not** drain — a timeout carries no promise
that the producer has finished, so stamping a release at the instant the
controller gave up would date the window's right edge up to `--timeout`
seconds late. A void run reported as void beats a void run reported with a
plausible-looking window.

**`release()` is allowed in the final drain; `fire()` is not,** and the
asymmetry is the point. Releasing is a true statement — the load was on
continuously from `on_at` until now, so every drained benchmark really did run
under it, and the only error is a right edge late by the length of the stall
(`occupancy_measured` divides by the span actually measured, so it stays
physically bounded regardless). Firing would be a false one: the producer has
already stopped, so a load applied at drain time covered nothing at all, and a
record claiming `fired` over a zero-length window is worse than the honest
`at-never-matched`.

**`record["drained_after_stop"]`** counts what the final read picked up, and is
initialised to 0 on every path so a reader can never mistake "this build
predates the drain" for "the drain found nothing". A large value means the
controller was starved near the end of the run, and those completions are
stamped at drain time rather than at their own — the record now says so instead
of hiding it.

**Measured, both controllers on the same scenario** (trigger through a normal
poll, then all 34 remaining lines in one burst, then stop with no grace):

| controller | `completions_seen` | `released` | `problem` | `drained_after_stop` |
|---|---|---|---|---|
| at `HEAD` (no final drain) | **6** of 40 | `False` | `until-never-matched` | — |
| with the drain | **40** of 40 | `True` | `None` | 34 |

Six. The other thirty-four were on disk, in a closed file, and were never read.
That is the whole defect, and it reproduces every time once the grace period is
removed — the "flake" was only ever the grace period sometimes being enough.

**Pinning test:** `test-canary-load.py`, "a line written just before the
stop-file is still read". It lets the trigger land through a normal poll, then
dumps the rest of the suite in one burst and stops with **no** grace period,
which is what a starved controller effectively sees. The assertion to believe
is `completions_seen == 40` — 40 only if nothing was discarded, and true under
every interleaving. `drained_after_stop` is deliberately *not* asserted
non-zero: a poll can legitimately land inside the burst, and pinning a number
that depends on that would make the test the very kind of race it closes.

**Amends the entry above.** "Not blocking — both retried green" was right about
the immediate consequence and wrong about the diagnosis: at least one of those
under-load tooling failures was a real defect that a retry hid. A suite that
fails only under load is not thereby a suite that is wrong.

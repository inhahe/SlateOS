### TD-TOOLS-THE-CONTAINMENT-FALLBACK-RAN-AFTER-THE-THING-IT-WAS-MEANT-TO-BACK-UP. `terminate_tree` killed the parent first, so `taskkill /T` had no tree left to walk — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `scripts/proctree.py` — `terminate_tree`, `TASKKILL_GRACE`; and
`scripts/osh-bash-diff.py` — `run_case`'s scratch directory.

**What:** a corpus sweep was found sitting at 0% CPU for **23 hours** while one
of its own cases spun at 100%. Two more escapees were still running from the
previous day, together having burnt ~140,000 CPU-seconds and pinned two cores
— which retroactively explains a whole morning of "flaky" corpus timeouts and
one investigation that concluded the *tests* were measuring the machine. They
were; the machine was being eaten by earlier runs of the same harness.

Two defects, both in the Windows `terminate_tree`:

* **Order.** `close_job` ran first and `taskkill /F /T` second. But `/T`
  enumerates *down from the named PID*, and that PID had just been killed, so
  the fallback covered nothing. Its only reason to exist is the window between
  `CreateProcess` and `AssignProcessToJobObject`, during which an MSYS shell
  that reaches a `&` in its first milliseconds forks a grandchild **outside**
  the job. Measured with a `while :; do :; done &` probe: the old order leaked
  that grandchild roughly **one run in three**, and the survivor spins forever.
* **No timeout.** `subprocess.run(["taskkill", ...])` was unbounded, and a
  caller blocked inside it *still holds the job handle open* — so the hang and
  the survival are the same event. `taskkill` blocks when a target sits in an
  uninterruptible wait.

**Fixed** by running `taskkill /F /T` **before** `close_job`, bounding it with
`TASKKILL_GRACE = 10.0`, running it as a `Tree` itself so its own descendants
die with it, and adding a direct `proc.kill()` in between for the case where
`taskkill` is refused outright. Verified 10/10 probe runs with zero survivors
against the ~1-in-3 baseline.

`osh-bash-diff.py` additionally passes `ignore_cleanup_errors=True` to its
scratch `TemporaryDirectory`: on Windows a directory cannot be removed while a
process has it as its cwd, so a single leaked process used to raise
`PermissionError` **out of** `run_case` and end the sweep at whatever case
happened to leak.

**Standing lesson:** a fallback that runs *after* the primary mechanism is not
a fallback — it is dead code that looks like insurance. Order the belt and the
braces so the second one is still reachable when the first fails, and give
every external kill a deadline: the process holding the containment handle must
never be the one waiting on an unbounded call. And when a test suite starts
"measuring the machine", enumerate what is *on* the machine before blaming the
tests.

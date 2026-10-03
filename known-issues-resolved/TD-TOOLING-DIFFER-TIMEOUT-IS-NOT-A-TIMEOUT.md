### TD-TOOLING-DIFFER-TIMEOUT-IS-NOT-A-TIMEOUT. `subprocess.run(timeout=…)` on Windows can wait forever, and wedged a corpus sweep — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `scripts/osh-bash-diff.py`'s `run_case`, which ran each shell under
`subprocess.run(…, timeout=case.timeout)` (default 20 s).

**Symptom.** A sweep stopped dead. The runner sat at **0 % CPU with no child
processes**, seven minutes past a 20-second budget, having apparently stopped
mid-run. No case was named as slow, nothing timed out, nothing failed — the
sweep simply never advanced, and the only way out was to kill it.

**Root cause is in CPython, not in the differ.** `subprocess.run`'s Windows
timeout path is:

```python
except TimeoutExpired as exc:
    process.kill()                                   # the direct child only
    if _mswindows:
        exc.stdout, exc.stderr = process.communicate()   # note: no timeout
```

The capture threads are blocked in `read()` on the pipes, and a pipe does not
report EOF while *any* process still holds its write end. A grandchild that
inherited those handles — a `&` job, an interposed `#!` interpreter, an external
the case spawned — keeps them open after the shell is killed, so the second
`communicate()`, which takes no timeout at all, never returns. **The timeout
silently becomes infinite.** The 0 %-CPU-no-children picture is exactly that:
the shell is dead, the grandchild has been reaped or detached, and the drain is
still waiting on a handle nobody will close.

Measured, on a shell that leaves a sleeping grandchild behind and a 3-second
budget: the tree-killing version returns in **3.3 s** with the partial output;
`subprocess.run` was **still blocked at 20 s** when the probe gave up.

**Why it mattered here.** The corpus is the only thing that validates a parity
change, so a differ that can hang past its own deadline does not just cost time
— it removes the ability to check work at all, and it does so *silently*, in a
way that reads as "slow sweep" rather than "broken tool".

**Fixed** by extracting the process-tree machinery `run-timeout.py` already had
— a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` on Windows, a process
group plus `SIGKILL` on POSIX — into `scripts/proctree.py`, and giving it two
entry points over that one mechanism: `Tree` (the streaming case, which
`run-timeout.py` now imports instead of duplicating ~95 lines of `ctypes`) and
`run_captured` (the batch case, which the differ now calls). `run_captured`
kills the **whole tree first** and only then drains, so every write end is
closed before the drain begins and the drain terminates. The grace it passes to
that second `communicate()` is a guard against the impossible, not a budget.

**The general rule this leaves:** in this repo, `subprocess.run(timeout=…)` is
not a timeout. Any command that could spawn a grandchild — which is every shell
invocation — goes through `proctree.run_captured` or `run-timeout.py`.

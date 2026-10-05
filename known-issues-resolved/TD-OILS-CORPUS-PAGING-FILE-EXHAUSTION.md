### TD-OILS-CORPUS-PAGING-FILE-EXHAUSTION. Three background-job corpus cases failed a full run with "The paging file is too small" — ✅ RESOLVED 2026-08-01 (harness, not a shell bug)

**Where:** the differential harness `scripts/osh-bash-diff.py`, not oils itself.

**Was:** a full `python scripts/osh-bash-diff.py` run on a loaded Windows host
reported `background-capture-sink`, `background-output` and `jobs-listing` as
failures. The osh stderr for the first was:

```
line 67: cat: The paging file is too small for this operation to complete. (os error 1455)
```

Every one of the three passed on its own (`-k background` → 5/0/0, `-k
jobs-listing` → 1/0/0) with the same binary, immediately after.

**What it was.** Windows error 1455 is `ERROR_COMMITMENT_LIMIT`: the system
could not reserve backing store for a new process. A host limit, not a
divergence — and the three cases that hit it are exactly the ones that keep
several `sleep`/`cat` children alive at once, so they are the first to be
refused a process when commit is tight. bash's own children were started at a
different moment in the same case and survived, which is why the two sides
differed rather than both failing.

**Fixed by** treating a failure to *start* a child as a failed measurement
rather than an answer, alongside the timeout that `measure` already retried:
`starved()` looks for `os error 1455` / "The paging file is too small" /
POSIX `EAGAIN` in either stream, and a starved run is measured once more in a
fresh temporary cwd. A second starvation is still reported, but the report says
which kind of failure it was so it is not chased as a shell divergence.

**Note for the future.** The retry costs nothing on a healthy run, but it does
mean a *genuine* oils bug whose symptom is one of those three strings would be
measured twice before being reported. None of them is a message oils produces
itself — they all come from the OS through a spawn failure — so this is safe,
but a new spawn-related diagnostic should not be worded to collide with them.

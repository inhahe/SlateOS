### [E] A process tree cannot contain everything started outside its Job Object -- 2026-10-21

**Status:** OPEN -- the limits that remain after `proctree` began adopting
escapes (lane-e, 26c579474 and its review follow-up). None of them has a fix
in sight; this entry exists so they can be found.

**In short:** `run-timeout.py`, and everything else built on
`proctree.Tree`, kills a whole process tree on a timeout, on Ctrl-C, and when
the runner itself dies, by keeping the tree in a Windows Job Object. Windows
starts some programs outside their caller's job: a Store-packaged app reached
through an App Execution Alias (the `python.exe` and `python3.exe` in
`%LOCALAPPDATA%\Microsoft\WindowsApps`) is the one seen here. A tree now
finds such processes and adopts each into a job of its own, so they die with
it too. A few cases still get past that, listed below. Each needs an unusual
coincidence, and run-timeout's `taskkill /T` still reaches most of them on a
timeout.

**What is not contained:**

| Case | What happens |
|---|---|
| An escape before the sweep that adopts it (up to `ESCAPE_POLL`, 0.5 s) | If the runner dies in that window, the escape keeps running. A timeout or Ctrl-C is still covered: the closing sweep and `taskkill /T` reach it. |
| An escape whose parent left the tree before the job's completion port said the parent had joined | Its parentage cannot be proved, so it is left alone, since adopting a stranger would kill it. `taskkill /T` reaches it only while the chain of parents is still alive. |
| A process Windows refuses a job (none seen yet) | It is named through `warn`, held by handle and killed with the tree, but it outlives the runner's own death. |
| A process that cannot be opened (another user's, or elevated) | It is named through `warn` and left alone. |
| Anything across the WSL boundary | It runs in a separate Linux VM. Bound it with `timeout(1)` inside the `wsl` call: see `run-timeout.py` and `TD-B-DIFF-HARNESSES-HAVE-NO-PER-CASE-BOUND-AND-ORPHAN-ACROSS-WSL`. |
| A clock set back between a parent's start and its child's | The child looks older than its parent and is left alone. The proof compares wall-clock creation times. |

**Where:** `scripts/proctree.py` (`_Escapes`, and its module docstring for
the mechanism and the measurements), `scripts/run-timeout.py`.

**Fix:** none planned. Closing the first two gaps would need to hear of a
process the moment it is created, which only a kernel-mode or ETW
process-start trace gives, and both need administrator rights. Revisit this
if run-timeout ever runs elevated.

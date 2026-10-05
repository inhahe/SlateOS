### TD-OILS-KILL-PGRP. osh has no notion of a process group, so `kill 0` and a negative pid do not reach one — OPEN 2026-07-27

**Where:** `userspace/oils/src/interp.rs` — `Shell::kill_one`.

**What.** `kill 0` signals every process in the shell's own process group in
bash; `kill -TERM -123` signals process group 123. osh tracks individual child
processes only, so both are treated as ordinary pids and report `No such
process`.

**Proper fix.** Needs process groups first — which SlateOS's process model does
not yet have, and which Windows (where osh is developed) expresses differently
(job objects). Once `setpgid`-equivalent grouping exists, `kill_one` should route
a zero or negative pid to the group rather than to a process.

**Impact.** Narrow: scripts that self-signal a whole pipeline. Untestable in the
corpus anyway, since `kill 0` would kill the harness.

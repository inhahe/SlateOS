## B-PS-C-COLUMN-TRUNCATES-CPU-TIME-TO-SECONDS (lane B, 2026-09-26) — FIXED 2026-09-26

**In short:** `ps -f`'s `C` column (percent of CPU a process has used over its
life) comes out lower than procps' for any process whose CPU time is not a
whole number of seconds: a process that has used 0.33 s of CPU in its first
second shows `0` here and `33` in procps. Found by `ps-diff.sh`'s `ps -ef`
case on a heavily loaded host, where GNU's `ps` took over a second to list
itself; on an idle host both sides are under a second and both print 0, which
is why the case usually passes.

**Where:** `userspace/coreutils/src/bin/ps.rs`, `cpu_percent`: it divides the
CPU ticks by `TICKS_PER_SEC` *before* multiplying by 100, so the fraction of
a second is lost. procps (`output.c`, `pr_c`) multiplies first -- `total_time
* 100 / Hertz`, then divides by the elapsed seconds -- and caps the result at
99, which ours does not.

**The proper fix:** port `pr_c` from the procps-ng WSL ships (4.0.4) exactly,
including how it measures elapsed time (whole seconds or ticks -- 4.x's
`TIME_ELAPSED` needs reading, not guessing) and the 99 cap, and give
`ps-diff.sh` a case whose subject is old enough and busy enough to have a
non-zero `C` on an idle host, so the column is tested on purpose rather than
only when the host is slow.

**Severity: low.** One column of one format, off by at most the fraction of a
second of CPU time the process has used, divided by its age.

**How it was closed (2026-09-26).** `cpu_percent` is procps-ng 4.0.4's
`pr_c` over libproc2's `TIME_ELAPSED`, read from the source rather than
inferred: `/proc/uptime` read once per listing into `boot_tics` (truncated),
the age `(boot_tics - start_time) / Hz` as a double and back to whole jiffies,
`ticks * 100 / jiffies`, the low 32 bits, capped at 99. Unit tests pin the
arithmetic; `ps-diff.sh` 60/0. What is still missing is a harness case whose
subject is busy and old enough to have a non-zero `C` on an idle host.

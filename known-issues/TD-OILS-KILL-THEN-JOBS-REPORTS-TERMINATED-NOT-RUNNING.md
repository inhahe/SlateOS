### TD-OILS-KILL-THEN-JOBS-REPORTS-TERMINATED-NOT-RUNNING. `kill %n; jobs` on the same line shows the kill under osh and not yet under bash — OPEN — 2026-08-03

**Where:** `userspace/oils/src/interp.rs` — `builtin_kill` records
`128 + signum` and clears the handle as part of the kill.

**Reproduce:**

```sh
sleep 30 >/dev/null 2>&1 &
kill %1
jobs
```

bash prints `[1]+  Running` (it has not yet taken delivery of the `SIGCHLD`, and
`jobs` reports what the job table says); the next `jobs` prints `Terminated`.
osh prints `Terminated` at once.

**Why.** bash's kill and its *notice* of the death are two separate events, and
a listing on the same line falls between them. osh's `kill` terminates the child
synchronously and writes the status down itself, so there is no gap to fall
into. Pre-existing and unrelated to the forked-child work — confirmed by probing
it on its own.

**Proper fix.** Defer the status write to the next reap (`poll_jobs`), so a
listing on the same line still sees `Running`. That means `kill` recording only
an *expected* signal on the row and letting the reap turn it into a status —
a change to how a signalled job's status is settled, worth doing with the
`kill`-by-pid work above rather than alone.

**Impact.** Cosmetic and one line wide: any script that kills and lists in the
same breath. Every subsequent listing agrees.

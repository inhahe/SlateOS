## TD-B-FLOCK-WAIT-POLLS (lane B, 2026-09-26) — **open**, the fix is lane D's

**In short:** `flock -w SECONDS` (wait for a lock, but not forever) cannot
wait the way util-linux's does on SlateOS, so ours waits a slightly different
way everywhere. Upstream calls the blocking `flock()` and has a timer
interrupt it with a signal when the time is up. SlateOS's C library
implements a blocking `flock()` as a loop that retries until the lock is free
(`posix/src/file.rs`, `do_flock`: `SYS_SLEEP` then `continue` on `EAGAIN`),
and nothing in that loop returns `EINTR` when a signal handler has run -- so
the timer would fire, the handler would set its flag, and `flock` would go on
waiting forever.

**What was done instead** (`userspace/flock/src/main.rs`, the lock loop):
with `-w`, `flock` tries `LOCK_NB` until the deadline, sleeping 1 ms,
doubling to at most 25 ms, between tries. A caller sees the same outcomes --
`scripts/flock-diff.sh` checks them against util-linux -- and only notices a
release up to 25 ms late. Without `-w`, the blocking call is upstream's.

**The proper fix, lane D:** the libc's blocking `flock()` should return
`EINTR` when a caught signal is delivered while it waits, as Linux's does
(the kernel's `SYS_SLEEP` would have to report the interruption). Then `-w`
can be upstream's timer again (design-decisions §1035), and every other
program that relies on a signal interrupting a blocking call benefits. Worth a request once lane D's
signal delivery is known to reach that loop; until then the polling is
correct, only less exact.

**Progress, 2026-10-01 -- the kernel half exists.** Lane A added
`SYS_FS_FLOCK_HANDLE` (1094, commit `b108865ea`, on main with lane A's next
publish): BSD `flock(2)` on a handle that waits *in the kernel* when
`LOCK_NB` is absent, ends the wait with `EINTR` when a caught signal arrives,
and restarts it under `SA_RESTART`, as Linux does. The Linux-ABI `flock(2)`
waits for real too (it had answered `EWOULDBLOCK` even without `LOCK_NB`).
Lane A has told lane D, whose `do_flock` can move onto 1094 and drop the nap
loop. **Lane B's step, once that libc change is on main:** put `-w` back to
util-linux's `setup_timer` + blocking `flock()` + `EINTR` check, delete the
`LOCK_NB` polling, and re-run `scripts/flock-diff.sh` -- including a case that
holds the lock past the deadline, which is the one the timer exists for.

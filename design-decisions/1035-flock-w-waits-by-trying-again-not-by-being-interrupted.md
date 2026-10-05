## 1035. `flock -w` waits by trying again, not by being interrupted

**Date:** 2026-09-26
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `flock -w 5 FILE CMD` means "wait up to five seconds for the
lock". util-linux implements the limit by sleeping in the lock call and
setting an alarm that interrupts the sleep. On SlateOS the lock call cannot be
interrupted -- the C library implements the wait as a loop that retries until
the lock is free -- so the alarm would go off and `flock` would keep waiting
forever. Our `flock` instead asks for the lock without waiting, sleeps a
moment, and asks again until the time is up. The answers a script sees are
the same; a lock that comes free is noticed up to 25 ms later.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **Try `LOCK_NB` until the deadline, 1 ms doubling to 25 ms between tries (chosen)** | `-w` works on SlateOS today; everywhere, a released lock is taken up to 25 ms late | one mechanism on every platform; no signal handler, timer or async-signal-safety to get right | not upstream's mechanism; a busy file sees one `flock` call per interval from each waiter |
| Upstream's timer and signal | on SlateOS `flock -w` never times out | upstream's code, and no latency | needs the libc's blocking `flock()` to return `EINTR` (lane D) -- until then it hangs, which is worse than the old program |
| Upstream's where it works, polling on SlateOS | the same results by two mechanisms | exact on Linux | two code paths, one of them untested by the harness, which runs on Linux |

Without `-w`, `flock` blocks in `flock()` exactly as upstream does: nothing
there needs interrupting. `-w 0` is `-n`, and a negative or unrepresentable
time is refused with upstream's timer error, so the difference is confined to
how the wait is spent. `scripts/flock-diff.sh` checks the outcomes against
util-linux with a lock held by a third party.

**Where:** `userspace/flock/src/main.rs` (the lock loop, `POLL_MAX`).

**Revisit** when lane D's `flock()` returns `EINTR` on a delivered signal
(known-issues TD-B-FLOCK-WAIT-POLLS): the timer is then upstream's and exact,
and this entry can close.

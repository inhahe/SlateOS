### [A] `A-A-FUTEX-WAIT-REQUEUE-PI-IS-DEAF-TO-SIGNALS` — a thread in `FUTEX_WAIT_REQUEUE_PI` is not woken by a signal -- 2026-10-15
**Status:** OPEN (lane A).

**In short:** a thread waiting on a condition variable whose mutex uses
priority inheritance (glibc's `pthread_cond_wait` with a `PTHREAD_PRIO_INHERIT`
mutex parks in `FUTEX_WAIT_REQUEUE_PI`) cannot be interrupted by a signal: its
handler waits until the condition is signalled or the wait times out, however
long that is. Killing it still works (`kill_task` does not wait for it).

**Where.** `kernel/src/ipc/futex.rs`, `futex_wait_requeue_pi`: the park loop
leaves only when the thread is made the PI mutex's owner or its timeout
passes. Unlike `futex_wait_bitset`'s user path, it never registers as a
signal waiter (`proc::signal::register_signalfd_waiter`) and never checks
`proc::signal::wait_ends`, so a posted signal neither wakes it nor ends the
wait.

**Found by** the 2026-10-15 audit of every park's re-check
(`A-A-PREEMPTION-BEFORE-A-PARK-ERASED-THE-WAKE-OWED-TO-IT`, review of
4b6f6bf85..30f6af1bc).

**The fix.** As Linux does: register as a signal waiter before the first
park, and when a deliverable signal is pending at a wake, take the waiter off
whichever queue holds it (the condvar's or, after a requeue, the PI mutex's,
as the timeout path does), restore the deboost, and answer
`ERESTARTNOINTR`-style -- Linux restarts `FUTEX_WAIT_REQUEUE_PI` after the
handler rather than returning `EINTR` to `pthread_cond_wait`. A test: a ring-3
fixture with a PI condvar wait and a `SIGALRM` handler that must run while the
condition is never signalled.

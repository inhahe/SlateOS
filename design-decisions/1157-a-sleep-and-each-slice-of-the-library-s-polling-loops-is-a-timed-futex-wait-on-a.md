## 1157. A sleep, and each slice of the library's polling loops, is a timed futex wait on a word of its own, which the kernel ends for a signal -- not `SYS_SLEEP`, which sleeps its full time

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `sleep`, `nanosleep`, `usleep` and `clock_nanosleep` slept in
the kernel's `SYS_SLEEP`, which no signal ends: a program that caught
`SIGINT` during `sleep(10)` ran its handler ten seconds later, and
`nanosleep` never answered `EINTR`. The loops the library builds from the
kernel's non-blocking calls -- `poll`, `select`, `epoll_wait`, the socket
waits, `flock`, the timerfd and inotify reads -- slept their 10 ms slices the
same way. All of them now sleep in a timed futex wait on a word nothing ever
wakes: the kernel ends that wait for a signal, and the counts
`posix/src/interrupt.rs` keeps say whether a handler ran here.

### The alternative: an interruptible `SYS_SLEEP`

Ask lane A for a `SYS_SLEEP` that the kernel ends for a signal and that
answers the time left, as Linux's `hrtimer_nanosleep` does.

- **For:** the kernel's own sleep, which picks a tick-based path for sleeps
  over 100 ms to spare its high-resolution timers; and the remaining time
  from the kernel rather than from a second clock read.
- **Against:** a kernel change for a fault that is entirely this library's
  to fix, with the lane waiting on it; and a second mechanism beside the futex
  wait every other interruptible wait here already uses, with its own
  restart rules to keep in step. The timer cost is the one every timed wait
  already pays -- the hrtimer queue is sized for one timer per task in a
  timed wait (`kernel/src/hrtimer.rs`, 256 a CPU before it even warns, 4096
  before it refuses) -- and a sleep is one such task. The remaining time is
  the deadline less the clock, read once as the sleep ends.

### Consequences

- A signal handler ends a sleep at once, `SA_RESTART` or not (Linux never
  restarts one), with the time left in `nanosleep`'s `rem` and
  `clock_nanosleep`'s `remain`, and `sleep` answering the whole seconds
  left, truncated, as glibc's does.
- The polling loops stay polling loops -- 10 ms slices, and 2 ms for
  `flock`, which had spun on `yield` -- but a signal ends a slice at once, and
  a handler ends the call by its rule: `Never` for `poll`, `select` and
  `epoll_wait` (signal(7)), `IfAsked` for the reads, `accept` and `flock`,
  `Never` for a socket with a timeout.
- `pause` and `sigsuspend` wait the same way rather than polling a count
  every 2 ms, which also closed a lost wake-up in `sigsuspend`.

**Where:** `posix/src/lowlevellock.rs` (`sleep_until`, `nap`),
`posix/src/time.rs`, `posix/src/poll.rs`, `posix/src/epoll.rs`,
`posix/src/socket.rs`, `posix/src/file.rs`, `posix/src/signal.rs`.

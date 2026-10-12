### A-FREEZE-ENDS-SOME-CALLS-WITH-EINTR -- 2026-10-09 -- OPEN (lane A)

**Status:** OPEN (lane A). A limit of the freezer's first version
(design-decisions §1562).

**In short:** freezing a program -- a container's pause, and later a kernel
replacement or hibernation -- pulls each of its threads out of the call it is
waiting in, and the call is meant to start again unnoticed when the program
is thawed. For most calls it does: reads, writes, `wait4`, `nanosleep`,
`pause`, `sigsuspend`, `FUTEX_WAIT` with no timeout, the native `SYS_SLEEP`.
A few return `EINTR` instead, as they do when a signal handler runs.
A program that handles `EINTR` (as it must for those calls) retries; one that
does not reports an error it would never have seen without the freeze.

| Call | Under a freeze now | Linux under its freezer | Fix |
|---|---|---|---|
| `poll`, `ppoll` | `EINTR` | restarts with the time left (`restart_block`, `do_restart_poll`) | a restart block holding the absolute end time |
| `select`, `pselect6` | `EINTR` | restarts, the time left written back to the caller's `timeval` | `ERESTARTNOHAND` and the write-back |
| `epoll_wait`, `epoll_pwait` | `EINTR` | `EINTR` too | none needed |
| `rt_sigtimedwait` | `EINTR` | `EINTR` (before `TASK_FREEZABLE`) | a restart with the deadline, to be invisible |
| `FUTEX_WAIT` with a timeout | `EINTR` | restarts to its absolute time (`futex_wait_restart`) | a restart block, as `nanosleep` has |

**Where:** `kernel/src/syscall/linux.rs` (`caller_has_deliverable_signal`,
`interruptible_wait_slice`, the `rt_sigtimedwait` loop) and
`kernel/src/ipc/futex.rs` (the timed waits). The restart machinery they would
use is `syscall::linux::restart` and `restart_block`.

**Why it matters for §1126:** a kernel replacement should be invisible to
every program, and these are the calls through which it would not be. A
container pause has the same exposure on Linux for `epoll_wait`, so that one
is not a bug.

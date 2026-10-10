## 1562. The freezer parks a thread at the edge of user mode, and backs it out of any call it waits in

**Date:** 2026-10-09 · **Decided by:** Claude (operator-approved scope: §1126,
the operator's decision to replace the kernel whole and to hibernate on the
same freeze; this is its first step) · **Lane:** A

**In short:** to swap in a new kernel without a restart, or to hibernate,
every program first has to be stopped at a point where nothing of the
kernel's is going on for it -- so that what it was doing can be written down
as its registers alone. The *freezer* does that. A thread stops at the moment
it is about to go back to its program: after a system call, an interrupt or
an exception, or before its first instruction. A thread waiting inside a call
(reading a pipe, sleeping, waiting on a lock) is pulled out of the call
first, which is arranged so that it issues the same call again when it is
thawed -- with a sleep keeping its deadline -- and the program never notices.
Container pauses (`docker pause`) use the same mechanism. They used to stop
each thread wherever it happened to be, sometimes in the middle of kernel
work while holding a lock that other programs then waited on.

**The mechanism** (`kernel/src/proc/freezer.rs`):

- **Where a thread parks.** At the two signal-delivery checkpoints -- a system
  call's exit (`handlers::deliver_pending_signal`) and an interrupt's or
  exception's return to ring 3 (`deliver_pending_signal_on_interrupt_exit`) --
  *before* any signal is looked for, as Linux's `get_signal` tries to freeze
  first; and before a thread's first instruction (the spawn, fork and clone
  trampolines), which passes no checkpoint. It blocks on a `frozen` wait
  (`/proc/<pid>/wchan` says `frozen`) until thawed, then the checkpoint goes on.
- **How a waiting thread gets out.** Every interruptible wait already asks
  "is a deliverable signal pending?" and registers in the signal-waiter list
  while it sleeps. It now asks `signal::wait_ends`, which is also true when
  the thread is to freeze. The freezer wakes exactly the registered waiters.
  So a frozen wait ends as a wait interrupted by a signal ends: with the
  restart sentinel (`ERESTARTSYS`, `ERESTARTNOHAND`, `ERESTART_RESTARTBLOCK`).
  With no signal to deliver, the checkpoint rewinds the call, so after the
  thaw the program re-issues it. `nanosleep` keeps its absolute deadline in
  its restart block. The native `SYS_SLEEP` now does too: it is resumed
  through a native `SYS_RESTART_SYSCALL` (1171), the counterpart of Linux's
  `restart_syscall`, which the Linux number cannot be since 219 means
  something else in the native table.
- **A thread in ring 3.** Interrupted by the reschedule IPI (another CPU) or
  its next tick, it parks on the interrupt's way back to ring 3.
- **When it is done.** `freeze_system` returns once every user thread but
  the caller's is parked, stopped (job control or a debugger), not yet
  started, or gone. A thread that does not get there in time -- one in an
  uninterruptible wait that does not end -- fails the freeze: the system is
  thawed and the threads are named, as Linux reports "tasks refusing to
  freeze".
- **Two reasons.** The whole system (`freeze_system`/`thaw_system`) or a set
  of processes (`freeze_processes`/`thaw_processes`: a container's pause; a
  child of a frozen process is frozen with it). A process frozen for both stays
  frozen until both have thawed it.
- **Kill and stop.** A fatal signal still ends a frozen process, as cgroup
  v2 allows. Job control is separate: a stopped thread counts as at rest, and
  a thaw does not continue it, where a container's unpause used to resume
  every suspended thread, `SIGSTOP`ped ones included.

| | For | Against |
|---|---|---|
| **Park at the user-mode edge; back waits out with the signal machinery (chosen; Linux's original "fake signal" freezer)** | a frozen thread is described entirely by its user registers, which is what §1126's records need; holds no lock; one place to park, one predicate for every wait; reuses the restart rules every call already has for signals | a call that a signal ends with `EINTR` rather than a restart -- `epoll_wait`, `poll`, `select`, `sigtimedwait` and a timed `FUTEX_WAIT` here -- ends with `EINTR` under a freeze too (Linux's `epoll_wait` does, under its freezer) until each has a restart block |
| Freeze waiting threads in place (Linux 6.1+'s `TASK_FREEZABLE`) | nothing is woken; no call ends | the thread's state is then a kernel stack in the middle of a wait, which no hand-over record can describe; every wait would need its own record kind |
| Suspend each thread wherever it is (what container pause did) | simple | a thread can stop holding a kernel lock, blocking other programs until the thaw; useless for §1126, whose records cannot describe a kernel stack |

**What it does not do yet, recorded in `known-issues`:**
- `A-JOB-CONTROL-STOP-SUSPENDS-A-THREAD-WHEREVER-IT-IS`: `SIGSTOP` from another
  process suspends threads mid-kernel, as the old container pause did. The
  freezer counts a stopped thread as at rest. That is right for its pause, but
  wrong for §1126's records, so a group stop should move onto the freezer's
  park point before the records are built.
- `A-FREEZE-ENDS-SOME-CALLS-WITH-EINTR`: `poll`, `select`, `epoll_wait`,
  `sigtimedwait` and the timed futex waits return `EINTR` where Linux
  restarts `poll`, `select` and the timed futex waits with the time left.
  Each needs a restart block.

**Tested** by `freezer::self_test` (the bookkeeping) and
`spawn::self_test_freezer` (ring 3, both ABIs): a parent in `wait4`, children
in `nanosleep`, a pipe read, `FUTEX_WAIT` and ring 3, and a native program in
`SYS_SLEEP`. All are frozen and held for a second: every thread is parked and
the computing child makes no progress. Thawed, every call goes on with no
`EINTR`, and the sleeps end at their own deadlines. Then one process is frozen
alone, as a container's pause freezes it.

**How to reverse:** container pause can go back to `sched::suspend`
(`container::pause`/`unpause`); the checkpoints' `park_if_frozen` calls and the
`wait_ends` substitutions are inert while nothing is frozen.

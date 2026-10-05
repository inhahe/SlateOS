## 1156. A call that waits in the C library ends for a signal as glibc's does on Linux: only for a handler that ran on its own thread, and despite `SA_RESTART` wherever Linux's kernel would not restart it

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** some calls wait inside this library rather than in the
kernel -- `sem_wait`, the message queues, System V's `msgrcv` and `semop`,
`io_getevents`, `aio_suspend`, `gai_suspend`, Linux's `futex()`. When a
signal comes, each must decide whether to fail with "interrupted" (`EINTR`)
or keep waiting. They now decide as glibc's do on Linux, which is not what
POSIX's text alone gives: POSIX has a handler installed with `SA_RESTART`
restart every such call, and Linux ends some of them anyway -- the timed
waits, System V's calls, `io_getevents`. This follows Linux, because the
programs written for it count on those interruptions.

### The decision

Three rules, from glibc 2.39's answers on Linux -- 115 cases in
`posix/src/interrupt_oracle.txt` (`posix/tools/oracle/interrupt_harness.py`),
replayed by `interrupt::tests::every_interruption_is_glibcs`:

1. **A signal that runs no handler on the waiting thread never ends the
   call**: one set to `SIG_IGN`, one ignored by default (`SIGCHLD`,
   `SIGURG`), a stop and continue, one another thread handles.  POSIX says
   so too (XSH 2.4.4: "Signals that are ignored shall not affect the
   behavior of any function").
2. **A handler installed without `SA_RESTART` ends every one of them**, with
   `EINTR` (`gai_suspend`: `EAI_INTR`).
3. **A handler installed with `SA_RESTART` ends only the calls Linux's kernel
   never restarts**: the timed semaphore waits, `msgsnd`, `msgrcv`, `semop`,
   `semtimedop`, `io_getevents`, and `aio_suspend`, `gai_suspend` and
   `futex(FUTEX_WAIT)` when given a timeout.  `sem_wait`, the message-queue
   calls (timed or not), and the untimed `aio_suspend`, `gai_suspend` and
   `futex` wait on.

The thread functions POSIX forbids to answer `EINTR` (`pthread_mutex_lock`,
`pthread_cond_wait` and the rest) and `getaddrinfo_a(GAI_WAIT)` end for no
signal, here as in glibc.

### The alternative: POSIX's `SA_RESTART` alone

XSH `sigaction`: "If set, and a function specified as interruptible is
interrupted by this signal, the function shall restart and shall not fail
with [EINTR] unless otherwise specified."  Read strictly, that restarts
`semop`, `msgrcv` and `sem_timedwait` too; glibc's manual says as much in
general ("return from that handler will resume a primitive"), and
signal(7) as Ubuntu 24.04 ships it lists `sem_timedwait`, with `sem_wait`,
among the calls `SA_RESTART` restarts -- which the oracle shows Linux does
not do for `sem_timedwait`.

- **For:** the letter of both texts, and one rule where this has two.
- **Against:** a program written for Linux that installs its shutdown
  handler with `signal()` -- which sets `SA_RESTART` -- and waits in
  `msgrcv` or `semop` for work gets `EINTR` there on Linux, checks its flag
  and exits.  Restarting would leave it waiting for work that never comes.
  Linux's behaviour is not an accident to be corrected: signal(7) lists the
  System V calls and `io_getevents` as "never restarted", and the kernel's
  restart machinery is built to end timed waits (`ERESTART_RESTARTBLOCK`,
  which any handler turns into `EINTR`).  And no ported program can depend
  on `SA_RESTART` keeping one of these calls going, since on Linux it does
  not.

This is D-Q6's rule applied with its purpose rather than its letter: it
departs from glibc where glibc contradicts its standards; here glibc
contradicts POSIX's and its own manual's general statement, and is followed,
because what it does is the kernel's documented, deliberate behaviour and
the one the programs are written against.

### How a wait tells, which is not a choice but is not obvious

The native kernel cannot see a disposition or `SA_RESTART` -- they are this
library's table -- so it ends a futex wait for every signal it hands the
trampoline.  The trampoline's dispatch, which runs on the thread whose wait
was ended, counts in that thread's block each handler it runs and those
installed without `SA_RESTART`; a wait compares the counts with a mark taken
before it slept (`posix/src/interrupt.rs`).  The calls that are system calls
on Linux (the message queues, System V's, `io_getevents`, `futex`) take the
mark as the call begins, since a signal arriving anywhere in a Linux system
call is still pending when it next sleeps; the calls glibc builds from futex
waits in user space (`sem_wait`, `aio_suspend`, `gai_suspend`) take one per
sleep, since a handler that runs between two of glibc's sleeps is over by
the second.  `io_pgetevents` takes its mark before it sets its signal mask,
so that a signal the mask lets through as it is set ends the call, as the
atomic mask-and-sleep of Linux's makes it.

**Where:** `posix/src/interrupt.rs`, `posix/src/lowlevellock.rs`
(`futex_wait_interruptible`), and the calls: `semaphore.rs`, `mqueue.rs`,
`sysv_msg.rs`, `sysv_sem.rs`, `linux_aio_abi.rs`, `aio.rs`, `gai_a.rs`,
`linux_futex.rs`.

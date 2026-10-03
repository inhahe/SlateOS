## D-POSIX-FUTEX-WAITS-DISCARD-EINTR — libc's futex waits throw the kernel's answer away, so no wait of this library's ends early for a signal handler (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/interrupt.rs`)**

**In short:** when a signal handler runs while a thread is blocked in
`sem_wait`, POSIX says the call returns -1 with `errno` `EINTR` -- and
`gai_suspend` returns `EAI_INTR`. Here the thread goes back to waiting once
the handler returns, because this library's futex wait
(`posix/src/lowlevellock.rs`, `futex_wait` and `futex_wait_timeout`) ignores
what the `SYS_FUTEX_WAIT` system call answered. The kernel does report the
interruption: `sys_futex_wait` (`kernel/src/syscall/handlers.rs`) hands the
signal-delivery checkpoint a restart sentinel (`ERESTARTSYS`) that becomes a
restart or a user-visible `EINTR`.

**Who sees it:** a program that breaks a thread out of `sem_wait`,
`sem_timedwait` or `sem_clockwait` with a signal -- a common way to stop a
worker -- waits on instead; and `gai_suspend` never answers `EAI_INTR`
(`posix/src/gai_a.rs` says so in its module comment).

**The proper fix:** a futex wait in `lowlevellock` that returns the
kernel's `EINTR`, used exactly where POSIX requires the interruption to show
-- `sem_wait`, `sem_timedwait`, `sem_clockwait`, `gai_suspend` -- and not
where it forbids it (`pthread_cond_wait` and `pthread_mutex_lock` never
return `EINTR`). Three things to settle first. Which interruption each
function reports: Linux's signal(7) has `sem_wait` restarted when the
handler was installed with `SA_RESTART` but the System V semaphore and
message calls never restarted -- to be confirmed against glibc, not
copied from the manual. How to honour `SA_RESTART` at all, since the native
kernel cannot see it: libc's own dispositions are the only record of it. And
an interruption by a signal that ran no handler -- one this library
ignores, or one another thread took -- must not end the wait, which the
kernel's answer alone cannot tell apart. Plus a ring-3 test that signals a
thread blocked in `sem_wait`, since the host tests have no signals to send.

**Where:** `posix/src/lowlevellock.rs` (`futex_wait`, `futex_wait_timeout`),
`posix/src/semaphore.rs`, `posix/src/gai_a.rs` (`gai_suspend`).

**Fixed 2026-09-30.** Every call that waits in this library now asks what
the signal did (`posix/src/interrupt.rs`). The trampoline's dispatch counts,
in the thread's `PerThread` block, each handler it runs and those installed
without `SA_RESTART`; a wait the kernel ends for a signal compares the counts
with a mark taken before it slept, so a signal that ran no handler on the
thread -- ignored, ignored by default, or handled on another thread -- ends
nothing, which the kernel's answer alone could not tell. Which calls end for
which handlers is glibc's on Linux: 115 cases recorded by
`posix/tools/oracle/interrupt_harness.py` and replayed by
`interrupt::tests::every_interruption_is_glibcs`. `sem_wait`, the four
message-queue calls, and `aio_suspend`, `gai_suspend` and `futex(FUTEX_WAIT)`
without a timeout end for a handler without `SA_RESTART`; `sem_timedwait`,
`sem_clockwait`, System V's `msgsnd`, `msgrcv`, `semop` and `semtimedop`,
`io_getevents`, and the timed `aio_suspend`, `gai_suspend` and `futex` for
any handler (design-decisions §1156, which records why `SA_RESTART` does not
restart the second group). The thread functions and `getaddrinfo_a(GAI_WAIT)`
still end for none, as glibc's do. Two things came with it: `io_pgetevents`
now holds its signal mask for the call -- it read it and ignored it, which
was harmless only while nothing could interrupt the call -- and reads its
timeout before the mask, as Linux does; and Linux's `futex()`, which had the
opposite fault (`EINTR` for every signal, ignored or `SA_RESTART` alike),
restarts as Linux's kernel does, to the same deadline. A ring-3 fixture,
`services/ctest-eintr`, runs the kernel's half -- a real signal ending a real
futex wait -- for `sem_wait`, `sem_timedwait`, `mq_receive` and `msgrcv`; its
rung is lane A's (`requests/d-a-run-the-ctest-eintr-fixture.md`). The same
fault in the calls the kernel itself sleeps in is
`D-POSIX-KERNEL-WAITS-END-WITH-EINTR-FOR-EVERY-SIGNAL`.

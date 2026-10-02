## D-POSIX-KERNEL-WAITS-END-WITH-EINTR-FOR-EVERY-SIGNAL — a native program's blocking system call fails with EINTR for any signal its trampoline takes: one it ignores, a child's exit, one whose handler asked for SA_RESTART (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/interrupt.rs`, `posix/src/lowlevellock.rs`)**

**In short:** a program here that is blocked in a system call the kernel
itself sleeps in -- `read` from a terminal or a pipe, `waitpid`, `accept`,
`recv` -- has it fail with -1 and `errno` `EINTR` whenever a signal reaches
its process, whatever the signal does: one the program ignores; `SIGCHLD`,
which every child's exit sends and which is ignored by default; one whose
handler was installed with `SA_RESTART`; one another thread handles. On
Linux none of those ends the call -- an ignored signal is never delivered,
and `SA_RESTART` restarts the call. A program that retries on `EINTR` does
not notice, but one that takes it for an error fails where on Linux it
would have waited on.

**Why:** a native process's dispositions are this library's
(`posix/src/signal.rs`). The kernel knows only that the process registered a
trampoline, so it hands the trampoline every catchable signal, and as it
builds the trampoline's frame it turns any restart sentinel into
`KernelError::Interrupted` (`deliver_pending_signal`,
`kernel/src/syscall/handlers.rs`: "native handlers cannot request
SA_RESTART"). The call returns that when the trampoline is done, and the
library's wrapper answers `EINTR`. The calls that wait inside the library
had the same fault and no longer do (`D-POSIX-FUTEX-WAITS-DISCARD-EINTR`).

**The proper fix:** the same one, in the library's wrappers of the kernel's
blocking calls, with no change to the kernel, which must still end the sleep
and still cannot see a disposition. The dispatch already counts, per thread,
the handlers it runs and those without `SA_RESTART`
(`posix/src/interrupt.rs`). A wrapper whose call comes back `Interrupted`
compares the counts with a mark taken before the call, and issues the call
again when no handler ran on the thread -- or, for a call Linux restarts
under `SA_RESTART`, when only such handlers ran. Which calls those are is
signal(7)'s two lists: `read`, `write` and `ioctl` on slow devices, `open` of
a FIFO, `wait4` and the other waits, the socket calls without a timeout,
`flock` and `fcntl(F_SETLKW)` restart; `poll`, `select`, `epoll_wait`, the
sleeps, `sigsuspend`, `pause` and `sigtimedwait`, and the socket calls with
a timeout never do, and answer `EINTR` for any handler -- but must still go
on for a signal that ran none. A restarted timed call needs its remaining
time, as Linux's restart block keeps it. glibc on Linux is the oracle here as
it was for the waits: `interrupt_harness.py`'s shape, over those calls.

**Where:** the wrappers of the blocking system calls in `posix/src/`
(`unistd.rs`'s `read` and `write`, `process.rs`'s `waitpid`, `socket.rs`,
...); the kernel's side is `deliver_pending_signal` in
`kernel/src/syscall/handlers.rs`.

**Fixed 2026-09-30.** The library's wrappers of the kernel's blocking calls
restart as Linux's kernel restarts them (`interrupt::restarting`): `read`
and `write` on a pipe, a socket pair, the terminal and a pty, the eventfd
read, and every wait for a child (`process.rs`'s `wait_common`) are issued
again for as long as the kernel ends them for a signal that ran no handler
on the thread, or only handlers installed with `SA_RESTART`. The loops the
library builds from the kernel's non-blocking calls count handlers from the
call's start and ask after each look: `poll`, `ppoll`, `select`, `pselect`,
`epoll_wait` and its two end for any handler; the TCP and UDP waits,
`accept` and `flock` for one without `SA_RESTART` -- or for any, on a socket
with a timeout; the timerfd and inotify reads as `read` does. Their slices,
and every sleep (`sleep`, `nanosleep`, `usleep`, `clock_nanosleep`), are
timed futex waits on a word of their own, which the kernel ends for a signal
where `SYS_SLEEP` sleeps its full time: a handler ends a sleep at once, with
the time left (design-decisions §1157). `pause` and `sigsuspend` wait for a
handler on their own thread the same way instead of polling a process-wide
count every 2 ms; `sigsuspend` counts from before it sets its mask, which
closed a lost wake-up. And `ppoll`, `pselect` and `epoll_pwait` hold their
signal masks for the call, which they had ignored (`signal::under_mask`).
glibc's answers for 25 more calls -- 240 cases in all -- are in
`interrupt_oracle.txt`: the host replays the sleeps, `pause` and
`sigsuspend`, and holds a table of the other calls' rules to glibc's lines;
`services/ctest-eintr` gained six ring-3 checks (a pipe read through an
`SA_RESTART` handler and through a child's `SIGCHLD`, `nanosleep`, `poll`,
`waitpid`, `pause`). Still to do: `sigwait`, `sigtimedwait` and `sigwaitinfo`
are stubs (`D-POSIX-SIGWAIT-AND-SIGTIMEDWAIT-ARE-STUBS`). Moot for now: a
FIFO and a signalfd cannot be made here, and a record lock is granted at
once, so nothing waits in `F_SETLKW`.

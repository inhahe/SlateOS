## D-POSIX-SA-SIGINFO-HANDLERS-WERE-CALLED-WITH-ONE-ARGUMENT — a handler installed with `SA_SIGINFO` was called as `handler(sig)`, and read its `siginfo_t *` and `ucontext_t *` from whatever was left in two registers (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/signal.rs`)**

**In short:** a C program that installs a signal handler with `SA_SIGINFO`
declares it `void handler(int sig, siginfo_t *info, void *context)` and may
read `info->si_code`, `info->si_pid` or the interrupted registers through
`context`. This library called every handler with the signal number alone,
so those two pointers were whatever the dispatch happened to leave in the
`rsi` and `rdx` registers: a handler that read them read garbage, or
crashed.

**Why:** the dispatch (`dispatch_self_signal`) stored `SA_SIGINFO` with the
rest of the action and never consulted it; `run_handler` took an
`extern "C" fn(i32)`. The kernel side was not involved -- the trampoline was
handed the frame of the interrupted registers all along, and passed only
the number on.

**Fixed 2026-09-30.** An `SA_SIGINFO` handler is called with a `siginfo_t`
and a `ucontext_t`, on the alternate stack when it asked for it. The
`siginfo_t` says what the dispatch knows: `SI_TKILL` with this process's
pid and uid for `raise`, `abort` and `pthread_kill`, `SI_USER` likewise for
`kill` of its own pid, `SI_USER` with no sender for a signal the kernel
delivered (`D-POSIX-SIGINFO-FROM-THE-KERNEL-IS-THE-NUMBER-ALONE`). The
`ucontext_t` holds the mask as the signal came, the alternate stack as
registered, the floating-point control state its `fpregs` points at, and
-- from the kernel's frame -- the interrupted registers, which the handler
may change: they are resumed as it leaves them, and so is `uc_sigmask`, as
Linux's `rt_sigreturn` has it.

**Where:** `posix/src/signal.rs` (`run_siginfo_handler`, `siginfo_for`,
`call_handler`, `__call_on_alt_stack`).

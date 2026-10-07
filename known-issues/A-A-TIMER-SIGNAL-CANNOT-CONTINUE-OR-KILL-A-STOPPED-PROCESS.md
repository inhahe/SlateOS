### A-A-TIMER-SIGNAL-CANNOT-CONTINUE-OR-KILL-A-STOPPED-PROCESS -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A), found while writing POSIX timers (§1540). Applies
to `ITIMER_REAL` too.

**In short:** a timer can be told to send any signal, `SIGCONT` and `SIGKILL`
included. When `kill` sends one of those to a stopped program, the kernel
continues it or ends it on the spot. When a timer sends it, the kernel only
queues it, and a stopped program never gets to look at its queue: a timer's
`SIGCONT` does not wake it, and a timer's `SIGKILL` or deadly `SIGALRM` waits
until something else continues it.

**Where:** `proc::posix_timer::fire` and `proc::itimer::real_fire` run in the
timer interrupt and can only queue (`signal::post_timer_signal`,
`signal::set_pending_info`). Stopping, continuing and ending a process
(`handlers::post_signal` → `stop_process_for_signal`, `continue_process`,
`kill_process_threads`) take locks an interrupt may not wait for. A running
or sleeping process is fine: the queued signal wakes its interruptible
sleep, and the delivery checkpoint carries out `SIGKILL`'s and `SIGSTOP`'s
default actions (which never reach a handler).

**Proper fix:** hand the post to a kernel thread when the target is stopped
and the signal is `SIGCONT` or fatal -- a small queue of `(pid, signal,
record)` the interrupt fills and the thread drains through `post_signal` --
or make the continue and the kill safe to start from an interrupt (set the
state and wake the threads, leaving the rest to them), as Linux's
`prepare_signal` and `complete_signal` do.

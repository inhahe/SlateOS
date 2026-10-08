### A-A-TIMER-SIGNAL-CANNOT-CONTINUE-OR-KILL-A-STOPPED-PROCESS -- 2026-10-07 -- OPEN until its fix (lane-a-wip, 2026-10-08) has a boot on main (lane A)

**Status:** OPEN -- fixed on lane-a-wip 2026-10-08, awaiting a boot on main.
Found while writing POSIX timers (§1540).

**In short:** a timer can be told to send any signal, `SIGCONT` and `SIGKILL`
included. When `kill` sends one of those to a stopped program, the kernel
continues it or ends it on the spot. When a timer sent it, the kernel only
queued it, and a stopped program never looks at its queue: a timer's
`SIGCONT` did not wake it, and a timer's `SIGKILL` waited until something
else continued it.

**Corrected 2026-10-08:** this entry also said a timer's deadly `SIGALRM`
should end a stopped program. It should not: POSIX lets a stopped process
take only `SIGKILL` and `SIGCONT` until it is continued, and Linux's
`wants_signal` passes a stopped thread over for anything else -- measured
(`build/stoppedsigtest.c` on Linux 6.6.87). Waiting for `SIGCONT` was right
for the timer; it was `kill` that was wrong the other way: `kill -TERM` ended
a stopped program with no handlers on the spot.

**The fix** (`syscall::handlers`):
- `fire` (POSIX timers) hands a `SIGCONT` or `SIGKILL` it queued to the work
  queue (`defer_act_on_stopped`: a fixed, interrupt-safe list of
  `(pid, signal)`), whose `act_on_stopped` continues or ends the process if
  it is stopped.
- `post_signal`: a deadly signal other than `SIGKILL` to a stopped process is
  left pending; the delivery checkpoint ends it after a `SIGCONT`.

**Tested by** `spawn::self_test_linux_stopped_signals`
(`build/stoppedsigtest.c`), every case checked against Linux 6.6.87.

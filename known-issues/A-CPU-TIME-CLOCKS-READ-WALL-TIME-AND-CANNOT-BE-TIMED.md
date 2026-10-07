### A-CPU-TIME-CLOCKS-READ-WALL-TIME-AND-CANNOT-BE-TIMED -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A). The reading half was noted in todo.txt
(`CLOCK_PROCESS_CPUTIME_ID(2) / CLOCK_THREAD_CPUTIME_ID(3) still return
monotonic ns`); the timer half is new with POSIX timers (§1540).

**In short:** a program can ask how much processor time it has used, and set
a timer that fires after it has used so much. Here the first answers how long
the computer has been up instead, and the second is refused. So C's
`clock()` measures wall time, a benchmark that compares CPU time with elapsed
time sees them equal, and a CPU-time limit set with a timer cannot be set.

**Where:**

- `clock_gettime(CLOCK_PROCESS_CPUTIME_ID / CLOCK_THREAD_CPUTIME_ID)` and the
  per-process/per-thread CPU clocks glibc passes as negative ids
  (`syscall::linux::sys_clock_gettime`) read `hrtimer::now_ns()`.
- `timer_create` on any of them answers `EOPNOTSUPP` after checking the id
  (`syscall::linux::timer_create_common`, `TimerClockKind::CpuTime`).
- `setitimer(ITIMER_VIRTUAL / ITIMER_PROF)` have no timer behind them either
  (`proc::itimer` is `ITIMER_REAL` only).
- The scheduler does count CPU time per task, but in ticks
  (`sched::cpu_ticks`, `thread::process_cpu_ticks`, used by `getrusage` and
  `times`), 10 ms apart.

**Proper fix:** account each task's run time precisely -- stamp the TSC when a
task is switched in and add the difference when it is switched out (and at
each tick for the running task) -- split user from system time at the
syscall and interrupt boundaries the tick accounting already uses. Then the
CPU clocks read that sum (the thread's, or every thread's of the process),
and a CPU-time timer -- POSIX timer, `ITIMER_VIRTUAL`, `ITIMER_PROF` -- is
checked at each tick and switch-out of a task of its process (Linux's
`run_posix_cpu_timers`), queuing its signal through the same path as the
other timers (`posix_timer`'s `fire`).

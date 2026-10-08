### A-CPU-TIME-CLOCKS-READ-WALL-TIME-AND-CANNOT-BE-TIMED -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A). The reading half is **fixed on lane-a-wip
2026-10-08, awaiting a boot** (design-decisions §1549); the timer half is
next.

**In short:** a program can ask how much processor time it has used, and set
a timer that fires after it has used so much. The first answered how long the
computer had been up; it now answers with the time each thread has run. The
second is still refused, so a CPU-time limit set with a timer cannot be set.

**Fixed (reading):** `clock_gettime`/`clock_getres`/`clock_settime` on
`CLOCK_PROCESS_CPUTIME_ID`, `CLOCK_THREAD_CPUTIME_ID` and the per-process and
per-thread ids (`syscall::linux::cpu_clock`, `read_cpu_clock`), and natively
`SYS_CPU_CLOCK` (1156): the precise run time for `CPUCLOCK_SCHED`, the
sampled ticks for PROF and VIRT. Ring-3 tests `self_test_linux_cpu_clocks`
(12/12 on Linux 6.6.87) and `self_test_native_cpu_clock`.

**Still open (timing):**

- `timer_create` on any CPU-time clock answers `EOPNOTSUPP` after checking
  the id (`syscall::linux::timer_create_common`, `TimerClockKind::CpuTime`),
  natively too (`SYS_POSIX_TIMER`).
- `clock_nanosleep` on `CLOCK_PROCESS_CPUTIME_ID` sleeps on the monotonic
  clock; on a process's or thread's clock by id it answers `EINVAL`.
- `setitimer(ITIMER_VIRTUAL / ITIMER_PROF)` have no timer behind them
  (`proc::itimer` is `ITIMER_REAL` only).
- `RLIMIT_CPU` is recorded and enforced by nothing (`SIGXCPU` at the soft
  limit, `SIGKILL` at the hard one).

**Proper fix:** a CPU-time timer -- POSIX timer, `ITIMER_VIRTUAL`,
`ITIMER_PROF`, the `RLIMIT_CPU` limits, a CPU-clock sleep -- is checked at
each tick of a task of its process or thread (Linux's `run_posix_cpu_timers`,
cheap when the earliest expiry is cached where the tick can see it), and an
expiry queues its signal through the same path as the other timers
(`posix_timer`'s `fire`).

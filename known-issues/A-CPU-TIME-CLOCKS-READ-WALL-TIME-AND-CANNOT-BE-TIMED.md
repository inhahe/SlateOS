### A-CPU-TIME-CLOCKS-READ-WALL-TIME-AND-CANNOT-BE-TIMED -- 2026-10-07 (lane A)

**Status:** OPEN -- fixed on lane-a-wip (29e8dc55d, f7cee3e11), awaiting a
boot on main: the reading half (design-decisions §1549) and the timing half
(§1550). Stamp it FIXED and move it to `known-issues-resolved/` once a boot on
main has run the three ring-3 tests below.

**In short:** a program can ask how much processor time it has used, and set
a timer that fires after it has used so much. The first answered how long the
computer had been up, and the second was refused. Both now work as on Linux.

**Fixed (reading):** `clock_gettime`/`clock_getres`/`clock_settime` on
`CLOCK_PROCESS_CPUTIME_ID`, `CLOCK_THREAD_CPUTIME_ID` and the per-process and
per-thread ids (`syscall::linux::cpu_clock`, `proc::cputimer::CpuClock`), and
natively `SYS_CPU_CLOCK` (1156): the precise run time for `CPUCLOCK_SCHED`,
the sampled ticks for PROF and VIRT.

**Fixed (timing):** `timer_create` on every CPU-time clock (natively too,
through `SYS_POSIX_TIMER`); `clock_nanosleep` on `CLOCK_PROCESS_CPUTIME_ID`
and a process's or another thread's clock (natively `CPU_CLOCK_NANOSLEEP`);
`setitimer`/`getitimer` `ITIMER_VIRTUAL` and `ITIMER_PROF` (natively
`SYS_ITIMER_SET`/`GET` 1 and 2); `RLIMIT_CPU`'s `SIGXCPU` and `SIGKILL`. All
checked at the tick (`sched::cpu_timers_due`, `proc::cputimer::expire`,
`posix_timer::expire_cpu`).

**Tests:** `self_test_linux_cpu_clocks`, `self_test_native_cpu_clock`,
`self_test_linux_cpu_timers` (ring 3; the Linux ones twelve of twelve on
Linux 6.6.87), and `cputimer::self_test`.

`RLIMIT_RTTIME` too, on the same tick (`A-RLIMIT-RTTIME-IS-NOT-ENFORCED`).

### A-RLIMIT-RTTIME-IS-NOT-ENFORCED -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN (lane A).

**In short:** a real-time thread (`SCHED_FIFO`, `SCHED_RR`) outranks every
ordinary one, so a real-time thread stuck in a loop starves the machine.
Linux lets a program cap that: `RLIMIT_RTTIME` is how long a real-time thread
may run without sleeping before it is sent `SIGXCPU` (soft limit, again each
second after) and then `SIGKILL` (hard limit). Here the limit is recorded and
reported by `getrlimit`, and enforced by nothing.

**Where:** `RLIMIT_RTTIME` (15) is stored with the other limits
(`pcb::set_rlimit`); nothing reads it. The CPU-time timer machinery that
`RLIMIT_CPU` now uses (`proc::cputimer`, design-decisions §1550) is where it
belongs. The real-time band's own throttle (`sched.rt_runtime_pct`) limits
the band as a whole, not a thread.

**Proper fix:** count, per thread, the ticks it has run under a real-time
policy since it last blocked (Linux's `p->rt.timeout`: reset when the thread
sleeps, charged at each tick while it runs real-time), and check it at the
tick against the process's limit in microseconds, cached where the tick can
read it (as `RLIMIT_CPU`'s thresholds are, in the process's
`ProcCpuAccount`): `SIGXCPU` at the soft limit and every second after (Linux
moves the soft limit on), `SIGKILL` at the hard one -- Linux's
`check_thread_timers`. Test it with a ring-3 program that sets the limit,
takes `SCHED_FIFO` (needs `RLIMIT_RTPRIO` or the real-time right) and spins.

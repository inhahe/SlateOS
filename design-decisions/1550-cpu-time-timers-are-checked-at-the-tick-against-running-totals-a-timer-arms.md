## 1550. CPU-time timers are checked at the tick, against running totals a timer arms

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can ask to be told when it has used so much processor
time: a timer on a CPU-time clock (`timer_create`), `ITIMER_PROF` and
`ITIMER_VIRTUAL` (`setitimer` -- what profilers such as `gprof` and Go's
`pprof` use), a limit on CPU seconds (`RLIMIT_CPU`, which ends a runaway
program), or a sleep until a clock reads a time (`clock_nanosleep`). All of
them were refused or did nothing. Now each fires as on Linux: at the first
timer tick (every 10 ms) at which the clock has reached its time -- up to a
tick late, never early.

**How:** there is no moment to set an interrupt for, because a CPU clock only
moves while its threads run. So, as Linux does, every tick looks: it compares
the interrupted thread's time with the earliest expiry armed on that thread's
clocks, and its process's running total with the earliest armed on the
process's. A process's running total is a few counters every thread of it
shares (`sched::ProcCpuAccount`), which the scheduler adds to at each switch
and tick -- but only once a timer has been armed on that process's clock.

**The choices:**

| decision | alternatives | why this one |
|---|---|---|
| Check at the tick (`sched::cpu_timers_due`), fire from it (`cputimer::expire`, `posix_timer::expire_cpu`) | a deferred check in the work queue at each tick of a process with timers; an `hrtimer` set for the earliest moment the clock could get there | Linux's way and its resolution; the check is a few comparisons when nothing is due, and firing takes only locks the interrupt may take (the timer tables, the signal registries; the scheduler's only tried) |
| A process's running total kept from the first arming on for the process's life (`pcb::activate_cpu_account`) | always kept; kept and stopped as timers come and go (Linux) | always kept puts a shared atomic add on every context switch of every process; stopping it again races the arming that needs it, for a saving no one measures |
| The total is filled under the process table's lock and then the scheduler's, as every charge is made under the scheduler's | fill it from a clock read | exact from the first tick on, except that a thread exiting at that very moment counts the microseconds of its exit path twice (documented at `sched::activate_cpu_account`) |
| Each source keeps its own slot of earliest expiry in the account (POSIX timers per measure, the two itimers, `RLIMIT_CPU`) | one slot per measure | two sources sharing a slot would overwrite each other's expiry; with one each, only the owner of a slot writes it, under its own lock |
| A thread's earliest expiries live in its task, updated with the timer table held -- the scheduler's lock taken inside it from process context, only tried from the interrupt | keep them in the timer table | the tick reads them with the scheduler's lock held, and taking the timer table there would invert `TABLE` → scheduler; a cache the interrupt fails to update is left earlier than the truth, which costs a look at the next tick, never a missed expiry |
| `clock_nanosleep` on a CPU clock wakes when the clock could first have got there -- the time left over the CPUs that can move it, at least a tick -- and looks again | arm a kernel-internal CPU timer that wakes the sleeper; poll every tick | no second kind of timer, and no 100 wakeups a second: a clock that runs flat out is caught within a tick, one that does not run at all costs a wakeup per the time left |
| An expiry already passed when the timer is set fires at once; a setitimer value gets a tick added; `RLIMIT_CPU`'s soft limit is raised a second at each `SIGXCPU` and `getrlimit` shows it; a timer whose thread or process has gone cannot be set (`ESRCH`) and reads zeros | -- | each is Linux 6.6's behaviour, measured on the host (`build/cpuclock_probe.c`, `build/cputimertest.c`, twelve of twelve) |

**Not done:** `RLIMIT_RTTIME` (a real-time thread's CPU time without
sleeping) is still enforced by nothing
(`known-issues/A-RLIMIT-RTTIME-IS-NOT-ENFORCED.md`).

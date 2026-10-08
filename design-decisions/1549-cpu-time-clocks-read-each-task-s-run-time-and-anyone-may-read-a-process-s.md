## 1549. CPU-time clocks read each task's run time, and anyone may read a process's

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can ask how much processor time it, or one of its
threads, has used -- `clock()`, `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)`,
and the clocks `clock_getcpuclockid` and `pthread_getcpuclockid` hand out.
Until now every one of them answered with how long the machine had been up.
Now they answer with the time the scheduler has measured each thread running,
to the nanosecond, summed over a process's threads for a process. As on
Linux, a thread's clock can be read only by its own process, and any
process's clock by anyone.

**What changed:**
- The scheduler already charged each task the TSC cycles it ran, at every
  switch-out (`Task::total_cycles`). A reading now adds the cycles since the
  task was switched in if it is running (`sched::cpu_sample`), and a process's
  is its live threads' plus those its exited threads left behind
  (`Process::acct_cycles`, `pcb::process_counters`).
- `clock_gettime`, `clock_getres` and `clock_settime` decode the CPU-time
  clock ids as Linux does (`syscall::linux::cpu_clock`): 2 and 3, and
  `~pid << 3 | perthread << 2 | which`. Native programs read the same clocks
  with the same ids through `SYS_CPU_CLOCK` (1156).
- `clock_settime(CLOCK_PROCESS_CPUTIME_ID)` is `EINVAL`, not `EPERM`, as
  Linux 6.6.87 answers; a process's clock by id is `EPERM`.

**The choices:**

| decision | alternatives | why this one |
|---|---|---|
| `CPUCLOCK_SCHED` (and so 2 and 3) reads the precise switch-to-switch run time; `CPUCLOCK_PROF` and `CPUCLOCK_VIRT` read the sampled ticks (user plus system, user), a whole 10 ms tick each | PROF from the precise time too; VIRT as the precise time scaled by the tick ratio, as Linux's `getrusage` reports | it is what Linux does under tick accounting, the default on its distributions, and it keeps each clock in step with the timer that will run on it (`ITIMER_PROF`, `ITIMER_VIRTUAL` are charged at the tick). A scaled VIRT can run backwards when the ratio shifts, which a clock must not |
| Any process's clock can be read by anyone; a thread's by its own process only | require `pcb::may_inspect` (the `/proc` rule, §1516) or a `Process` capability | Linux's rule, and §1516 already leaves `/proc/<pid>/stat`, which carries the same CPU times, open to all; a stricter native door would be no barrier, since a Linux program asks through the open one. Revisit with §1516 when users other than root exist |
| A process's total is read under the process table's lock with the scheduler's taken inside it, and a thread's time is folded into its process under the same lock as it leaves `threads` (`pcb::remove_exiting_thread`) | read the exited threads' sum and the live threads separately, as `getrusage` did; or keep a per-process running total updated at every context switch | read separately, a thread that exits between the two reads is missed entirely and the clock runs backwards; a running total puts a shared write on every switch of a multi-threaded process, which Linux too avoids unless a process CPU timer is armed. The lock order `PROCESS_TABLE` → `SCHED` is the documented one, and the scheduler never takes the process table |
| The in-flight part of a running task on another CPU compares this CPU's TSC with that one's stamp | read only what was charged at the last switch, as Linux does for other CPUs' tasks | an invariant TSC agrees across CPUs to a few cycles, and a reading that would go negative counts nothing; it makes a busy sibling thread's clock current rather than up to a time slice behind |

`getrusage`, `times`, `wait4`'s usage and `/proc/<pid>/stat` read the same
snapshot (`pcb::process_counters`), so they no longer miss a thread that
exits mid-read. They still report ticks; Linux scales its ticks to the precise
total (`cputime_adjust`), which would make short runs show their real time
rather than 0 or 10 ms -- left for later.

**Revisit** if CPU-time timers (`timer_create` on these clocks, `ITIMER_PROF`,
`ITIMER_VIRTUAL`, `RLIMIT_CPU`), which come next, need a per-process running
total after all.

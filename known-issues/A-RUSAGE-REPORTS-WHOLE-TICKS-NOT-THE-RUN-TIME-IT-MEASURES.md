### A-RUSAGE-REPORTS-WHOLE-TICKS-NOT-THE-RUN-TIME-IT-MEASURES -- 2026-10-08 (lane A)

**Status:** OPEN -- fixed on lane-a-wip (6186944ef), awaiting a boot on main.
Stamp it FIXED and move it to `known-issues-resolved/` once a boot on main has
run the self-tests named below.

**In short:** `time ./program` and `getrusage` say how much user and system
time a program used. They said it in whole 10 ms ticks -- a program that ran
4 ms showed 0, one that ran 15 ms showed 10 or 20 -- although the kernel
measures every thread's run time to the nanosecond (design-decisions §1549).
They now report what Linux does: the precise run time, split between user and
system in the proportion the ticks saw.

**Fix:** `sched::adjust_cputime` (Linux's `cputime_adjust`): all user while no
tick has found the kernel, all system while none has found user code,
otherwise the tick ratio; neither half ever goes back, held by the last split
given for the same process (`Process::prev_cputime`, `pcb::process_times`) or
thread (`Task::prev_cputime`, `sched::thread_times`). `ProcessUsage` and the
reaped-children accumulators (`Process::child_utime_ns`/`child_stime_ns`)
carry nanoseconds; a reaped child adds its split and its own children's, as
`wait_task_zombie` does. `getrusage` (all three `who`), `times`, `wait4`,
`waitid`'s `si_utime`/`si_stime`, `/proc/<pid>/stat` (the process's, as
Linux's `do_task_stat(whole)`) and `/proc/<pid>/task/<tid>/stat` (the
thread's), and the native `SYS_PROCESS_GET_RUSAGE` and wait-status images all
read it; the tick-unit ones truncate to `clock_t` as `nsec_to_clock_t` does.
The tick counts stay for the PROF and VIRT clocks and the itimers.

**Tests:** `cputimer::self_test` (the split's arithmetic and its never going
back), `pcb::test_cpu_time_accounting` (children's splits carried up),
`test_dispatch_wait_info_layout`/`rusage_info_layout` and the waitid and
`/proc` stat encoders (nanoseconds truncated to their units).

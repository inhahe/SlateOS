### A-RLIMIT-RTTIME-IS-NOT-ENFORCED -- 2026-10-08 (lane A)

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip (024ba92bd), on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264)
(design-decisions §1550). Stamp it FIXED and move it to
`known-issues-resolved/` once a boot on main has run
`self_test_linux_cpu_timers`, whose eighth part tests it.

**In short:** a real-time thread (`SCHED_FIFO`, `SCHED_RR`) outranks every
ordinary one, so a real-time thread stuck in a loop starves the machine.
Linux lets a program cap that: `RLIMIT_RTTIME` is how long a real-time thread
may run without sleeping before it is sent `SIGXCPU` (soft limit, again each
second after) and then `SIGKILL` (hard limit). The limit was recorded and
reported by `getrlimit`, and enforced by nothing.

**Fix:** each thread counts the ticks it runs under a real-time policy while
its process's soft limit is finite (`Task::rt_run_ticks`, Linux's
`rt.timeout`), cleared when it blocks; the tick checks the count against the
limits, which `setrlimit` and `fork` copy into the process's CPU account
(`proc::cputimer::rlimit_rttime_changed`): past the earlier of the two,
`proc::cputimer::rttime_expire` sends `SIGKILL` at the hard limit or `SIGXCPU`
at the soft one, moving the soft limit on a second (shown by `getrlimit`) --
Linux's `watchdog` and `check_thread_timers`.

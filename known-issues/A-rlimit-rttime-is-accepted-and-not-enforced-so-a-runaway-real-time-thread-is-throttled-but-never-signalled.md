### [A] `RLIMIT_RTTIME` is accepted and not enforced, so a runaway real-time thread is throttled but never signalled -- 2026-10-07

**Status:** OPEN

**What is missing.** Linux sends a real-time thread `SIGXCPU` when it has run
`RLIMIT_RTTIME` microseconds of CPU without blocking (the soft limit), and
`SIGKILL` at the hard limit. Audio servers depend on it: PipeWire and
PulseAudio set a limit (rtkit requires one, typically 200 ms) before asking
to be made real-time, so that a bug spinning their audio thread kills the
thread rather than the desktop. Here `setrlimit(RLIMIT_RTTIME, ...)` stores
the value (`pcb.rs`'s rlimit table, index 15) and nothing reads it.

**What protects the machine meanwhile.** The real-time throttle
(design-decisions 1544, `sched::rt_account_tick`): band work gets at most
`sched.rt_runtime_pct` (95%) of each second of a CPU on which ordinary work
waits, so a spinning real-time thread slows the rest of the system to a
twentieth rather than freezing it. What is lost is the program's own safety
net: it is never told, and goes on spinning.

**Where the fix goes.** `sched::timer_tick` already charges every tick to the
running task (`Task::tick_burst`). A per-task "ticks since this real-time task
last blocked" counter -- reset in `mark_ready` from `Blocked` and on a policy
change, as Linux's `p->rt.timeout` is -- compared against the owning
process's `RLIMIT_RTTIME` (soft: `SIGXCPU` once per second past it; hard:
`SIGKILL`), raised from the tick through the deferred-signal path the CPU
timers use (`RLIMIT_CPU`'s `SIGXCPU`, if present, is the model). The tick is
10 ms, so the check is good to a tick, as Linux's is to a jiffy.

**How to see it.** A ring-3 program that sets `RLIMIT_RTTIME` to 50 ms,
becomes `SCHED_FIFO` and spins for a second: on Linux it takes `SIGXCPU`; here
it spins to the end.

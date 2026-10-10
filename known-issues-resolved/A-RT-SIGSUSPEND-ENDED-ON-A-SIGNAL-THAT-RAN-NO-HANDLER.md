### A-RT-SIGSUSPEND-ENDED-ON-A-SIGNAL-THAT-RAN-NO-HANDLER -- 2026-10-08 (lane A)

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip 2026-10-08, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264).
Stamp it FIXED and move it to `known-issues-resolved/` once a boot on main has
run `self_test_linux_ptrace_tier2`, whose fifth part tests it.

**In short:** `sigsuspend` is how a program says "sleep until a signal I
handle arrives". Ours woke up and returned for any signal it let through,
even one that then did nothing -- one the program ignores by default, a stop
followed by a continue, or one a debugger took away. On Linux the call goes
back to sleep in those cases, and only a signal that runs a handler (or ends
the program) ends it.

**Where:** `syscall::linux::sys_rt_sigsuspend` answered `-EINTR` once a
signal its temporary mask lets through was pending. Linux's answers
`-ERESTARTNOHAND`, which the signal checkpoint turns into `-EINTR` when a
handler's frame is built and into a restart of the call -- its saved mask put
back first -- when none is (`arch_do_signal_or_restart`). POSIX says the same
in other words: sigsuspend "shall return after the signal-catching function
returns".

**What it broke:** a program waiting in `sigsuspend` under a debugger saw
the call fail whenever the debugger suppressed a signal (GDB's `handle ...
nopass`); one that was stopped (`SIGTSTP`) and continued saw it fail too,
where it should have gone on waiting; and a traced program's default-ignored
`SIGWINCH` or `SIGCHLD` -- a tracee's ignored signals are queued, so that its
tracer sees them -- ended its wait.

**Fixed:** the call answers `restart::ERESTARTNOHAND`, as `pause` already
did. Found while testing `PTRACE_SETSIGMASK` at a stop inside the call
(`build/ptracetier2test.c` part 5, which Linux 6.6 passes twelve times of
twelve: the call stopped at with `rax` `-ERESTARTNOHAND`, restarted once its
`SIGWINCH` is taken away, and ended with `EINTR` by a handled `SIGUSR1`).

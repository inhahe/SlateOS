## §112 — Job-control wait status gets a new syscall number, not new arguments on `SYS_PROCESS_WAIT`

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** §111 gave a process a way to *stop* itself. That is only half a
job-control implementation: a shell also has to *observe* the stop from the
parent side. Our native `waitpid` accepted `WUNTRACED`/`WCONTINUED` and then
dropped them — it called `SYS_PROCESS_WAIT` (501), which reports only a death.
So the question was how to get an options word and a wait *status* to and from
the kernel.

**Decision.** A new syscall, `SYS_PROCESS_WAIT_STATUS` (1063), taking
`(pid, options, status_out)`. `SYS_PROCESS_WAIT` (501) is left exactly as it
was.

**Why not extend 501 — two independent reasons, either one sufficient.**

1. *Its return channel is full.* 501 returns the child's exit code in `rax`.
   Every 64-bit value is a legitimate exit code, so there is no bit pattern
   left to mean "stopped" or "continued". Adding a status out-pointer would
   mean 501's callers must now read a second output they were never told
   about.
2. *Its argument registers already hold garbage.* This is the decisive one, and
   it was already written down: 501's own specific-pid path refuses to use
   `arg1` because "callers using a 1-arg syscall wrapper leave a stale
   (possibly valid) pointer in that register". An options word read from such a
   register would enable random option bits; a status pointer read from one
   would be **written through** — a wild store into a caller that did nothing
   wrong. A new syscall number is the only way to add arguments to a syscall
   whose existing callers do not set them.

**Against.** It is one more number in the table, and two syscalls now answer
"wait for a child", which is a maintenance hazard if they drift: they read the
same kernel records, so they must not disagree about which children a selector
names, which transition is reported first, or what wstatus bits describe it.

**How that hazard is contained** (per §109, "move the fanout down, not
sideways"): rather than duplicating `sys_wait4`'s 185-line blocking loop, that
body moved *down* into `handlers::wait_for_child_event`, and the two wstatus
encoders moved down into `pcb` as `JobControlEvent::to_wstatus` and
`ExitInfo::to_wstatus`. Both ABIs are now thin adapters over one implementation,
so they cannot drift — drift would require someone to deliberately re-fork the
shared core. `sys_wait4` shrank from 184 lines to 32 in the process.

**Alternative considered: a `waitid`-style syscall taking a `siginfo_t`.** That
is what Linux would suggest, and it generalises better (`P_PID`/`P_PGID`/`P_ALL`
selectors, `WNOWAIT`). Rejected for now because it needs a userspace struct
layout the kernel must write field-by-field, which is a larger trust boundary
than a single `i32`, and because our `waitid` already synthesises its result
from the same records in userspace. If `waitid` ever needs to be authoritative,
it should be built on `wait_for_child_event` too rather than beside it.

**Testing.** `dispatch::test_dispatch_wait_status_reports_job_control` drives
nine fixture cases with synthetic `record_jc_stopped`/`record_jc_continued`
records — all with `WNOHANG`, because the boot self-test task must not park —
covering option validation, invisibility without `WUNTRACED`, single-consumption
of each report, that a stop does not reap, and the zombie→`ECHILD` sequence.
The blocking path and the two-process case belong to the ring-3 fixture
`services/ctest-jobctl`, which is the only layer that can observe them.

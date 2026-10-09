### A-JOB-CONTROL-STOP-SUSPENDS-A-THREAD-WHEREVER-IT-IS -- 2026-10-09 -- OPEN (lane A)

**Status:** OPEN (lane A). Found while building the freezer
(design-decisions §1562).

**In short:** when one program stops another -- `kill -STOP`, a shell's
Ctrl-Z reaching a job that is not the one reading -- the kernel suspends each
of the stopped program's threads wherever it is at that moment. A thread in
the middle of a system call can be stopped there, holding whatever kernel
lock that call held; any other program that needs the same lock then waits
until the stopped one is continued. On Linux a stop takes effect only when
each thread is about to return to its program, holding nothing. Container
pause had the same flaw and moved onto the freezer the day this was found.

**Where:** `kernel/src/syscall/handlers.rs`, `stop_process_for_signal`: every
thread but the caller's is `sched::suspend`ed (`TaskState::Suspended`), which
takes effect at the thread's next switch wherever that is -- inside a wait
loop, or at a preemption point in kernel code. A self-stop is fine: the
stopping thread parks at its own signal checkpoint.

**How you would notice:** a stopped program's thread in a blocking call
that holds a sleeping lock across the wait (none found by reading yet, but
nothing prevents one) would hold it for as long as the program is stopped.
More concretely for §1126: a stopped thread is not at a point its registers
describe, so the kernel-replacement records cannot capture it, and the
freezer has to count it as at rest without knowing where it is.

**Proper fix:** make a group stop a reason to park at the same checkpoint
the freezer uses: post the stop, wake the target's interruptible waiters, and
let each thread stop itself at its next checkpoint (Linux's
`JOBCTL_STOP_PENDING` + `do_signal_stop`), with the parent told once every
thread has stopped (`WUNTRACED`). The freezer's `wait_ends` /
`park_if_frozen` machinery is that shape already. Job control is the
kernel's most race-prone code (see `stop_process_for_signal`'s two-phase
comment), so this wants its own change with the `ctest-jobctl` and
`ctest-pty` fixtures run several times over.

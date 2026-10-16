## 1547. ptrace: a debugger starts its program with the program's consent, and stops it as Linux does

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous; lane D proposed
the consent grant) · **Lane:** A

**In short:** a debugger -- GDB, LLDB -- controls another program: stops it,
reads and changes its registers and memory, plants breakpoints, runs it one
instruction at a time, and is told each time it stops. Linux does this with
`ptrace`, and the ports of both debuggers (lane D's) use its Linux back ends
unchanged. Nothing a program could call did any of it: `ptrace` answered
`EPERM`. Now the part a debugger needs to start a single-threaded program and
debug it works, for both ABIs -- the Linux `ptrace` and a native `SYS_PTRACE`
(1149) that takes the same arguments -- with Linux's answers throughout
(lane D's `requests/d-a-a-debugger-needs-ptrace-for-native-programs.md`,
"Tier 1").

**What works** (`kernel/src/proc/ptrace.rs`): `PTRACE_TRACEME`; the stop
after a traced exec (`SIGTRAP`, or `PTRACE_EVENT_EXEC` with
`PTRACE_O_TRACEEXEC`); a stop for every signal headed for the thread
(`SIGKILL` aside), where the tracer chooses what is delivered; a stop for
every exception it takes -- `int3` (`SIGTRAP`, `SI_KERNEL`), a single step
(`TRAP_TRACE`), a bad access; `wait` reporting each as `WIFSTOPPED` whether or
not `WUNTRACED` was asked for; `PTRACE_CONT`, `SINGLESTEP`, `DETACH`, `KILL`;
`GETREGS`/`SETREGS`, `PEEKUSER`/`POKEUSER` of the registers,
`GETSIGINFO`/`SETSIGINFO`, `GETEVENTMSG`, `SETOPTIONS` with `EXITKILL`;
`PEEK`/`POKE` of memory and `/proc/<pid>/mem`; `/proc/<pid>/status`'s
`TracerPid` and the `t (tracing stop)` state. And from lane D's Tier 2, the
registers a debugger cannot do without: the FPU and vector registers
(`GETFPREGS`/`SETFPREGS`; `GETREGSET`/`SETREGSET` of `NT_PRSTATUS`,
`NT_PRFPREG` and `NT_X86_XSTATE`) -- GDB reads them for `finish` from a
function returning a `double`, and saves them all around a call it makes in
the program -- and the debug registers, `PEEKUSER`/`POKEUSER` of
`u_debugreg`: GDB reads DR6 at every single step's stop and fails the step
if it cannot, and they are its hardware breakpoints and watchpoints. Also
`ARCH_PRCTL`.

**Choice 1 -- `PTRACE_TRACEME` grants the parent `DEBUG` over the caller's
process.** Design-decisions 24 (the operator's) gates introspection on a
`DEBUG` right the debugger holds over the target, never on pids or user ids.
A debugger that starts a program (`gdb ./prog`) forks a child that calls
`TRACEME` and then execs the program: the call is the target's own consent,
so it hands its parent the right, and every later request checks it again.
*What changes:* a debugger can debug what it starts, with no broker and no
prompt. The right stays with the parent after a detach, as Linux lets the
same user's process attach again. **Attaching** to a running program
(`PTRACE_ATTACH`, `SEIZE`) needs a right nothing hands out yet -- design
24's broker -- and is `EPERM`.

**Choice 2 -- a thread's stops are taken where Linux takes them, and its
registers live with the stop.** A signal's stop is on the thread's way back to
user mode, before the signal's disposition is looked at (Linux's
`ptrace_signal` in `get_signal`), so a traced process's signals are all
queued -- none acts when it is sent, an ignored one included. An exception's
stop is taken in the exception handler, before the fault is delivered or the
program ended, so a tracer that suppresses it runs the instruction again (or
the program on where it was moved), and one that passes it on gets exactly
what the program would have got untraced. The stopped thread's registers are
copied into its stop record; the tracer reads and writes the copy, and the
thread puts it back on its way out -- whole, `rcx` and `r11` included -- so no
pointer into another thread's kernel stack is ever kept.

**Choice 3 -- Linux's register image, Linux's selectors.** `GETREGS` gives
`struct user_regs_struct` with `cs` 0x33 and `ss` 0x2b, whatever this
kernel's selectors are (0x23 and 0x1b): GDB decides whether a program is
64-bit by `cs == 0x33`. `orig_rax` after an exec is execve's 59, as Linux's
`set_personality_64bit` leaves it; an interrupted call's restart value reads
as Linux's raw `-ERESTART*`, not this kernel's biased sentinel. `SETREGS`
refuses (`EIO`) what no user context may hold: a kernel `rip`, another code
selector.

**Choice 4 -- a debugger's write into code gets the program its own copy of
the page.** A breakpoint is an `int3` written into read-only text. The page
may be shared -- with the page cache, which holds a file's pages for every
mapper, or with a fork's sibling -- so it is copied first, as a
copy-on-write break copies, and the copy keeps the read-only protection
(`mm::cow::write_private`; Linux's `FOLL_FORCE`). The file and the other
mappers are untouched. A page shared by design is refused.

**Choice 5 -- `SETOPTIONS` takes every option Linux defines.** LLDB sets
`TRACECLONE`, `TRACEFORK`, `TRACEVFORK`, `TRACEEXIT` and `EXITKILL`
unconditionally and gives up if the call fails. All of them act (choices 8
to 11), with `TRACEEXEC`, `TRACEVFORKDONE` and `TRACESYSGOOD`;
`TRACESECCOMP` and `SUSPEND_SECCOMP` are accepted and have nothing to act
on, as no seccomp filter runs here. Attaching is the part of lane D's
"Tier 2" that is not done, tracked in `known-issues/A-ptrace-tier-2-*.md`.
*Easy to reverse:* the `option::MASK` check in `SETOPTIONS`.

**Choice 6 -- the FPU state is kept with the stop, as the general registers
are.** The stopping thread captures its FPU and vector registers
(`sched::fpu::capture_signal_image`, the signal frame's image) before it
publishes the stop; the tracer reads and writes that copy, shown as
`struct user_fpregs_struct` or as the XSAVE area with XCR0 in its first
software word (where GDB reads it); and the thread loads it back on its way
out if it changed. The alternative -- the tracer reading the save area the
switch-out fills -- races the stop: the tracer can see the stop before the
thread has switched out, read a stale area, or write one the switch-out then
overwrites. *What it costs:* an XSAVE and a small allocation per stop.

**Choice 7 -- the debug registers live on the thread's scheduler record and
are loaded at switch-in.** DR0-DR3 and DR7 are checked as Linux checks them
(user addresses; a length the CPU has, aligned, no I/O breakpoints;
`EINVAL`), DR7 reads back as written, and the CPU is given only the checked
breakpoints -- global, never `GD`. A thread switched in with none clears
the CPU's, so no other thread runs with them; a stop that ends before the
thread ever parked loads them itself (`sched::reload_current_user_state`,
which loads a changed `%fs`/`%gs` base the same way). A hit from user mode
is `SIGTRAP`/`TRAP_HWBKPT`, DR6 recording it as Linux's `virtual_dr6` does;
an execution breakpoint sets the resume flag so the instruction runs when the
thread goes back to it. A watchpoint the *kernel* hits -- `read(2)` filling a
watched buffer -- is dismissed with no record (Linux records it where only a
`PEEKUSER` of DR6 before the next user-mode debug exception could see it,
and sends no signal). An exec drops the thread's breakpoints, as Linux's
`flush_thread` does.

**Choice 8 -- what a traced program makes is traced as Linux traces it, and
its tracer gets `DEBUG` over a new process.** With `PTRACE_O_TRACECLONE`,
`TRACEFORK` or `TRACEVFORK` (or `CLONE_PTRACE`), a traced thread's new thread
or child is traced by the same tracer with the same options, from a
`SIGSTOP` stop before its first instruction -- taken in the trampoline that
first enters user mode, which delivers no signal -- and its creator stops at
`PTRACE_EVENT_CLONE`/`FORK`/`VFORK`, the new id its message. The program's
consent passes on to what it makes, so a new process's tracer is granted
`DEBUG` over it at its creation (`ptrace::attach_new`). *What changes:* a
debugger sees every thread and, if it asks, every child; GDB's default
(detach the child after a fork) works -- it removes its breakpoints from the
child before letting it go, where an untraced child ran into them and died.

**Choice 9 -- `DEBUG` also lets the debugger signal what it debugs.** Sending
a signal to another process needed its parenthood or a `DELETE` right; GDB
stops each thread with `tgkill(SIGSTOP)` and passes the user's ^C on, and an
auto-attached grandchild's tracer is not its parent. Holding `DEBUG` --
which already reads and writes every register and byte of the target --
now allows it (`handlers::check_signal_target`).

**Choice 10 -- a traced thread's end is its tracer's to wait for.** A
thread that is not its process's first, or a first thread whose tracer is
not its parent, is reported to its tracer's `wait` when it exits, by its own
id: `WIFEXITED` with its own `exit` code, or its process's status when the
process ended as a whole (Linux's `wait_task_zombie` reports the group's
code -- measured on Linux 6.6 with a racing `exit_group`). A tracer's `wait`
for a tracee that is not its child blocks rather than answering `ECHILD`.
*Divergence:* Linux hides a traced child's end from its real parent until
the tracer has reaped it; here both see it.

**Choice 11 -- system-call stops wrap every call, both ABIs.**
`PTRACE_SYSCALL` stops a thread at its next call's entry (`rax` `-ENOSYS`,
`orig_rax` the call) and exit (`rax` the result), `SIGTRAP | 0x80` under
`TRACESYSGOOD`; the tracer may change the call and its arguments, skip it
(`orig_rax` -1, the result its `rax`) or change the result; `PTRACE_SYSEMU`
never runs it; `PTRACE_GET_SYSCALL_INFO` describes the stop. While no traced
thread asked for them, a call pays one atomic load (`ptrace::syscall_entry`,
`syscall_exit`). `PTRACE_O_TRACEEXIT` stops an exiting thread -- after an
`exit_group` has ended the others, as Linux's `do_group_exit` orders it -- with
the status its message. *Divergence:* this kernel's vfork does not hold the
parent, so `PTRACE_EVENT_VFORK_DONE` follows `PTRACE_EVENT_VFORK` at once.

**Tested by** a ring-3 program (`build/ptracetest.c`,
`spawn::self_test_linux_ptrace`) whose every answer was checked against Linux
6.6.87 (WSL2), twelve runs of twelve: `TRACEME` (a second one `EPERM`), the
exec stop and its registers, `PEEKTEXT`, an `int3` written through
`/proc/<pid>/mem` into read-only text and not into the memfd it was run from,
the breakpoint's stop, two single steps and DR6's step bit, a suppressed
`SIGUSR1`, the exit; `PTRACE_EVENT_EXEC`, `GETEVENTMSG` and `PTRACE_KILL` on a
second child; on a third, the debug registers' refusals, xmm0 through
`GETFPREGS`, `NT_PRFPREG` and `NT_X86_XSTATE`, xmm1 written back, a hardware
execution breakpoint, the resume flag past it, and a write watchpoint; by a
second program (`build/ptracetier2test.c`, `spawn::self_test_linux_ptrace_tier2`,
twelve of twelve on Linux 6.6.87) -- a traced thread from its first
instruction to its exit report, fork and vfork events and a traced
grandchild, system-call stops with a changed result and a skipped call; and
by `ptrace::self_test` (the register image, its checks, the refusals),
`sched::debugreg::self_test` (Linux's checks, the classification) and
`sched::fpu`'s debugger views.

**Where it lives:** `kernel/src/proc/ptrace.rs`; the stops in
`syscall::handlers` (`traced_signal`, `exec_stop`), `idt`
(`traced_fault_stop`, `user_trap`, `handle_debug`) and `syscall::linux`'s
exec; `wait.rs`'s `ChildEvent::Traced`; `mm::cow::write_private`;
`fs::procfs`'s `mem`; `signal::classify` for a traced process's signals;
`sched::debugreg` and `sched::fpu`'s debugger views for the registers.

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
`TracerPid` and the `t (tracing stop)` state.

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
unconditionally and gives up if the call fails. `TRACEEXEC` and `EXITKILL`
are kept; the others are accepted and their stops not yet made -- a traced
program's new threads and children run untraced, and it exits without the
exit stop. That is lane D's "Tier 2" (threads, fork, the other events),
tracked in `known-issues/A-ptrace-tier-2-*.md`, with the debug registers,
the FPU registers, attaching and system-call stops. *Easy to reverse:* the
`option::MASK` check in `SETOPTIONS`.

**Tested by** a ring-3 program (`build/ptracetest.c`,
`spawn::self_test_linux_ptrace`) whose every answer was checked against Linux
6.6.87 (WSL2), twelve runs of twelve: `TRACEME` (a second one `EPERM`), the
exec stop and its registers, `PEEKTEXT`, an `int3` written through
`/proc/<pid>/mem` into read-only text and not into the memfd it was run from,
the breakpoint's stop, two single steps, a suppressed `SIGUSR1`, the exit;
`PTRACE_EVENT_EXEC`, `GETEVENTMSG` and `PTRACE_KILL` on a second child; and by
`ptrace::self_test` (the register image, its checks, the refusals).

**Where it lives:** `kernel/src/proc/ptrace.rs`; the stops in
`syscall::handlers` (`traced_signal`, `exec_stop`), `idt`
(`traced_fault_stop`, `user_trap`) and `syscall::linux`'s exec; `wait.rs`'s
`ChildEvent::Traced`; `mm::cow::write_private`; `fs::procfs`'s `mem`;
`signal::classify` for a traced process's signals.

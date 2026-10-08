### [A] ptrace "Tier 2": threads, fork, the other events, the debug and FPU registers, attaching and system-call stops are not done -- 2026-10-08

**Status:** OPEN

**What works** (design-decisions 1547): a debugger can start a
single-threaded program and debug it -- `PTRACE_TRACEME`, the exec stop,
signal and exception stops, breakpoints, single steps, the general
registers, memory (`PEEK`/`POKE`, `/proc/<pid>/mem`), `KILL`, `DETACH`,
`TRACEEXEC`, `EXITKILL` -- on both ABIs (Linux `ptrace`, native
`SYS_PTRACE` 1149). That is lane D's Tier 1
(`requests/d-a-a-debugger-needs-ptrace-for-native-programs.md`).

**What is missing,** each with where its fix goes:

- **Threads and children.** `PTRACE_O_TRACECLONE`, `TRACEFORK`,
  `TRACEVFORK`, `TRACEVFORKDONE`, `TRACEEXIT` and `TRACESYSGOOD` are
  accepted by `SETOPTIONS` (LLDB sets them unconditionally and gives up if
  the call fails) and their stops are not made: a traced program's new
  threads and children run untraced, and it exits without the exit stop.
  The fix: in `proc::thread_clone::clone_thread` and `proc::fork`, a new
  `Tracee` for the child when the creator's options ask, started in a stop
  (`SIGSTOP`, as Linux starts it), with the creator's `PTRACE_EVENT_CLONE`/
  `FORK` stop and `GETEVENTMSG` = the new id; `TRACEEXIT` from
  `thread::on_thread_exit`, before the thread is gone. `wait` then needs
  `__WALL` for non-leader threads and must report a tracee that is not the
  tracer's child (`take_stop_report` already scans by tracer; the per-pid
  wait path in `syscall::wait` still asks `pcb::peek_exit` first, which
  refuses a non-child). Thread-directed `SIGSTOP` to another process's
  thread (GDB's all-stop) needs `tgkill` to reach it.
- **Exec of a multi-threaded process.** Linux's `de_thread` ends every other
  thread before the new image runs; this kernel's exec does not, so a
  traced exec from a non-leader thread is not modelled.
- **Debug registers** (`POKEUSER` at `u_debugreg`, offset 848): `EIO`, and
  `PEEKUSER` there reads 0. GDB reads DR6 at every single-step stop, which
  works; a hardware watchpoint fails with "Couldn't write debug register"
  (`set can-use-hw-watchpoints 0` gives software ones), and there are no
  hardware breakpoints. The fix: DR0-DR3/DR7 per thread on the scheduler
  record, validated as Linux's `ptrace_set_debugreg` validates them, loaded
  at switch-in; `#DB` with a DR6 hit bit reports `TRAP_HWBKPT` with
  `si_addr`.
- **FPU and vector registers**: `PTRACE_GETFPREGS`/`SETFPREGS` and
  `GETREGSET`/`SETREGSET` (`NT_PRSTATUS`, `NT_PRFPREG`, `NT_X86_XSTATE`) are
  `EIO`. The stopped thread's FPU state is saved by the switch out; the fix
  reads and writes that image, with the `xcr0` copy at offset 464 GDB reads.
- **Attaching** (`PTRACE_ATTACH`, `SEIZE`, `INTERRUPT`, `LISTEN`): `EPERM`,
  because the right to debug a process one did not start comes from
  design-decisions 24's broker, which does not exist. The broker's design
  is lane A's (or a question for the operator: should it ask the user?).
  An attached tracee is not the tracer's child, so `wait` must then accept
  one: today only a child's stops reach it.
- **System-call stops** (`PTRACE_SYSCALL`, `SYSEMU`): `EIO`. The fix: entry
  and exit stops in `syscall::entry`, reported as `SIGTRAP | 0x80` under
  `TRACESYSGOOD`; a single step over a `syscall` instruction (Linux reports
  the step at the call's exit) belongs with it.
- **`/proc/<pid>/mem` across an exec.** The file is resolved by pid at each
  access, so a descriptor opened before the target exec'd reads the new
  image; Linux ties it to the address space and answers EOF after an exec.
  The fix: an exec generation on the process, recorded at open.
- **An untraced native program's `int3` or single step** is logged and the
  program runs on, as before: the native exception set has no breakpoint or
  trace code (`proc::exception::ExceptionCode`). A Linux program gets
  `SIGTRAP`, as on Linux.

**How to see it.** `ptrace(PTRACE_GETFPREGS, ...)` or `PTRACE_ATTACH` from
any program; a traced program that calls `pthread_create` with
`PTRACE_O_TRACECLONE` set: no `PTRACE_EVENT_CLONE` stop arrives.

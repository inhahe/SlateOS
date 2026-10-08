### [A] ptrace "Tier 2": threads, fork, the other events, attaching and system-call stops are not done -- 2026-10-08

**Status:** OPEN

**What works** (design-decisions 1547): a debugger can start a
single-threaded program and debug it -- `PTRACE_TRACEME`, the exec stop,
signal and exception stops, breakpoints, single steps, the general, FPU,
vector and debug registers (hardware breakpoints and watchpoints), memory
(`PEEK`/`POKE`, `/proc/<pid>/mem`), `KILL`, `DETACH`, `TRACEEXEC`,
`EXITKILL` -- on both ABIs (Linux `ptrace`, native `SYS_PTRACE` 1149). That
is lane D's Tier 1, and the registers of its Tier 2
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
- **Exec from a thread that is not the first.** The exec ends every other
  thread first, as Linux's `de_thread` does (since 2026-10-08), but the
  thread that exec'd keeps its own id, where Linux gives it the process's:
  `gettid` after such an exec is not `getpid`, and a traced one's
  `PTRACE_EVENT_EXEC` message, its former id, is its id still. The fix: the
  exec'ing thread takes the leader's task id (`pcb::claim_leader_id`'s id),
  which needs the scheduler to re-key a live task.
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

**How to see it.** `PTRACE_ATTACH` from any program (`EPERM`); a traced
program that calls `pthread_create` with `PTRACE_O_TRACECLONE` set: no
`PTRACE_EVENT_CLONE` stop arrives.

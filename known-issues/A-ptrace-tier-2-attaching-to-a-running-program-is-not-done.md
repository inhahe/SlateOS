### [A] ptrace "Tier 2": attaching to a running program is not done, and five smaller gaps -- 2026-10-08

**Status:** OPEN

**What works** (design-decisions 1547): a debugger can start a program and
debug it, threads and children included -- `PTRACE_TRACEME`; the exec,
signal, exception, creation (`PTRACE_EVENT_CLONE`/`FORK`/`VFORK`/
`VFORK_DONE`), exit (`PTRACE_EVENT_EXIT`) and system-call stops
(`PTRACE_SYSCALL`, `SYSEMU`, `PTRACE_GET_SYSCALL_INFO`); new threads and
children traced from their first instruction; a traced thread's end reported
to its tracer's `wait`; the general, FPU, vector and debug registers
(hardware breakpoints and watchpoints); memory (`PEEK`/`POKE`,
`/proc/<pid>/mem`); `KILL`, `DETACH`, `EXITKILL` -- on both ABIs (Linux
`ptrace`, native `SYS_PTRACE` 1149). That is lane D's Tier 1 and all of its
Tier 2 but attaching
(`requests/d-a-a-debugger-needs-ptrace-for-native-programs.md`).

**What is missing,** each with where its fix goes:

- **Attaching** (`PTRACE_ATTACH`, `SEIZE`, `INTERRUPT`, `LISTEN`): `EPERM`,
  because the right to debug a process one did not start comes from
  design-decisions 24's broker, which does not exist. Who should grant it
  -- the user at a prompt, the program in advance, a debugger over its own
  descendants, administrators -- is the operator's question A-Q25. The rest
  is in place: `wait` already waits for a tracee that is not the caller's
  child. `strace` tests `SEIZE` at its start and
  uses `TRACEME` when it fails, so only `strace -p` and `gdb -p` need it.
- **Exec from a thread that is not the first.** The exec ends every other
  thread first, as Linux's `de_thread` does, but the thread that exec'd
  keeps its own id, where Linux gives it the process's: `gettid` after such
  an exec is not `getpid`, and a traced one's `PTRACE_EVENT_EXEC` message,
  its former id, is its id still. The fix: the exec'ing thread takes the
  leader's task id (`pcb::claim_leader_id`'s id), which needs the scheduler
  to re-key a live task.
- **A traced child's end reaches its real parent at once.** Linux hides a
  traced child's zombie from its real parent until its tracer -- when that is
  not the parent -- has reaped it (`wait_consider_task`); here both are told
  at once (`ptrace::on_thread_exit`, `pcb::peek_exit*`). Only a child of a
  traced program that its tracer keeps tracing (`TRACEFORK` with the child
  not detached) is affected.
- ~~**`/proc/<pid>/mem` across an exec.**~~ Fixed on lane-a-wip
  2026-10-08, awaiting a boot: an open of it is bound to its process's exec
  generation (`Process::exec_gen`, the VFS's `open_binding` hook kept by the
  handle), so after an exec it reads end-of-file and writes fail (`EIO`;
  Linux writes nothing and answers 0), as Linux's, which holds the `mm` it
  opened, does. That also keeps a descriptor to a process's own memory,
  kept across its exec of a privileged program, from reaching the new
  image (the Mempodipper hole). Checked by `procfs::self_test`.
- **An untraced native program's `int3` or single step** is logged and the
  program runs on, as before: the native exception set has no breakpoint or
  trace code (`proc::exception::ExceptionCode`). A Linux program gets
  `SIGTRAP`, as on Linux.
- **Small divergences, recorded so they are not rediscovered:** this
  kernel's vfork does not hold the parent, so `PTRACE_EVENT_VFORK_DONE`
  follows `PTRACE_EVENT_VFORK` at once; a single step over a `syscall`
  instruction stops with `TRAP_TRACE` after the `SYSRET`, where Linux's
  `user_single_step_report` says `TRAP_BRKPT` at the call's exit (both at
  the next instruction; GDB treats both as the step); `PTRACE_GETSIGMASK`,
  `SETSIGMASK`, `PEEKSIGINFO` and `GET_RSEQ_CONFIGURATION` -- CRIU's -- are
  `EIO`.

**How to see it.** `PTRACE_ATTACH` from any program: `EPERM`.

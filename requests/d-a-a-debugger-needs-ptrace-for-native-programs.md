# Lane D -> lane A: a debugger needs ptrace for native programs

**Filed:** 2026-10-07 by lane D. **For:** lane A (`kernel/src/syscall/`,
`kernel/src/proc/`, `kernel/src/fs/procfs.rs`, `kernel/src/cap/`).
**Status:** OPEN.

**In short:** the operator wants a real debugger on SlateOS -- GDB and/or
LLDB, ported (design-decisions 1050; `roadmap.md` gives the port to lane D).
A debugger has to control another program. It stops the program,
reads and changes its memory and registers, and plants breakpoints.
It runs the program one instruction at a time and is told each time
the program stops, starts a thread, forks, runs another program or
exits. Nothing a native program can call does any of that today, so a
ported GDB would build and then fail at its first `run`. This asks lane
A for the kernel half: Linux's `ptrace` and `/proc/<pid>/mem` behaviour,
behind the DEBUG right that design-decisions 24 (an operator decision)
already says must guard it. Lane D will provide the library half and
the port itself.

## Why now

`todo.txt`'s "DEFERRED (2026-06-14): ptrace(2) behind the Process+DEBUG
capability" set its own trigger: "when a debugger/consumer needs live
control (gdb-style) over a target". The operator's request for a ported
debugger (design-decisions 1050, 2026-09-27) is that consumer. Its
companion item -- a ring-3 test of the cross-address-space
`process_vm_readv`, deferred "until the real debugger consumer lands" -- is
due as well. A debugger gives that test a live target without extra work.

## What exists

| Piece | State |
|---|---|
| `Rights::DEBUG` on a `Process` capability (`kernel/src/cap/rights.rs`) | exists; design-decisions 24 |
| `copy_from_user_as` / `copy_to_user_as` (`kernel/src/mm/user.rs`) | exist; copy to and from another address space |
| Linux ABI `process_vm_readv`/`writev` | cross-address-space copy gated on DEBUG -- Linux binaries only |
| Linux ABI `ptrace` | argument checks in Linux's order, then `EPERM` |
| native `SYS_THREAD_SUSPEND` / `SYS_THREAD_RESUME` (513/514) | the caller's own threads only |
| libc `ptrace`, `process_vm_readv`/`writev` (native) | argument checks, then `ENOSYS` -- no native call to make |
| `/proc/<pid>/` (`kernel/src/fs/procfs.rs`) | read-only: `status`, `stat`, `cmdline`, `maps`, `auxv`, `exe`, `environ`, `task`, `fd`, `caps` |

## The gate

Design-decisions 24 (operator): unilateral introspection is gated "by a
debug capability the caller holds over the specific target process,
never derived from ambient PID/uid authority", and "when [ptrace] is
built it will gate on the same `Process`+`DEBUG` capability". Every
operation below therefore takes that gate. A debugger comes to hold the
right in one of two ways, and they differ:

1. **The debugger starts the program** (`gdb ./prog`, `lldb -- prog`). Both
   fork a child, which calls `ptrace(PTRACE_TRACEME)` and then `execve`s
   the program. `TRACEME` is the *target's own consent* to be debugged by
   its parent. Lane D suggests it as the native way such a grant is
   made: the process gives its parent DEBUG over itself. No broker is
   needed, and this is how nearly all debugging starts.
2. **The debugger attaches to a running program** (`gdb -p PID`,
   `lldb -p PID`). The debugger holds nothing over a process it did not
   start. Design-decisions 24 says such authority comes "from a
   privileged debugger broker", which does not exist yet. That broker is
   lane A's design, or a question for the operator if lane A thinks it
   needs one -- for example, whether the broker should ask the user. Until
   it exists, attach answers `EPERM`, and the debugger can still start
   programs itself.

Two security rules come with the gate, both Linux's for the same reason:

- **A traced `exec` that would raise the process's authority must not
  keep both.** On Linux a setuid binary run under `ptrace` either loses
  its setuid or loses its tracer. The SlateOS equivalent: if an exec'd
  program would receive capabilities its tracer does not hold (e.g. a
  `startup.conf` grant), the trace ends or the grant is withheld --
  otherwise `TRACEME` and `exec` launder authority.
- **One tracer per thread**, and a tracer's DEBUG right is checked at
  attach *and* on each operation, so a revoked right ends access.

## What is asked

The semantics are Linux's, because the ports' back ends are GDB's and
LLDB's *Linux* back ends unchanged. The library will present these as
`ptrace(2)`, `waitpid(2)` and `/proc`. Whether the native calls are one
call shaped like `ptrace` (request, tid, addr, data) or several is lane
A's choice. A `ptrace`-shaped native call is the simplest for the
library: it is a direct pass-through.

The lists below are what the two debuggers actually call, read from
their source: LLDB 20.1.8
(`lldb/source/Plugins/Process/Linux/`, `Host/posix/ProcessLauncherPosixFork.cpp`)
and GDB 18.1 (`gdb/linux-nat.c`, `gdb/nat/linux-ptrace.c`,
`gdb/amd64-linux-nat.c`, `gdb/x86-linux-nat.c`, `gdbserver/linux-low.cc`).

### Tier 1 -- a debugger can start a single-threaded program and debug it

- **`PTRACE_TRACEME`** (above), and the stop that follows: after a traced
  `execve` succeeds, the tracee stops with `SIGTRAP` before running a
  single instruction of the new program. With `PTRACE_O_TRACEEXEC` set,
  this is a `PTRACE_EVENT_EXEC` stop instead.
- **Stops reported through `waitpid`/`waitid`**, with Linux's status words.
  `WIFSTOPPED`, `WSTOPSIG` = the signal, and `status >> 16` = the
  `PTRACE_EVENT_*` for an event stop. Every signal headed for a tracee
  stops it first ("signal-delivery-stop"). The tracer then decides
  whether the signal is delivered, through the `data` of the resuming
  call; 0 suppresses it.
- **`PTRACE_CONT`** and **`PTRACE_SINGLESTEP`**, each with the
  signal-to-deliver argument. On x86-64 a single step is the trap flag.
- **`PTRACE_GETSIGINFO` / `PTRACE_SETSIGINFO`**: the siginfo of the stop.
  Both debuggers tell a breakpoint from a step from a watchpoint by
  `si_code`. On x86-64 Linux, `int3` gives `SIGTRAP` with
  `si_code = SI_KERNEL` (0x80) and `rip` one past the `int3`. A single
  step gives `TRAP_TRACE` (2), and a debug-register hit gives
  `TRAP_HWBKPT` (4) with `si_addr`. LLDB's `NativeProcessLinux.cpp`
  switches on exactly these.
- **`PTRACE_GETREGS` / `PTRACE_SETREGS`**: `struct user_regs_struct` (27
  words, `r15` ... `gs`, including `orig_rax`, `fs_base` and `gs_base`). A
  debugger that moves `rip` while the tracee is stopped in a system call
  sets `orig_rax = -1` so the call is not restarted. Linux honours that,
  and the restart logic must too.
- **Memory, the way GDB insists on it: `/proc/<pid>/mem` and
  `/proc/<pid>/task/<tid>/mem`, readable and *writable* under DEBUG.**
  `pread`/`pwrite` at an offset equal to the address. GDB 18.1's
  `linux-nat.c` design note says it "strongly prefer[s] /proc/PID/mem",
  falls back to `PTRACE_PEEKTEXT`/`POKETEXT` only if the file is not
  writable, and that gdbserver has no fallback at all. Its reasons are
  requirements, not preferences:
  - Writes must reach **read-only pages**. A breakpoint is an `int3`
    written into code; Linux breaks copy-on-write for it, so the file
    on disk and other processes mapping it are not changed.
    `process_vm_writev` cannot do this.
  - It must work **while threads are running**, not only on a stopped
    thread.
  - The open file must be tied to the **address space**, not the pid.
    After the target `exec`s, reads give EOF and writes fail, so nothing
    lands in the new image by mistake. The `process_vm_*` calls have
    exactly this race.

  LLDB uses `PTRACE_PEEKDATA` / `PTRACE_POKEDATA` (one word at a time;
  `POKEDATA` writes code the same way) and `process_vm_readv` for bulk
  reads ("about 50 times faster", its comment says). For native callers,
  the last needs the same DEBUG-gated cross-address-space path the Linux
  ABI already has.
- **Killing the tracee**: `kill(pid, SIGKILL)` and `PTRACE_KILL` (GDB still
  sends it) end a stopped tracee too. `PTRACE_O_EXITKILL` -- kill the
  tracee if the tracer exits -- which GDB sets whenever the kernel
  supports it.
- **`/proc/<pid>/` as the debuggers read it**: `maps` in Linux's format
  (address range, permissions, offset, device, inode, path). Both
  debuggers find the executable's load address from `maps` and from
  `auxv`. `auxv` must be the tracee's actual auxiliary vector, because
  GDB relocates a position-independent program by its `AT_ENTRY` and
  `AT_PHDR`. `status` needs `State` (`t (tracing stop)`) and
  `TracerPid`, and there is also `exe`. LLDB also reads `stat`, `cmdline`,
  `environ`, `comm` and `smaps`.

### Tier 2 -- threads, every register, watchpoints, fork and exec

- **Threads.** `PTRACE_O_TRACECLONE`: a thread the tracee creates is
  traced from birth and starts stopped. The creator reports
  `PTRACE_EVENT_CLONE` with `PTRACE_GETEVENTMSG` = the new tid. Stops are
  per thread. A tracer collects the stops of threads that are not its
  children through `__WALL`, and LLDB waits with
  `waitpid(-1, __WALL | __WNOTHREAD | WNOHANG)`. One thread is stopped
  by a thread-directed `SIGSTOP`: LLDB sends `tgkill(pid, tid, SIGSTOP)`,
  GDB `tkill(tid, SIGSTOP)` (`linux-nat.c`'s `kill_lwp`). Both use it to
  stop every thread ("all-stop"), so a thread-directed signal must reach
  a traced thread of *another* process. The thread list comes from
  `/proc/<pid>/task/`.
- **Floating-point and vector registers.** `PTRACE_GETFPREGS` / `SETFPREGS`
  (the 512-byte FXSAVE image), and `PTRACE_GETREGSET` / `SETREGSET` with
  `NT_PRSTATUS`, `NT_PRFPREG` and **`NT_X86_XSTATE`** (the XSAVE image:
  AVX, AVX-512, PKRU). GDB reads which components are present from
  the `xcr0` copy Linux leaves in the software-reserved bytes at offset
  464 of the image. Both debuggers use `NT_X86_XSTATE` on every machine
  with AVX.
- **Debug registers -- hardware breakpoints and watchpoints.**
  `PTRACE_PEEKUSER` / `PTRACE_POKEUSER` at `offsetof(struct user,
  u_debugreg[i])` (offset 848 + 8*i on x86-64) for DR0-DR3, DR6 and DR7.
  Linux validates DR7: no kernel addresses, legal lengths and kinds.
  The values are per thread and switched with the thread.
- **Fork, vfork and exec.** `PTRACE_O_TRACEFORK`, `TRACEVFORK`,
  `TRACEVFORKDONE`, `TRACEEXEC`, `TRACEEXIT`, with their
  `PTRACE_EVENT_*` stops. The new child is traced and stopped, and
  `GETEVENTMSG` gives its pid. GDB's `follow-fork-mode` and
  `detach-on-fork` build on these. Both debuggers set all of them.
- **`PTRACE_DETACH`** (with a signal to deliver) and **`PTRACE_ATTACH`** /
  **`PTRACE_SEIZE`** / **`PTRACE_INTERRUPT`**: the attach path, under the
  broker above.
- **`PTRACE_ARCH_PRCTL`** (`ARCH_GET_FS` / `ARCH_GET_GS`), which GDB uses
  for the thread pointer when `fs_base` is not in the register set.

### Tier 3 -- the rest of what GDB offers

- **`PTRACE_SYSCALL`** with **`PTRACE_O_TRACESYSGOOD`** (system-call stops
  reported as `SIGTRAP | 0x80`), for GDB's `catch syscall`.
- `personality(ADDR_NO_RANDOMIZE)` honoured by `exec`. LLDB's launcher
  sets it so addresses repeat from run to run; GDB does by default
  (`set disable-randomization on`).

## What lane D does with it

- libc's `ptrace(2)` passes through to the native call(s), and
  `waitpid`/`waitid` decode the stops with `__WALL`. `<sys/ptrace.h>`,
  `<sys/user.h>` and `<sys/procfs.h>` get Linux's layouts. libc's
  `process_vm_readv`/`writev` reach the gated path.
- **The port: `scripts/gdb-spike/`**, the same "link it against our libc
  before writing a line" step that measured make, cmake and CPython. It
  starts now and is independent of this request: it measures what GDB
  18.1 lacks from the library, and gets `gdb --version` and
  `gdb -batch` on a static program (no live process) running. Live
  debugging waits on Tier 1.
- **The ring-3 test both halves want**: a C fixture that forks, calls
  `PTRACE_TRACEME`, `exec`s a known program, plants an `int3` at a known
  address through `/proc/<pid>/mem`, continues to it, and checks
  `SI_KERNEL`, `rip`, a single step's `TRAP_TRACE` and the exit
  status. That fixture is also the live-target test of cross-address-space
  memory that `todo.txt` deferred.

## If it is never done

Nothing gets worse. `ptrace` answers `ENOSYS` from the library, as
today, and a ported GDB can still examine a program without running it
(symbols, types, disassembly) and debug a remote target as a client.
But the operator's
"capable debugger, like cdb" cannot exist on SlateOS without the kernel
half, and the hand-written `userspace/gdb` crate has no process to
control either.

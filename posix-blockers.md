# Posix Zone — Blocked Work Items

The posix zone is fully audited and tested (2700 tests, all pass).
All remaining implementation work is blocked on kernel features
built by other zones. Here's exactly what's needed:

---

## 1. Signal Handling Shim (kernel-ipc zone)

**What posix needs:** Real signal delivery to userspace processes.

**Current state:** The posix layer has signal constants, sigaction
storage, sigset operations, and signal/raise/kill stubs. `raise()`
works for SIGABRT (calls abort). Everything else is a no-op or
returns ENOSYS.

**What kernel-ipc must provide:**
- A mechanism for the kernel to deliver an asynchronous notification
  to a userspace thread (analogous to Unix signal delivery: save
  registers, redirect execution to a handler, restore on return).
- A syscall for one process to send a signal to another (kill).
- Integration with the scheduler: SIGSTOP/SIGCONT, SIGCHLD on
  child exit, etc.
- Alternatively: if the design uses IPC messages instead of classic
  signals (per design.txt — "No Unix signals for process control"),
  provide the IPC-based equivalent that posix can translate to/from
  POSIX signal semantics.

**Posix roadmap entry:** `roadmap.md` line 1177, 1325.

---

## 2. epoll (kernel-ipc zone)

**What posix needs:** Kernel-side event notification for file
descriptors.

**Current state:** `epoll.rs` has `epoll_create`, `epoll_ctl`,
`epoll_wait` stubs that return ENOSYS with errno set.

**What kernel-ipc must provide:**
- A kernel object that monitors a set of file descriptors for
  readiness (readable, writable, error, hangup).
- Syscalls: create epoll instance, add/modify/remove fd watches,
  wait for events with timeout.
- Integration with pipes, sockets, and other I/O objects so they
  can wake epoll waiters.

**Alternative:** If the OS design uses io_uring or IOCP-style
completion instead of epoll, provide that and posix will translate
epoll calls into the native mechanism.

**Posix roadmap entry:** `roadmap.md` line ~1141 (implicit under
POSIX compatibility).

---

## 3. popen/pclose (kernel-process zone)

**What posix needs:** fd inheritance during process spawn.

**Current state:** `popen` stub returns NULL with ENOSYS. The
`posix_spawn_file_actions` infrastructure is built (addclose,
adddup2, addopen with 16-action storage), but the actions are
recorded and never applied because the kernel spawn syscall
doesn't support fd manipulation.

**What kernel-process must provide:**
- Extend `SYS_PROCESS_SPAWN` (or equivalent) to accept a list of
  fd operations to perform in the child before exec:
  - close(fd)
  - dup2(oldfd, newfd)
  - open(path, flags, mode) → fd
- This enables posix to create a pipe, spawn `sh -c <command>` with
  one end of the pipe as the child's stdin/stdout, and return the
  other end to the caller.

**Posix roadmap entry:** `roadmap.md` line 1273 ("actions recorded
but not yet applied — blocked on kernel fd inheritance").

---

## 4. /proc and /sys equivalents (kernel-core or new zone)

**What posix needs:** A way for userspace to query kernel state
(process info, memory stats, CPU info, etc.) through a filesystem
interface.

**Current state:** No /proc or /sys implementation exists in the
posix layer. Some information is available via sysconf() (page size,
CLK_TCK, etc.) but programs like `ps`, `top`, `free` need /proc.

**What kernel must provide:**
- Either a procfs/sysfs virtual filesystem, or
- Syscalls that expose the equivalent information (process list,
  memory usage, per-process status, etc.) that posix can surface
  through a /proc emulation layer.

**Posix roadmap entry:** `roadmap.md` line ~1141.

---

## Priority Order (suggested)

1. **popen/fd inheritance** — Relatively contained change to the
   spawn syscall. Unblocks popen, system() improvements, and
   shell pipe infrastructure.

2. **Signal handling** — Large but fundamental. Needed for bash,
   Python, and virtually every Unix program. The design decision
   about signals-vs-IPC-messages needs to be resolved first.

3. **epoll** — Needed for any event-driven server (nginx, node.js)
   and for the shell's job control. Can be deferred if io_uring
   is prioritized instead.

4. **/proc /sys** — Needed for system utilities but not for basic
   program compilation/execution. Can be deferred longest.

---

## What the posix session accomplished (for context)

- 2722 tests (all pass), up from ~2200 at start
- Fixed lgamma_r sign inversion bug
- Improved atan accuracy from 2% to 1e-15 relative error
- Exhaustive audit of all 58 source files
- Stress tests for regex, printf, fnmatch, qsort, bsearch, strstr
- Full coverage of wchar classification, FORTIFY wrappers, syslog,
  pwd/grp enumeration, clock_getres
- Cleaned up all actionable compiler warnings
- Implemented `fchdir()` via fd-path tracking (no kernel changes needed)
- Implemented full `*at()` dirfd support (`openat`, `fstatat`,
  `unlinkat`, `renameat`, `mkdirat`, `readlinkat`, `symlinkat`,
  `linkat`, `fchmodat`, `fchownat`, `faccessat`) — relative paths
  resolved against stored dirfd path

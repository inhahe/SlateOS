// Posix Zone — Blocked Work Items (historical)

This document used to enumerate the kernel features that the posix
zone was blocked on.  All four items have since been delivered.  Kept
in the tree as a record of how the zone reached its current state,
and pointers to the roadmap entries where the completed work lives.

For the *current* state of the posix layer, read the `### 2.5 POSIX
compatibility layer` section of `roadmap.md` (the per-feature `[x]`
list around lines 1183–1430).  For ongoing test/limitation tracking,
read `todo.txt`.

---

## 1. Signal Handling Shim — DONE

**Original ask:** Real signal delivery to userspace processes.

**Status:** Delivered by the kernel signal shim
(`kernel/src/proc/signal.rs` + `posix/src/signal.rs`).  The kernel
holds per-process pending / blocked / trampoline state and delivers
asynchronously at syscall-return by redirecting RIP to a userspace
trampoline (SEH-style).  Cross-process `kill()` routes through
`SYS_SIGNAL_SEND`; the target process's own disposition table
decides terminate / ignore / handle.  `sigprocmask` mirrors the
low-64 blocked mask to the kernel; `sigpending` queries the kernel.

Syscalls: `SYS_SIGNAL_REGISTER=522`, `SYS_SIGNAL_SEND=523`,
`SYS_SIGNAL_RETURN=524`, `SYS_SIGNAL_MASK=525`,
`SYS_SIGNAL_PENDING=526`.

Roadmap: `roadmap.md` lines 1225–1227.

Documented limitations (in `todo.txt` under *Judgment Calls*): only
64 signals deliverable asynchronously; the signal being delivered is
not auto-masked during its handler; `sigaltstack` is accepted but
ignored; the host test harness cannot exercise the syscall-issuing
delivery path (only the pure router / mapper logic).

---

## 2. epoll — DONE

**Original ask:** Kernel-side event notification for file descriptors.

**Status:** Delivered as a userspace implementation in
`posix/src/epoll.rs` on top of `check_readiness`, with full coverage
of `epoll_create` / `epoll_create1` / `epoll_ctl ADD/MOD/DEL` /
`epoll_wait` / `epoll_pwait` / `epoll_pwait2`.  Level-triggered,
`EPOLLONESHOT` supported, 16 instances × 128 entries, ABI-compatible
12-byte `EpollEvent`.  `HandleKind::Epoll` is integrated across
fdtable / file / fcntl / poll / spawn.

The implementation polls readiness on `epoll_wait` rather than being
event-driven from the kernel.  This matches the design tradeoff:
write the correct version first, optimise later.  A kernel-side
wakeup-on-ready epoll object is still possible (the kernel already
has the per-handle readiness queries epoll needs), but not blocking
any program.

Companions also delivered: `timerfd` (`timerfd_create` /
`timerfd_settime` / `timerfd_gettime`), `inotify` (event-driven on
the native kernel watch API `SYS_FS_WATCH_*`), `signalfd`,
`eventfd` (with `EFD_SEMAPHORE` and ref-counted dup across spawn).

Roadmap: `roadmap.md` lines 1222–1224.

---

## 3. popen / pclose / fd inheritance — DONE

**Original ask:** Extend `SYS_PROCESS_SPAWN` to accept a list of
close/dup2/open file actions to perform in the child before exec.

**Status:** Delivered.  `posix/src/spawn.rs` implements the full
`posix_spawn_file_actions` surface (`addclose`, `adddup2`,
`addopen`, `addchdir_np`, `addclosefrom_np`) with 16-action storage.
The kernel spawn syscall accepts the action list and applies it in
the child.  `popen()` (`posix/src/stdio.rs`) is a real
implementation: it creates a pipe, builds a file-actions list that
duplicates the appropriate pipe end onto stdin / stdout, calls
`posix_spawnp` with `sh -c <command>`, and returns the other end
wrapped in a `FILE*`.  `pclose()` waits on the recorded child pid
and returns its exit status.

Roadmap: `roadmap.md` line 1201 (posix_spawn / posix_spawnp) and
line 1202 (execvp via SYS_PROCESS_EXEC).

---

## 4. /proc and /sys — DONE

**Original ask:** A way for userspace to query kernel state through
a filesystem interface.

**Status:** Delivered.  `procfs` mounted at `/proc`: 10K+ lines,
70+ root files (`version`, `uptime`, `meminfo`, `cpuinfo`, `stat`,
`vmstat`, `buddyinfo`, `net/*`, etc.) plus per-PID directories
(`status`, `cmdline`, `stat`, `maps`, `caps`).  `sysfs` mounted at
`/sys`: kernel info, hostname (r/w), sysctl params (r/w), PCI
devices, fs cache stats.

Roadmap: `roadmap.md` lines 1432–1433.

---

## Current State of the Posix Zone

- ~20,000 host tests pass (`cargo test -p posix --target x86_64-pc-windows-gnu --lib`)
- Bare-metal staticlib build (`x86_64-unknown-none`) clean
- All four 2025-era kernel blockers cleared during 2026-Q1/Q2 work
- Open known limitations / judgment calls are tracked in `todo.txt`:
  - Host-side raw `syscall` instruction — gating attempted, reverted,
    deferred with documented path forward (todo.txt L4628+).
  - `socketpair()` SOCK_DGRAM / SOCK_SEQPACKET / SCM_RIGHTS — easy
    to add when first user appears (todo.txt L4723+).
  - Signal-handling residuals: 64-signal cap, no sigaltstack, no host
    end-to-end coverage (todo.txt L4750+).
  - `eventfd` / `inotify` inheritance and event-mask completeness
    notes (resolved entries; tracked for context).
  - `getdents64` snapshot cache and several scoped flaky tests
    (resolved by per-test mutexes in 2026-06-03 / 2026-06-04 work).

No item in `posix-blockers.md`'s original list is still blocking
forward progress.  This file is retained as historical context; the
current source of truth is `roadmap.md` (state) and `todo.txt`
(limitations and follow-ups).

## 12. Toolchain on Slate OS — run prebuilt Linux binaries on the compat layer (Path Z), native-first kept inviolate

**Date:** 2026-06-13

**Decided by:** Operator (this was `open-questions.md` Q4; Claude initially framed
it as "clang-bootstrap vs build-a-gcc-cross-compiler," the operator's feedback
redirected it to the real fork — *how the toolchain runs on the OS* — and the
operator chose Z: run prebuilt Linux toolchain binaries on the Linux-ABI layer
now, native-port selectively later. The operator also green-lit installing
whatever tooling is needed.)

**Context:**
§9 set the toolchain (gcc/cmake/make/pkg-config, then CPython→fastpy) as the next
initiative. The first real fork is *how those programs run on the OS*:
- **Path X** — run **prebuilt Linux** gcc/make/cmake on the kernel's Linux-ABI
  layer (the Linux ELF loader + `ld.so` loading + Linux syscall table that
  already exist); drop a distro's binaries + glibc/`ld.so` on the image and
  harden the compat layer until they run. Matches the roadmap's "gcc … *(via
  POSIX layer)*" wording. No host C cross-compiler needed; least work to a usable
  toolchain; directly hardens the Linux-ABI layer (reused by every future Linux
  app); **this is the Linux compatibility we need anyway.**
- **Path Y** — native SlateOS/Slate port: gcc cross-compiler targeting
  `x86_64-slateos` + a native C library. Purity path (native syscalls, capability
  security) but enormous per-program effort for gcc/CPython, and it does **not**
  advance Linux compat.
- **Path Z** — hybrid: X now, Y selectively later for components where
  capability-native behavior matters.

**Decision — Path Z, starting with X.** Run prebuilt Linux toolchain binaries on
the Linux-ABI layer to get a working dev environment and harden Linux
compatibility; native-port only where it specifically pays off, later. Install
clang (and any other needed tooling) — clang targets both `x86_64-linux-*` (to
compile real Linux C programs that stress/harden the compat layer) and
`x86_64-slateos` (native C, if/when Y components are pursued).

**Bounding principle (operator-reaffirmed) — native-first is inviolate; the
compat layer must not leak into the native architecture.** Slate OS is a
**native-first OS with a deliberately-scoped Linux compat bridge**, not a
"Linux-compatibility OS." This decision does **not** soften the §4 rule. In
particular:
- We do **not** shape native primitives (IPC, scheduling, startup, the absence
  of signals) around Linux to make translation cheaper. Native primitives are
  designed on their own merits (channel+capability IPC; priority round-robin
  per-CPU scheduling; `SYS_PROCESS_GET_ARGS` startup with no SysV stack; no Unix
  signals — process control is IPC, hardware faults are SEH-style exceptions).
- The compat layer is **fast as a downstream consequence** of a well-designed
  native kernel, never as a design goal that bends native primitives.
- The one choice that helps compat — **ext4** — was made on native merits
  (`design.txt`: "ext4 first, don't write a custom filesystem"); its Linux-native
  semantics are a coincidental benefit and are *why* we avoid WSL1's
  NTFS-semantics performance disaster without any leak.
- Test for any new construct: *would it exist if Linux had never existed?* If no,
  it stays in the compat layer (like signals — see `kernel/src/proc/signal.rs` —
  and the auxv, §4) and never touches native launch/syscalls/startup.

**Honest scope / known walls (not promising 100% Linux parity):**
- The toolchain (CLI: gcc/make/cmake/bash/python/git) sits in the zone
  syscall-translation layers handle well (cf. FreeBSD Linuxulator, illumos LX
  zones). This is low-risk.
- Genuinely hard / may-never-fully-support categories: GPU-accelerated GUI
  (solved later at the **Wayland-protocol + Mesa-port** boundary, *not* by
  matching Linux's kernel graphics uAPI), containers (cgroups/namespaces/
  overlayfs/netlink/seccomp), systemd, exotic networking (AF_PACKET/netlink/eBPF),
  ptrace fidelity, io_uring corners, FUSE.
- Escape hatch for the hard cases: a WSL2-style **real Linux kernel in a VM**
  remains available later — but it needs a hypervisor (KVM-equivalent) Slate
  doesn't have yet, so it's a separate large project. Compat-layer and VM are not
  mutually exclusive (Windows ships both).

**Why this is defensible despite Microsoft's WSL1→WSL2 pivot:** WSL1 translated
to a hostile pre-existing kernel (NT/NTFS) and chased seamless full-Linux parity;
its perf death was the fs-semantics mismatch. Slate is co-designed, uses ext4,
and **bounds the promise** (native-first; compat covers a chosen software set).
The completeness treadmill only bites a project that promises 100% — we don't.

**Alternatives considered:**
- **Path Y (native port first)** — rejected for now: doesn't advance Linux
  compat, enormous for gcc/CPython, and CLAUDE.md already prefers **fastpy** for
  native OS userspace, shrinking the need to native-port big C apps.
- **Pure clang-bootstrap of a native C runtime first** (Claude's original Q4
  "A/C") — rejected: it's native-ABI work mislabeled as the fast path; it does
  not deliver the Linux compatibility the operator wants, and the toolchain
  goal is better served by running real Linux binaries.

**Where it lives:**
- Path X work: `kernel/src/syscall/linux.rs`, the Linux ELF loader, procfs/sysfs
  (Linux-ABI hardening); disk-image work to stage a glibc/`ld.so` + prebuilt
  toolchain runtime; clang (host) for compiling Linux test programs.
- roadmap.md task 5031 (gcc/cmake/make/pkg-config "via POSIX layer").
- Native-first/no-leak constraint: design-decisions.md §4, `design.txt`,
  `kernel/src/proc/signal.rs` module doc.

**How to reverse:**
- If running prebuilt Linux gcc proves to hit an unfixable Linux-ABI wall,
  fall back to Path Y for that component (native cross-build), or stage the
  WSL2-style Linux-VM escape hatch (after building a hypervisor). The
  native-first/no-leak principle is **not** reversible — it is settled policy.

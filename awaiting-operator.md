# Awaiting Operator — Participation / Go-Ahead Queue

Things that are **gated on the operator's input, participation, or explicit
go-ahead** but are **not design decisions**. Unlike `open-questions.md`, none of
these ask you to *choose* between options — they're items where I either want
your green light before committing significant effort, or where I literally
cannot proceed without you doing something (running hardware, providing an
asset, granting access, etc.).

This list does **not** necessarily block other work: when something here is
parked, I keep working on unblocked roadmap tasks. It exists so that when you
*do* have attention to spare, you can see what's waiting on you at a glance.

## How this differs from the other queues

| File | Holds |
|------|-------|
| `open-questions.md` | Design **decisions** needing your choice (architectural forks, user-visible policy, tradeoffs with no obvious answer). |
| **`awaiting-operator.md`** (this file) | Non-decision items needing your **go-ahead or participation** (green-light a big initiative, run something on real hardware, supply an asset/credential). |
| `todo.txt` | Deferred work **I can pick up myself** — no operator needed. |
| `known-issues.md` | Bugs and accumulated tech debt. |

When an item here is resolved (you give the go-ahead, or do the thing), I remove
it and either start the work or note where it went.

---

## Open items

### 1. Green-light for large multi-day "port" initiatives (e.g. Chromium)
- **What:** Before I sink days/weeks into a large external port (Chromium, a
  full Mesa GL stack, a JS engine, a large language runtime, etc.), I'd like
  your explicit go-ahead, since these reshape the roadmap and are expensive to
  unwind.
- **Why it needs you:** Not a design decision — more a prioritization/commitment
  call. These crowd out other roadmap work for a long stretch, so it should be a
  deliberate choice by you, not something I drift into autonomously.
- **Status / my read:** **Premature regardless of go-ahead.** Chromium
  specifically is blocked on hard prerequisites we don't have yet: a working
  graphics stack (compositor + GPU or at least a presented framebuffer), font
  rendering, a large slice of the Linux/POSIX ABI, networking with TLS, and a
  self-hosting or cross toolchain capable of building it. We're currently at the
  Linux-ABI-syscall + shell layer. So this is a "much later" item even with your
  blessing — I'm not parked waiting on you for it *right now*; I'm noting it so
  the policy ("ask before starting a giant port") is written down.
- **What unblocks it:** (a) the graphics/toolchain prerequisites land, **and**
  (b) you say "go." Until both, I won't start.

### 2. Real-hardware boot / driver validation
- **What:** Everything is currently validated in QEMU via `scripts/boot-test.sh`.
  Driver work (USB, real GPU, NIC, NVMe/AHCI on physical disks) eventually needs
  validation on actual hardware, which I can't do.
- **Why it needs you:** Requires you to flash an image to a USB stick / disk and
  boot a physical machine, then report what you see on serial/screen.
- **Status / my read:** Not blocking now — QEMU covers the current work. Becomes
  relevant once we have drivers whose behavior diverges between QEMU and real
  silicon. I'll flag specific drivers here when they reach that point.
- **What unblocks it:** You boot a build on real hardware and relay the serial
  log / observed behavior when I ask.

### 3. On-disk glibc/musl toolchain artifacts to E2E-test the Linux-ABI loader
- **What:** A real prebuilt Linux x86-64 dynamic executable + its interpreter on
  the OS image — minimally a `ld-linux-x86-64.so.2` (or `ld-musl-x86_64.so.1`)
  plus a small dynamically-linked binary (e.g. a `busybox`/`coreutils` build, or
  even a hand-built `hello` linked against glibc/musl), ideally a small sysroot
  (`/lib`, `/lib64`, `/usr/lib`) so `ld.so` can resolve `DT_NEEDED` libraries.
  This is the chosen path per **Q4 → Path Z** (run prebuilt Linux toolchain
  binaries on the Linux-ABI layer) and the operator-confirmed priority order
  (**Q3 / §9**: terminal/dev + GCC/Make toolchain first, GUI later).
- **Why it needs you:** It's a missing **prerequisite asset**, not a design
  decision or a coding gap. The kernel-side loader path is already built and
  boot-tested *in isolation*: SysV initial-process stack (argc/argv/envp/auxv),
  `PT_INTERP` extraction + `ld.so` load at an ASLR'd `AT_BASE`, PIE main-image
  ASLR, file-backed `MAP_PRIVATE` mmap (incl. `MAP_FIXED` segment overlays),
  real `brk`/`sbrk` heap, `madvise(MADV_DONTNEED)` reclaim, and per-thread
  `%fs`/`%gs` TLS preserved across context switches (F13/F14, just landed). What
  I *can't* do autonomously is manufacture a real glibc/musl image to run — and
  every remaining high-value step on the prioritized path (executing a real
  `ld.so`, then per-syscall behavioural fidelity tuning driven by an actual
  binary's failures) needs one to test against. The roadmap explicitly notes
  "end-to-end interpreter EXECUTION deferred until a real glibc/musl ld.so is on
  disk."
- **Status / my read:** **This is the current gate on the operator-prioritized
  toolchain path.** Spec-only fidelity wins that *don't* need a binary
  (arch_prctl/unshare/setns/seccomp/brk/clock validation, etc.) have already
  been swept by prior sessions; the remaining identified ABI gaps are either
  operator-resolved-to-keep (NUMA mempolicy, TD5/TD7) or deliberately deferred
  with rationale (hard links TD24, mmap phase-2 TD22, DRM/ALSA TD10–13). So
  without a real binary to run, further toolchain progress is genuinely waiting
  on this asset rather than on more blind coding.
- **What unblocks it:** You drop a prebuilt `ld.so` + a small dynamically-linked
  binary (and ideally its shared libs) onto the OS image / a path the VFS can
  read, and point me at it. I'll wire it into a boot/E2E test, run it, and use
  the first real failures to drive concrete syscall-fidelity fixes. Until then I
  won't fabricate a fake toolchain or guess at divergences.

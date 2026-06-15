# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This file is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.

Format for each entry:

- **Question** — the decision to be made.
- **Options** — each with its pros and cons.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

---

## Q7. How to prevent kernel-task-stack overflow when an interrupt fires on a near-full stack? (B-DF1) — OPEN 2026-06-14

**Question.** Hardware IRQs currently run on the *current* kernel task stack
(IDT IST index 0 for all of vectors 32–56; only #DF uses an IST). In-kernel
heavy code (gzip, `format!`-heavy JSON, crypto) on a 64 KiB kernel task stack
can come close to `stack_bottom`; the next timer/mouse IRQ then pushes its frame
into the guard page → double fault. The 16 KiB gzip stack array was already
fixed (B-DF1, committed), but the underlying "IRQ frame overflows a near-full
kernel stack" problem is systemic. How should we fix it?

**Options.**

- **A. Dedicated per-CPU IRQ stack (x86 IST).** Route IRQ vectors to a separate
  per-CPU stack so handlers never consume the interrupted task's stack (Linux's
  IRQ-stack model).
  - *Pros:* textbook-correct; bounds interrupt stack use independently of task
    stack depth; fixes the whole class of bug, not one benchmark.
  - *Cons:* the timer handler re-enables interrupts mid-handler (`apic.rs:1162`,
    `sti` after EOI, for preemption), so IRQs nest — a single shared IRQ IST
    would be clobbered by a nested IRQ resetting RSP to the IST top. A correct
    implementation must support nesting (manual stack-switch in the entry stub
    with a per-CPU "depth" check, switching only on the outermost IRQ) or move
    the `sti`/preemption work off the IST. Careful change to the hottest,
    most safety-critical path.
- **B. Increase kernel task stack size (e.g. 64 KiB → 128 KiB).**
  - *Pros:* trivial, low-risk; immediate headroom for debug-built heavy code.
  - *Cons:* band-aid — an interrupt can still overflow a sufficiently deep
    stack; costs +64 KiB committed per kernel task; masks rather than fixes.
- **C. Keep heavy code out of the kernel (microkernel-pure).** Move gzip /
  dashboard / crypto to userspace services with demand-grown stacks.
  - *Pros:* aligns with the microkernel design; userspace stacks grow on demand.
  - *Cons:* large effort; these modules are currently in-kernel and benchmarked
    in-kernel; doesn't help other legitimately-deep in-kernel paths.
- **D. Release-build the kernel for boot tests.** Debug builds inflate `core::fmt`
  stack use enormously; release frames are far smaller.
  - *Pros:* may sidestep the overflow without code changes.
  - *Cons:* doesn't fix the real bug (an interrupt can still overflow a near-full
    stack); diverges test build from the debug workflow.

**Claude's recommendation.** **A (per-CPU IRQ IST stack with nesting support)**
is the proper, production-grade fix and should be done eventually — but it is a
careful change to the hottest path and deserves explicit go-ahead given the
project's "scheduler/interrupt is the most safety-critical path, validate
carefully" stance. As a *cheap immediate mitigation that is independently
reasonable*, **B** (bump to 128 KiB) would unblock `BENCH_OK` now; I have **not**
applied it autonomously because per-task memory cost and "is 64 KiB the right
kernel-stack size?" is itself a tradeoff the operator may want to own. In the
meantime the bug is contained: it only triggers in the post-`BOOT_OK` benchmark
suite and does **not** affect normal operation (default boot test passes).

**Where it bites.** `kernel/src/idt.rs` (IDT IST assignment for vectors 32–56;
`init()` ~line 1762), `kernel/src/apic.rs:1162` (timer `sti`), TSS/IST setup
(per-CPU IST stacks), `kernel/src/sched/task.rs` `TASK_STACK_SIZE` /
`kernel/src/mm/kstack.rs` `STACK_SIZE` (option B). Symptom in
`kernel/src/bench.rs` deferred suite (`dashboard_api_status` onward).

**Status.** OPEN.

---

## Q8. Which libc + rootfs strategy unblocks Path Z dynamic execution of prebuilt Linux binaries? — OPEN 2026-06-14

**Question.** The operator-prioritized direction is **Path Z** (Q4, resolved
2026-06-13): run *prebuilt* Linux toolchain binaries on the Linux-ABI layer now.
The Linux-ABI loader/syscall plumbing for this is now extensively built and
proven for **static** binaries end-to-end (`proc::spawn::self_test_linux_file_mmap`
and `self_test_linux_brk` spawn real ring-3 Linux-ABI processes that issue
`open`/`mmap`/`brk` and exit with a verified code; file-backed `mmap`, real
`brk`/`sbrk`, `madvise(DONTNEED)`, SysV initial stack + auxv, PIE/interp ASLR, and
the `ld.so` *load* path are all implemented). The one documented blocker for
**dynamic** execution (roadmap.md line 5089) is: *"end-to-end interpreter
EXECUTION deferred until a real glibc/musl `ld.so` is on disk."* There is no real
Linux rootfs today — `scripts/create-disk.py` only builds a minimal FAT image
with test files for the FAT driver self-test. To run an actual dynamically-linked
prebuilt Linux binary we need a real libc + dynamic linker on a real on-disk
filesystem. **Which libc, and how do we build/populate the rootfs?**

**Options.**

- **A. musl (static-first, then dynamic).** Bootstrap with musl: tiny, permissive
  (MIT), trivial to build fully-static, and its `ld-musl-x86_64.so.1` is a single
  self-contained file.
  - *Pros:* fastest path to a *real compiled* Linux binary running (static musl
    "hello world" needs no rootfs libc at all — just the ELF on disk); minimal
    ABI surface; easy to vendor/build on the dev box; great for proving the
    loader against real (non-hand-assembled) binaries.
  - *Cons:* most *prebuilt* Linux toolchain binaries (the actual Path-Z target —
    distro GCC/binutils/CMake) are linked against **glibc**, not musl, so musl
    proves the loader but does not by itself run the prioritized prebuilt
    toolchain; musl's syscall/behaviour assumptions differ from glibc in places.
- **B. glibc directly.** Target glibc from the start, since the prioritized
  prebuilt toolchain (Q3: GCC/CMake/Make first) is glibc-linked.
  - *Pros:* matches the actual binaries we want to run; no second migration; the
    `ld-linux-x86-64.so.2` + `libc.so.6` + friends are exactly what a distro
    `gcc` needs at runtime.
  - *Cons:* glibc is large and exercises far more of the Linux ABI (TLS edge
    cases, `__libc_start_main`, `vDSO`, NSS, locale, many more syscalls/`ioctl`s)
    — a much bigger first-light bring-up than musl; harder to build/obtain on a
    Windows dev host; more ABI gaps to chase before *anything* runs.
- **C. Both, staged.** musl static now to prove the loader against real compiled
  binaries, then glibc for the prebuilt toolchain.
  - *Pros:* de-risks the loader with the cheap target first; clear milestones.
  - *Cons:* two bring-ups; some throwaway musl-specific work.

  Orthogonally, the **rootfs** question: the design says *ext4 first*, so the
  real answer is an ext4 image populated with the libc tree (vs. the current
  FAT-only test image). Building/populating an ext4 rootfs on a Windows dev host
  (and how to source the libc files — vendor prebuilt vs. build-from-source) is
  itself a setup decision bundled into this.

**Claude's recommendation.** **C (musl static first, then glibc), on an ext4
rootfs.** A static musl binary needs no on-disk libc and exercises the
already-built static-load path with a *real* compiler-produced ELF (the current
end-to-end tests use hand-assembled ELFs), so it's the cheapest way to flush out
real-world loader/ABI bugs before taking on glibc's much larger surface for the
actual prebuilt toolchain. I have **not** started this autonomously because (1)
the libc choice steers a large amount of subsequent ABI-compat work and is costly
to reverse, (2) it's the operator's prioritized initiative and they may have a
preference (e.g. "go straight to glibc, I don't care about musl"), and (3)
building/sourcing a libc + ext4 rootfs on the Windows dev box has setup forks
worth a quick operator steer. In the meantime the static-binary path is already
proven and the loader plumbing is complete, so no autonomous progress is lost by
waiting.

**Where it bites.** `scripts/create-disk.py` (rootfs build — currently FAT test
image only), `kernel/src/proc/spawn.rs` (`load_interpreter`, the `ld.so` entry
path), `kernel/src/elf.rs` (`interp_path`/`load_segments_with_bias`),
`kernel/src/syscall/linux.rs` (further ABI gaps glibc will exercise), and the
ext4 mount/root path. Roadmap.md line 5089 ("end-to-end interpreter EXECUTION
deferred until a real glibc/musl ld.so is on disk").

**Status.** OPEN.

---

No further open questions remain. All earlier deferred operator decisions
(Q1–Q6) have been resolved — see the "Recently resolved" list below and
`design-decisions.md` for full rationale. New decisions that genuinely need the
operator should be appended above this line as `## Q9 …`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13,
  **operator-confirmed 2026-06-14**; keep the UMA no-op returning 0, option A
  (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13, **operator-confirmed 2026-06-14** (keep the shipped defaults:
  native strict/committed, Linux lazy/overcommit; both configurable); build the
  both-strategies model (Option 5); map the system-wide overcommit knob to a
  fine-grained native cap (`admin.memory_policy`), not `CAP_SYS_ADMIN` (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).
- Q5 file-backed `mmap` — how far to take the fix — resolved 2026-06-14
  (§22), then **REOPENED 2026-06-14** by the operator, then **RE-RESOLVED
  2026-06-14**: adopt **C-lite** (a unified *read-only* page cache for
  shared-library text dedup + de-double-caching), deferred until a concrete
  consumer appears (the dynamic linker is the likely first; stable VFS
  file-identity is the precursor); writable `MAP_SHARED` writeback stays declined
  / `ENOSYS` (§23). Deferral trigger logged in `todo.txt`.
- Q6 cross-process memory introspection — resolved 2026-06-14: keep
  channel/shared-memory IPC for *consensual* sharing; add a
  **debug-capability-gated** cross-address-space `process_vm_readv`/`writev`
  (`Rights::DEBUG` on a `Process` capability; `EPERM` without it). `ptrace`
  remains a deferred follow-up behind the same gate (§24).

---

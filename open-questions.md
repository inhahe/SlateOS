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

No further open questions remain. All earlier deferred operator decisions
(Q1–Q6) have been resolved — see the "Recently resolved" list below and
`design-decisions.md` for full rationale. New decisions that genuinely need the
operator should be appended above this line as `## Q8 …`.

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

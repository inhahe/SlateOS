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

## Q13 — De-double-cache the read-only page cache against the block buffer cache (§36 sub-task 4 tail)

**Status:** OPEN

**Background.** The §36 C-lite read-only page cache is functionally complete:
file identity, the `mm::page_cache` store (refcount model §37), fault-path
integration (shared RO frames + CoW on private write), VFS coherence
invalidation, and a memory-pressure shrinker — all boot-verified (two clean
BOOT_OK boots; the shrinker fired under real critical pressure, freeing 54 idle
frames with no fault). The one remaining §36 item is **performance, not
correctness**: a file's data can currently live in memory *twice*.

**Question.** How should file *data* I/O be cached so a page lives in exactly
one place? Today `mm::page_cache::get_or_fill` fills a 16 KiB page via
`fs::handle::read_at` → VFS → (for ext4/FAT) the **block buffer cache**
(`fs/cache.rs`, 512 B sectors). So an mmap'd file page is cached as 32 sectors
in the buffer cache *and* as one 16 KiB page in the page cache.

**Options.**
- **(A) Page-cache-primary (Linux-like).** Make the page cache the single cache
  for regular-file data; the buffer cache caches only filesystem *metadata*
  (superblock, bitmaps, inode tables, directory blocks). File `read`/`write`
  and mmap all go through the page cache. *Pro:* the canonical, proven design;
  truly one copy; unifies `read(2)` and mmap coherence for free. *Con:* large
  refactor of the ext4/FAT data read/write paths; must route metadata vs. data
  correctly per filesystem; biggest blast radius.
- **(B) Read-through + drop-behind.** Keep the buffer cache as the device cache,
  but have the page-cache fill path mark the sectors it consumed as
  immediately-evictable (or bypass the buffer cache for whole-page file reads),
  so the data isn't pinned in both. *Pro:* small, localized; no FS-path
  refactor. *Con:* doesn't truly unify — a concurrent `read(2)` still
  re-populates the buffer cache; coherence between `read(2)` and mmap still
  relies on the §36 invalidation hooks, not a shared frame.
- **(C) Leave as-is (status quo).** Accept the double-caching; the page cache is
  small relative to the buffer cache and the win is bounded. *Pro:* zero risk,
  ships now. *Con:* memory wasted on hot mmap'd files; not the §36 end-state.

**Claude's recommendation.** (A) is the correct long-term end-state, but it is a
real FS-data-path refactor with genuine tradeoffs, so it deserves an operator
call before I commit to it (it changes how every filesystem reads file data). If
the operator wants an incremental win first, (B) is a safe stepping stone that
(A) can later subsume. Meanwhile I am treating §36 as *delivered* (correctness +
eviction) and moving on to the next unblocked roadmap task; this optimization is
not gating anything.

**Where it bites.** `kernel/src/mm/page_cache.rs` (`get_or_fill` fill path),
`kernel/src/fs/cache.rs` (buffer cache), `kernel/src/fs/handle.rs` /
`kernel/src/fs/vfs.rs` (`read_at` routing), and the ext4/FAT data read/write
paths under `kernel/src/fs/` and `fs/`.

---

## Q14 — Two disconnected cgroup subsystems: which is authoritative, and how do they connect?

**Status:** OPEN

**Background.** While fixing B-CGROUP-DBLCHARGE I found the OS has **two
independent cgroup implementations that do not talk to each other**:

1. **`kernel/src/cgroup.rs`** — the in-kernel resource controller. It has the
   real *enforcement* hooks: the physical frame allocator charges a task's
   cgroup on every `alloc_frame`/`alloc_frame_zeroed` (recording the owner in
   the per-frame `FRAME_CGROUP` array, uncharging on `free_frame`), plus
   `io_charge` and PID accounting. It reads the *current task's* group via
   `sched::current_task_cgroup()` → `Task::cgroup_id`.
2. **`fs::cgroupfs`** (`kernel/src/fs/cgroupfs.rs`) — the user-facing cgroup-v2
   filesystem (5 controllers, hierarchical groups, `memory.max`, PID limits,
   per-group process assignment, `cgrp` kshell command, `/proc/cgroupfs`,
   8 self-tests). Marked `[x]` in `roadmap.md` (line 949).

The disconnect: `fs::cgroupfs` has **no** reference to `mem_charge`,
`alloc_frame`, `Task::cgroup_id`, or `sched::*cgroup*` — it is a configuration/
accounting surface with **no enforcement**. Conversely `kernel/src/cgroup.rs`
**enforces** but has **no task-assignment path** — every `Task` is constructed
with `cgroup_id = ROOT_CGROUP` and (after I removed a speculative setter)
nothing ever changes it, and `fork`/`thread_clone`/`spawn` don't inherit it. Net
effect: **neither system actually constrains a real process's memory.**
`cgroupfs` limits are cosmetic; the kernel controller is dormant
(D-CGROUP-TASK-UNASSIGNED in `known-issues.md`).

**Question.** What is the intended architecture, and which layer owns
process→cgroup assignment and limit enforcement?

**Options.**
- **(A) `cgroupfs` as the frontend, `kernel/src/cgroup.rs` as the engine.** Wire
  `cgroupfs` writes through to the kernel controller: `memory.max` →
  `cgroup::set_mem_limit`, `cgroup.procs` assignment → set the task's
  `cgroup_id`; and have `fork`/`clone`/`spawn` inherit the parent's `cgroup_id`.
  *Pro:* one enforcement engine (the proven frame-allocator charging), a
  standard cgroup-v2 UX on top; both subsystems keep their current roles. *Con:*
  must reconcile the two group-ID spaces (cgroupfs groups vs. `cgroup.rs`
  `CgroupId`, capped at 256) and map all 5 controllers; moderate integration.
- **(B) Collapse onto one.** Delete/absorb one implementation. Either make
  `cgroupfs` a thin VFS view over `kernel/src/cgroup.rs` state (drop cgroupfs's
  parallel bookkeeping), or move enforcement into cgroupfs and retire
  `kernel/src/cgroup.rs`. *Pro:* eliminates the duplication entirely; one source
  of truth. *Con:* biggest blast radius; risks regressing whichever subsystem's
  self-tests; `kernel/src/cgroup.rs` is on the allocator hot path so its data
  layout (per-frame `u8` owner array) must be preserved regardless.
- **(C) Containers drive `kernel/src/cgroup.rs` directly, leave `cgroupfs`
  standalone.** Wire `container.rs` (which already creates a `cgroup.rs` group
  per container in `Container::cgroup_id`) to assign its tasks + inherit on
  fork; treat `cgroupfs` as an independent, separately-scoped feature. *Pro:*
  smallest change to make real memory limits work (containers are the concrete
  consumer). *Con:* leaves the two cgroup systems permanently parallel — two
  ways to express "a cgroup," confusing long-term.

**Claude's recommendation.** (A). The frame-allocator charging in
`kernel/src/cgroup.rs` is the correct, hot-path-proven enforcement engine, and
cgroup-v2 (`cgroupfs`) is the right user-facing model — they should be two ends
of *one* pipe, not two pipes. The only autonomous, clearly-correct increment I'd
make without an operator call is having `fork`/`clone`/`spawn` **inherit** the
parent's `cgroup_id` (universal cgroup semantics; inert while all tasks are root,
harmless otherwise) — but even that is pointless until an assignment path exists,
which is the design fork above. I have **not** implemented any of this; I logged
the gap and moved to the next unblocked roadmap task.

**Where it bites.** `kernel/src/cgroup.rs` (`set_mem_limit`, `mem_charge`,
`current_task_cgroup`), `kernel/src/fs/cgroupfs.rs` (controller writes, process
assignment), `kernel/src/sched/task.rs` (`cgroup_id` field + 3 constructors all
defaulting to `ROOT_CGROUP`), `kernel/src/sched/mod.rs` (would need a
lock-taking `set_task_cgroup` setter), `kernel/src/container.rs`
(`Container::cgroup_id`), and the task-creation paths in
`kernel/src/proc/{fork,thread,thread_clone,spawn}.rs` (cgroup inheritance).

---

## Q15 — Which large initiative comes next? (roadmap is otherwise complete)

**Status:** OPEN

**Background.** The operator's last directive (Q12 — the C-lite read-only page
cache) is **delivered**, and a sweep of `roadmap.md` shows every tracked item is
`[x]`/`[-]`-complete *except four unchecked entries*, all of them large:

1. **TCP/IP stack → userspace service** (`roadmap.md` line 1125). The full
   network stack (IPv4/IPv6, TCP/UDP/DNS/DHCP/DHCPv6/SLAAC/MLD/firewall, ~80
   self-tests) is feature-complete but **kernel-resident**; the design calls for
   moving it into a userspace daemon.
2. **GPU acceleration** (line 4601) — currently a software rasterizer. Also the
   long-term home for hardware video encode (per resolved **Q10**).
3. **H.264/VP9 fullscreen-capture encoder** (line 4605) — **operator-deferred by
   Q10** (do hardware-encode via the GPU driver long-term; no software codec /
   stub meanwhile). *Not a candidate until (2) lands.*
4. **Port Docker / a container runtime** (line 5293) — a giant *external* port;
   per the standing rule it needs explicit operator go-ahead on prerequisites.

Separately, two design questions are already **OPEN and gating** bounded follow-up
work: **Q13** (de-double-cache the page cache vs. buffer cache) and **Q14** (wire
the two disconnected cgroup subsystems together). Both are smaller than the four
initiatives above but need an operator call before I implement them.

**Question.** What should the next focus be?

**Options.**
- **(A) Answer Q13 + Q14 first, then I execute them.** *Pro:* bounded, retires
  real tech-debt (memory wasted on double-cached file data; cgroup limits that
  currently enforce nothing), no multi-day architectural commitment; I can finish
  both quickly once the direction is chosen. *Con:* incremental, not a headline
  feature.
- **(B) TCP/IP stack → userspace.** *Pro:* directly advances the core microkernel
  principle (services in userspace), fully internal (no external deps), the stack
  is already feature-complete so it's a *migration* not new functionality. *Con:*
  real architectural fork — IPC vs. syscall socket ABI, whether the NIC driver
  moves too, performance vs. the current in-kernel fast paths; costly to reverse.
- **(C) GPU acceleration.** *Pro:* unblocks both faster compositing and the
  deferred hardware video encoder (Q10). *Con:* the largest/longest effort; needs
  real GPU-driver work (command submission, memory management) that's a project
  in itself.
- **(D) Docker / container runtime port.** *Pro:* big capability headline. *Con:*
  giant external port; explicitly gated on operator go-ahead and on prerequisites
  (the cgroup enforcement gap from Q14 is one of them).

**Claude's recommendation.** **(A)** as the immediate next step — answering Q13
and Q14 lets me deliver bounded, debt-reducing work right away without gambling
days on an architectural fork. For the next *large* initiative after that, **(B)
TCP/IP → userspace** is the most design-coherent: it's internal, the stack is
already complete (lowering risk), and it's squarely on the microkernel roadmap.
(C) and (D) are bigger and have harder prerequisites. Meanwhile I have **stopped
the autonomous loop** — the safe, bounded, host-testable work in reach this
session (pthread_atfork → fork(), daemon() real fork, .init_array gap tracked) is
done and committed, the posix suite is green, and every remaining path needs one
of the decisions above.

**Where it bites.** (A) Q13/Q14 — see those entries. (B) the whole `net/` +
kernel socket-syscall layer and a new userspace netstack service. (C)
`gui/gpu/`, `gui/compositor/`. (D) `kernel/src/container.rs`, `pkg/`, plus a
large external dependency.

---

All earlier deferred operator decisions (Q1–Q12) have been resolved — see the
"Recently resolved" list below and `design-decisions.md` for full rationale. New
decisions should be appended above this line as `## Q16 …`.

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
- Q8 Path Z libc + rootfs — resolved 2026-06-14, **operator-delegated to
  Claude**: go straight to **glibc** on an **ext4** rootfs, no musl
  stepping-stone (§25). Claude reversed its own earlier musl-first recommendation
  per the operator's stated preference for hard-work-upfront over throwaway
  scaffolding, given the static-load path is already proven end-to-end.
- Q7 kernel-task-stack-vs-IRQ overflow (B-DF1) — resolved 2026-06-15,
  **operator-chosen option A** (Claude recommended A): per-CPU guard-page IRQ
  stack with a manual nesting-aware switch + deferred preemption, plus the
  `cli`/`sti` recursion guard the restructuring exposed (§26). Validated:
  `http_gzip_8KiB` no longer double-faults at the gzip→dashboard transition.
- Q9 bare-ELF ABI auto-classification — resolved 2026-06-24, **operator-chosen
  option D** (Claude recommended D): default unmarked bare ELF → Linux ABI, add
  `NT_GNU_ABI_TAG` note-walk as a positive Linux signal, stamp native binaries
  with an explicit SlateOS marker; `spawn_process_with_abi` override kept (§33).
- Q10 fullscreen-capture video codec — resolved 2026-06-24, **operator deferred
  to Claude's recommendation**: hardware encode via the GPU driver long-term
  (option C), defer the software-codec port near-term (option D), no stub
  encoder meanwhile; if a software path is ever needed first, AV1/`rav1e` over
  H.264 (§34).
- Q11 zero-copy page-flipping for large channel messages — resolved 2026-06-24,
  **operator-chosen option B** (Claude recommended B): explicit opt-in
  `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region; copy
  path stays the default. Compiler follow-up: keep it programmer/library-
  controlled (library-level auto-threshold helper), the compiler does not
  auto-insert the flag (§35).
- Q12 next large initiative — resolved 2026-06-24, **operator-chosen option E**:
  build the C-lite read-only page cache now; lifts the §23 "not now" hold (§36).

---

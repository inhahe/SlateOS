## 39. Connect the two cgroup subsystems (Q14) — cgroupfs as the frontend, `kernel/src/cgroup.rs` as the enforcement engine (option A)

**Date:** 2026-06-30

**Decided by:** Operator (this was `open-questions.md` Q14; the operator chose
option **A**). The operator's words: *"Q14: A."* Claude recommended A.

**Background.** The OS had two independent cgroup implementations that did not
talk to each other: `kernel/src/cgroup.rs` (the in-kernel resource controller —
the real *enforcement* hooks: the frame allocator charges a task's cgroup on
every `alloc_frame`/`alloc_frame_zeroed` via the per-frame `FRAME_CGROUP` owner
array, plus `io_charge` and PID accounting, reading the current task's group via
`sched::current_task_cgroup()` → `Task::cgroup_id`), and `fs::cgroupfs` (the
user-facing cgroup-v2 filesystem — 5 controllers, hierarchical groups,
`memory.max`, PID limits, per-group process assignment, but **no enforcement**).
Net effect before this change: neither system actually constrains a real
process's memory (cgroupfs limits cosmetic; the kernel controller dormant —
D-CGROUP-TASK-UNASSIGNED).

**The decision.** Wire the two ends into **one pipe**: `fs::cgroupfs` is the
cgroup-v2 **frontend**, `kernel/src/cgroup.rs` is the **enforcement engine**.
Concretely:
- `cgroupfs` controller writes flow through to the kernel controller:
  `memory.max` → `cgroup::set_mem_limit`, and `cgroup.procs` assignment sets the
  target task's `cgroup_id`.
- `fork`/`clone`/`spawn` **inherit** the parent's `cgroup_id` (universal cgroup
  semantics).
- The two group-ID spaces (cgroupfs groups vs. `cgroup.rs` `CgroupId`, capped at
  256) are reconciled, and the 5 controllers mapped through.

**Alternatives considered (from Q14).**
- **(B) Collapse onto one (rejected).** Delete/absorb one implementation. *Pro:*
  eliminates duplication entirely. *Con:* biggest blast radius; risks regressing
  whichever subsystem's self-tests; `cgroup.rs` is on the allocator hot path so
  its per-frame `u8` owner array must be preserved regardless — so the "collapse"
  saving is smaller than it looks.
- **(C) Containers drive `cgroup.rs` directly, leave cgroupfs standalone
  (rejected).** *Pro:* smallest change to make container memory limits real.
  *Con:* leaves two permanently-parallel ways to express "a cgroup" — confusing
  long-term.

**Rationale.** The frame-allocator charging in `cgroup.rs` is the correct,
hot-path-proven enforcement engine, and cgroup-v2 (`cgroupfs`) is the right
user-facing model — they should be two ends of *one* pipe, not two pipes. A also
keeps both subsystems in their current roles (lowest regression risk to the
existing self-tests) while finally making limits real.

**Where it bites.** `kernel/src/cgroup.rs` (`set_mem_limit`, `mem_charge`,
`current_task_cgroup`), `kernel/src/fs/cgroupfs.rs` (controller writes, process
assignment), `kernel/src/sched/task.rs` (`cgroup_id` field + 3 constructors,
all defaulting to `ROOT_CGROUP`), `kernel/src/sched/mod.rs` (a lock-taking
`set_task_cgroup` setter), `kernel/src/container.rs` (`Container::cgroup_id`),
and the task-creation paths in `kernel/src/proc/{fork,thread,thread_clone,spawn}.rs`
(cgroup inheritance).

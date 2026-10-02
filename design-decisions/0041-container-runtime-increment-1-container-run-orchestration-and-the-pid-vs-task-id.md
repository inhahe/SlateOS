## 41. Container runtime increment 1 — `container::run()` orchestration and the PID-vs-task-id binding split

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). The operator pre-approved the
Docker/container-runtime port as a whole in §40 ("the explicit operator go-ahead
the standing rule required"); this entry records the specific implementation
choices Claude made within that scope for the first increment.

**Context.** §40 chose initiative D (container runtime). The container subsystem
(`kernel/src/container.rs`) already had the full create/start/stop/delete state
machine plus all four namespaces, a cgroup, and veth networking — but nothing
actually *launched a process inside a container*. `start()` only flipped the
state flag; `add_process()` bound a pre-existing (synthetic, in tests) PID.
Increment 1's job: a real `docker run`-equivalent that spawns an init process,
binds it to the container's cgroup (so Q14/§39 billing applies), and transitions
to Running.

**The decision.**
1. **Add `container::run(id, elf_data, options) -> pid`** that orchestrates
   spawn → bind → Running atomically: it validates the container is `Created`,
   calls `proc::spawn::spawn_process` (the process is enqueued but does not
   execute until the scheduler picks it, so the binding is guaranteed in place
   before its first instruction), binds it, records the init PID, and flips to
   Running. On any post-spawn failure it tears the process down
   (`kill_process_threads` + `pcb::destroy`) so a failed run leaks nothing.
2. **Split the process-id from the task-id in the binding path.** A spawned
   process's global PID and its initial thread's scheduler *task id* are
   independent allocations (observed in the self-test: pid=215, task=179). The
   scheduler-level resources — cgroup billing (`set_task_cgroup`) and network
   namespace (`set_task_net_ns`) — are keyed on the **task id**; the PID-namespace
   mapping and the container's tracked-process list are keyed on the **PID**.
   The old `add_process(id, global_pid)` conflated them, which silently no-ops
   the cgroup/net-ns assignment whenever PID ≠ task id (the cgroup `set` fails to
   find a task with that id). Fixed by adding `add_process_task(id, pid, task_id)`
   / `remove_process_task(id, pid, task_id)` as the real entry points;
   `add_process`/`remove_process` are now thin wrappers passing `pid` as both
   (correct only for the current-task case, e.g. the existing net-ns self-test).
   Threads the process spawns later inherit the cgroup automatically
   (`sched::spawn` copies the creator's `cgroup_id`), so binding the initial
   thread suffices.

**Alternatives considered.**
- *Have `run()` reuse `add_process(id, pid)` unchanged.* Rejected: it would bill
  nothing to the cgroup (PID ≠ task id), defeating the entire point of building on
  Q14. The conflation was a latent bug regardless of `run()`.
- *Make `add_process` take both ids and update its one existing caller.* Would
  also work, but keeping the single-id wrapper preserves the ergonomic
  "bind the current task" call site (`add_process(id, current_task_id())`) used by
  the net-ns-propagation self-test, where PID==task by construction.
- *Spawn-into-namespaces (clone-style) vs. spawn-then-bind (setns-style).* This
  increment uses spawn-then-bind because the process does not run until after the
  bind completes, so the result is observably equivalent for a single init
  process. The genuine clone-vs-setns fork (relevant once a container must
  *enter* an existing namespace mid-life, and for mount-namespace/rootfs/
  pivot_root) is deferred to a later increment; it is an implementation choice,
  not an operator policy fork, so it will be resolved autonomously and recorded
  here when reached.

**Deferred to later increments (not in increment 1).** Mount-namespace field +
rootfs / `pivot_root` on the `Container` struct (it currently has no mount_ns
field); OCI image pull/unpack + overlayfs; a userspace `docker run` CLI. The
cgroup `nr_tasks` accounting asymmetry surfaced while writing the self-test
cleanup is logged in `known-issues.md` (it is pre-existing Q14 behavior, not
introduced here).

**Where it bites.** `kernel/src/container.rs` (`run`, `add_process_task`,
`remove_process_task`, the `init_pid` field, self-test 17). Relies on §39 (Q14)
for the cgroup billing that `run` exercises end-to-end.

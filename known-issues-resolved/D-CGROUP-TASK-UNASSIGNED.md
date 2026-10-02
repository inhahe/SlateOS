### D-CGROUP-TASK-UNASSIGNED. Cgroup memory controller now reachable for real workloads — RESOLVED (2026-07-01)

**Original problem:** every `Task` was constructed with
`cgroup_id: ROOT_CGROUP` and no path ever set it to anything else, so
`current_task_cgroup()` always returned root, `charge_cgroup_alloc`
fast-exited, and the per-cgroup memory limit / accounting was never
exercised by real workloads — only by self-tests charging an explicit
cgroup. Container memory limits did not actually constrain memory.

**Resolution (Q14, operator option A):**
1. **Assignment path** — `sched::set_task_cgroup(task_id, cgroup)`
   (`kernel/src/sched/mod.rs:1287`) is the single authoritative
   process→cgroup assignment: it swaps `task.cgroup_id` under the SCHED
   lock and keeps the cgroup `nr_tasks` counts consistent (detach old,
   attach new) with a strict SCHED→cgroup-TABLE lock order.
   `container.rs` `add_process_task` (line ~1543) calls it to move a
   container's task into the container's cgroup, and `remove` (line
   ~1640) moves it back to root.
2. **Inheritance path** — `sched::spawn` (`mod.rs:1031/1046`) captures
   `current_task_cgroup()` before the task-creation critical section and
   copies it onto the new task, so `fork` (routes through
   `thread::spawn`→`sched::spawn`), `thread_clone`, and `spawn_user`
   (also `→sched::spawn`) all inherit the creating task's cgroup — Linux
   fork/clone semantics. Recorded in design-decisions §39.
3. **End-to-end test** — `cgroup_e2e_test_task` in `kernel/src/main.rs`
   runs as a live scheduler task (so `current_task_cgroup()` resolves to
   a real task, unlike the no-task kmain self-tests): it creates a
   memory-limited child cgroup, joins it via `set_task_cgroup`, allocates
   N=32 frames through the ordinary `alloc_frame` path (into a stack
   array — no heap growth to perturb the count), and asserts the group's
   `mem_usage` rose by exactly N; then frees them and asserts usage
   returns to baseline (uncharge follows the per-frame `FRAME_CGROUP`
   record, so it debits the right group even after the task rejoins root).
   Prints `[cgroup-e2e] PASS`/`FAIL` on the boot serial log.

**Discovered:** 2026-06-30 while fixing B-CGROUP-DBLCHARGE. **Resolved:**
2026-07-01 once Q14 settled which layer owns process→cgroup assignment
(`kernel/src/cgroup.rs` enforces + owns assignment via `set_task_cgroup`;
`fs::cgroupfs` remains the config frontend).

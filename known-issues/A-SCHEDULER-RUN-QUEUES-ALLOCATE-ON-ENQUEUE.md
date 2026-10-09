### A-SCHEDULER-RUN-QUEUES-ALLOCATE-ON-ENQUEUE -- 2026-10-09 -- OPEN (lane A)

**Status:** OPEN (lane A). Found with the two-CPU hang's root cause
(design-decisions §1563), which made these allocations safe but not
allocation-free.

**In short:** putting a task on a CPU's run queue -- every wake, every
preemption, every spawn -- can allocate memory. The round-robin queues are
`VecDeque`s that grow, and the EEVDF and deadline backends keep `BTreeMap`s
whose inserts allocate. Wakes happen in interrupts (a timer's sleep ending, a
device's interrupt), so the kernel allocates in interrupt context on its
hottest path. Since §1563 that no longer deadlocks. But an allocation that
fails for want of memory, when the system is nearly out, panics the kernel
(Rust's allocation-failure handler). A wake under memory pressure, the moment
the system most needs to keep running, would take the machine down instead.

**Where:** `kernel/src/sched/priority_rr.rs` (`PriorityRoundRobin::queues`,
`enqueue`), `kernel/src/sched/eevdf.rs` and `kernel/src/sched/deadline.rs`
(their trees), reached through `backend::SchedulerBackend::enqueue` and
`PerCpuScheduler::enqueue`.

**How you would notice:** a kernel panic for a failed allocation, in an
interrupt or in the scheduler, while memory is nearly exhausted -- where the
OOM killer, given the chance, would have freed some.

**Proper fix:** run queues that cannot fail: each task carries its own queue
links (an intrusive list, Linux's `sched_entity.run_node`), made when the task
is made -- in process context, where a failed allocation is a failed spawn --
so enqueueing only links. For the round-robin backend that is a doubly linked
list per (CPU, level) through a per-task node, which also makes a task
structurally unable to be queued twice (`PriorityRoundRobin::dequeue`'s sweep
for duplicates exists because it can be now). EEVDF and the deadline backend
need an intrusive ordered structure (an intrusive red-black or AVL tree, as
Linux's `rb_node`).

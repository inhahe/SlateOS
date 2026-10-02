## 964. The scheduler takes no run-queue entry on trust: removal is by id, a pick checks that the task can run, and an exiting task that is resumed re-parks instead of halting

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** the lists of tasks waiting for a processor could keep an entry
for a task that had died or gone to sleep, because the code that takes a task
off a list looked in only one place on it. When such a leftover entry reached
the front, the scheduler ran whatever it named -- a dead task came back to
life -- and when the leftover belonged to a task in the middle of exiting,
the kernel stopped the whole machine without printing anything. That was the
intermittent silent boot hang of known-issues.md `B-FORKEXEC-BOOT-HANG`. Now
removal searches the whole list, the scheduler checks every task before
running it, and the exit path reports and carries on instead of stopping the
machine.

**What was decided.**

1. **Removal is by id.** `PriorityRoundRobin::dequeue` removes every entry
   for the task at every priority level, ignoring the level the caller
   computed, and `PerCpuScheduler::dequeue` sweeps every online CPU's queue,
   not only the one `last_cpu` names. (`dequeue_any`, which the
   anti-starvation booster had for exactly this, is folded in.)
2. **A pick checks what it picked.** `pick_runnable_locked` -- the only path
   from a queue entry to a CPU, used by both the main switch and the idle
   fallback -- drops and reports (`*** BUG: stale run-queue entry`) an entry
   for a task that is `Dead`, `Blocked`, `Suspended`, `Running` elsewhere or
   gone from the table, and resumes the current task in place only if it is
   `Ready` or `Running`. On SMP it also hands a task still executing on
   another CPU back to that CPU instead of running it twice, and re-homes a
   stolen task that its affinity forbids here.
3. **Death purges.** `kill_task` and `task_exit` remove every entry of the
   task under the guard that publishes it `Dead`.
4. **An exiting task that is resumed re-parks.** `task_exit` is `-> !` and
   loops: resumed after publishing `Dead`, it prints
   `*** BUG: task N was resumed after it exited`, marks the task `Dead`
   again and switches away. It used to fall into `cpu::halt_loop()`, which
   halts with interrupts disabled.

**Alternatives considered.**

| option | why not |
|---|---|
| Record each task's queue position in `Task` (Linux's `on_rq`), making removal O(1) and a double enqueue detectable at the enqueue | every path that moves entries between CPU queues without the task table -- push balance, the CPU-offline drain, work stealing -- would have to keep it current, and those are the paths that already let `last_cpu` go stale. By-id removal needs no bookkeeping to be right. Revisit if a removal ever shows up on a hot path |
| Fix only the removals that missed, and keep trusting the pick | the pick is the one place every stale entry passes through, whatever left it there: checking there closes the whole class, including sources nobody has found yet, for one table lookup per switch |
| On a resumed dead task, panic -- fail-stop, but loud | the resumed task is harmless to re-park, and a panic turns a recoverable scheduler inconsistency into a lost boot -- the outcome being fixed. The report keeps it from being silent, and `DEAD_TASKS_RESUMED` counts it |
| Keep `cpu::halt_loop()` but print first | still stops a uniprocessor machine: every later diagnostic, watchdog and test is lost to a condition the scheduler can recover from |

**Cost.** One task-table lookup per pick (the `BTreeMap` the switch already
reads twice) and, on SMP, one atomic load per online CPU. Removal now visits
every queued entry on every online CPU instead of one level; no removal is on
a per-switch path.

**Revisit if** removal ever lands on a hot path (then track the queue
position in `Task`), or a stale-entry report appears in a healthy boot: the
report's `found by` site and state name the removal that missed.

**Where this bites:** `kernel/src/sched/mod.rs` (`pick_runnable_locked`,
`classify_pick`, `running_elsewhere`, `report_stale_rq_entry`, `task_exit`,
`kill_task`, `starvation_boost_locked`, `test_stale_run_queue_entries`),
`kernel/src/sched/priority_rr.rs` (`PriorityRoundRobin::dequeue`,
`PerCpuScheduler::dequeue`).

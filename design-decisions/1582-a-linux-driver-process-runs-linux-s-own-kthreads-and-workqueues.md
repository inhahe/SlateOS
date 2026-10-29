## 1582. A Linux driver process runs Linux's own kthreads and workqueues; a subsystem's failing initcall is carried past, and threads that wait for work do not hold the process

**Date:** 2026-10-28 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); how the layer runs Linux's threads is Claude's call, under §1578 · **Lane:** A

**In short:** Linux drivers hand work to "workqueues" (pools of helper
threads Linux keeps) and start threads of their own. Rather than imitate
these, each driver process now runs Linux's own thread and workqueue code,
the layer supplying only how a thread is born and how it ends. Two rules
follow that are judgement calls. Linux's own subsystems sometimes fail to
set themselves up in a driver process -- the workqueues' registration in
sysfs, a file system the process does not have -- and Linux carries on past
such failures, so the process does too; only the driver's own failing ends
it. And helper threads idly waiting for work (kthreadd, the workqueues'
workers) no longer make the process look stuck: it is done when every other
thread has finished.

### The decision

1. **Linux's own `kernel/kthread.c` and `kernel/workqueue.c`** (with
   `lib/idr.c`, `radix-tree.c`, `xarray.c`, `bitmap.c`, `kasprintf.c`,
   `kernel/time/time.c`) are compiled into every driver process, replacing
   the layer's own kthread code. The layer gives what Linux's fork, exit,
   scheduler and memory manager would: `kernel_thread()` (a task with a
   record of its own), `do_exit()`, a `kthreadd` task started before the
   first initcall, a slab (`kmem_cache_*`) and per-CPU memory over kmalloc,
   `rcuwait`, `kfree_rcu`, and Linux's scheduler hooks for workqueue
   workers (`wq_worker_sleeping` / `wq_worker_running`), without which a
   work item that sleeps would hold up every other item on its pool.
   Workqueues are initialised where Linux initialises them:
   `workqueue_init_early()` before the loop first turns,
   `workqueue_init()` once kthreadd can make workers.
2. **A failing initcall is fatal only when it is the driver's own.** The
   build marks the code of the driver's own objects (`scripts/linux-tree.sh`
   renames their `.text` and `.init.text` to `*.lxdriver`), the linker
   script brackets it (`__driver_text_start` / `__driver_text_end`), and an
   initcall whose function lies there ends the process on failure (exit
   126), at whatever level it runs; a failure anywhere else -- a Linux
   subsystem's -- is logged and carried past. Linux itself ignores every
   initcall's return value beyond logging it. A positive return, which no
   initcall should give, is logged and taken as success, as Linux takes it.
3. **Daemons.** A thread idly waiting for work -- kthreadd, a workqueue
   worker (`PF_WQ_WORKER`) or a kthread worker (`kthread_worker_fn`) with
   no work item in hand -- does not keep the process from being done: when
   nothing can run again, the process exits 0 if every thread left waiting
   is such a daemon, DEADLOCK (124) otherwise. A worker blocked in the
   middle of a work item is not idle, so a stuck work item is a deadlock.
4. **Linux's tick, where workqueues need it.** A work item that hogs the
   CPU, giving way only at `cond_resched()`, is marked CPU-intensive as
   Linux's tick marks it (`wq_worker_tick`), so that its pool runs the work
   queued behind it: `cond_resched()` does the tick's part once a task has
   run a jiffy, the layer counting each task's run time (at every switch
   and at the tick), and `loops_per_jiffy` reads what a 4 GHz CPU
   calibrates, so that the threshold is Linux's 10 ms, not the second it
   would be at the uncalibrated value.
5. **No sysfs** -- *superseded by §1583 (2026-10-29)*, under which Linux's
   own driver core runs and sysfs registration succeeds, showing nothing.
   It was: a subsystem registering with sysfs is told it cannot
   (`subsys_virtual_register` answers `-ENODEV`).
6. **RCU callbacks run in softirq context**, as Linux's RCU runs them: one
   that would sleep ends the process as a BUG. The slab refuses what it
   cannot keep: a `SLAB_TYPESAFE_BY_RCU` cache is a stop (its objects'
   memory must stay the cache's past their free, which kfree does not
   keep), and a `SLAB_PANIC` cache that cannot be made ends the process
   (exit 127).

### The alternatives

- **The layer's own workqueues** (a thread per queue running items in
  order): far less code, but workqueue semantics are subtle and drivers
  depend on them -- `flush_work`, `cancel_delayed_work_sync`, ordered and
  unbound queues, `WQ_MEM_RECLAIM` rescuers, concurrency management -- and
  every one would be a reimplementation to keep in step. §1578 chose
  Linux's own code wherever the bottom allows.
- **Every failing initcall fatal** (as before): the workqueues' sysfs
  registration fails in every process, so every process would end before
  its driver ran; and Linux's own subsystems will meet more of this.
- **Fatal by level** (an initcall at device level or later is the
  driver's): simpler, but a driver may register a `core_initcall` or
  `subsys_initcall` of its own, and a Linux subsystem may run at device
  level; the level says when an initcall runs, not whose it is.
- **Make the daemons finish** (stop kthreadd and the workers when the
  driver's initcalls are done): it would let the old exit rule stand, but a
  real driver needs its workers for as long as it lives, and the rule is
  only about telling a stuck process from a finished one.

### What it costs

- Work behind a deferrable timer (`TIMER_DEFERRABLE`, as a deferrable
  delayed work item is) does not keep the process alive: once only idle
  daemons are left, the process ends without waiting for it, as an idle
  Linux CPU does not wake for such a timer. Work queued now wakes a worker,
  and work behind an ordinary timer keeps the loop waiting for it, so both
  are run; a work item a worker has begun is waited for.
- (Until §1583, every driver process logged the workqueues' failed sysfs
  registration once; now it succeeds, and the failed initcall every process
  logs is `kernel/resource.c`'s `iomem` mount, §1584.)
- The build renames the driver objects' sections, so a driver's own code
  must live in `.text` or `.init.text` for its initcall to count as the
  driver's (an initcall in another section would be carried past). Linux's
  `__init` and plain functions are both covered.
- The process is bigger: the hello driver grows from about 150 KB to about
  255 KB.

### Where it lands

`linuxdrv/emul/manifest`, `linuxdrv/emul/lx_sched.c` (kernel_thread,
do_exit, kthreadd, the daemon rule's question, the worker hooks and tick,
rcuwait, kfree_rcu), `linuxdrv/emul/lx_emul.c` (slab, per-CPU memory,
sysfs's refusal, the initcall rule, run-time accounting, population counts,
workqueue init), `linuxdrv/emul/lx_softirq.c` (RCU callbacks in softirq
context, the turn's accounting), `scripts/linux-tree.sh` (the driver's
sections marked), `linuxdrv/link.ld` (`__driver_text_start/_end`),
`linuxdrv/kit/src/sched.rs` (the exit rule, `slx_task_exit`), and the hello
driver's tests ("earlyfail" among the kernel's runs).

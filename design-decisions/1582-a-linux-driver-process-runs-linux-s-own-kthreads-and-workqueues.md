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
it. And helper threads that wait for work forever (kthreadd, the workqueues'
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
2. **A failing initcall below device level is logged and carried past**;
   one at device level or later -- where a driver's `module_init` runs --
   still ends the process (exit 126). The linker script marks the boundary
   (`__initcall_device_start`). Linux itself ignores every initcall's
   return value beyond logging it.
3. **Daemons.** A thread that only waits for work -- kthreadd, a workqueue
   worker (`PF_WQ_WORKER`), a kthread worker (`kthread_worker_fn`) -- does
   not keep the process from being done: when nothing can run again, the
   process exits 0 if every thread left waiting is such a daemon, DEADLOCK
   (124) otherwise.
4. **No sysfs.** A subsystem registering with sysfs is told it cannot
   (`subsys_virtual_register` answers `-ENODEV`): a driver process has no
   file system for it, and turning `CONFIG_SYSFS` off needs `CONFIG_EXPERT`,
   which under `allnoconfig` also turns off `BUG`, `BLOCK`, `EPOLL` and
   dozens more -- a different configuration altogether.

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
- **Make the daemons finish** (stop kthreadd and the workers when the
  driver's initcalls are done): it would let the old exit rule stand, but a
  real driver needs its workers for as long as it lives, and the rule is
  only about telling a stuck process from a finished one.

### What it costs

- A work item stuck forever with nothing else waiting on it now reads as a
  finished process (exit 0), not DEADLOCK: its worker is a daemon. A driver
  waiting on that item (flush_work) is not a daemon, so the usual case is
  still caught.
- Every driver process logs the workqueues' failed sysfs registration
  once ("subsystem initcall ... failed: -19; carrying on").
- The process is bigger: the hello driver grows from about 150 KB to about
  255 KB.

### Where it lands

`linuxdrv/emul/manifest`, `linuxdrv/emul/lx_sched.c` (kernel_thread,
do_exit, kthreadd, the daemon rule's question, the worker hooks, rcuwait,
kfree_rcu), `linuxdrv/emul/lx_emul.c` (slab, per-CPU memory, sysfs's
refusal, the initcall rule, population counts, workqueue init),
`linuxdrv/link.ld` (`__initcall_device_start`), `linuxdrv/kit/src/sched.rs`
(the exit rule, `slx_task_exit`), and the hello driver's workqueue tests.

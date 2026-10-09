### A-CPU-AFFINITY-DID-NOT-MOVE-A-RUNNING-THREAD -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** limiting a thread to certain CPUs ("affinity") did not reliably
take effect, in four ways:
- A thread that was running when its affinity changed kept running where it
  was for as long as it stayed busy. The most common case is a thread pinning
  itself, so the most common use did nothing until the thread next slept.
- A mask naming only CPUs that are not online could put a task on a CPU's
  queue that no CPU serves, where it waited forever.
- New threads, forked children and spawned programs ignored their creator's
  affinity and could run on any CPU.
- The Linux calls applied nothing. `sched_setaffinity` reported success and
  `sched_getaffinity` always answered "every CPU", so no Linux program could
  pin itself at all.

**Where:** `kernel/src/sched/mod.rs` -- `classify_pick`, which resumed the
current task in place whatever its mask said; `choose_cpu_for_task`, whose
fallback took the lowest CPU in the mask whether it was online or not;
`set_cpu_affinity`, which moved only queued tasks. Also
`kernel/src/proc/thread.rs`, where every process thread was created with every
CPU, and `kernel/src/syscall/linux.rs`, `sys_sched_{set,get}affinity`.

**Fixed:**
- `sched::set_affinity` stores the mask and refuses one with no online CPU.
  A queued task changes queue at once. A running task moves at its next
  switch, which is asked for: a caller that moved itself switches before
  returning, and a task running on another CPU has that CPU asked to
  reschedule (`request_preempt_on`).
- The pick moves a current task whose mask forbids its CPU, instead of
  resuming it there. It does not do so in the idle fallback, where the task's
  context is still live and another CPU could only hand it back.
- The fallback CPU choice looks only at online CPUs.
- A thread takes its creator's mask.
- The Linux calls read and set the real mask, under the native call's
  permission rule (design-decisions 1515).

`sched::affinity_self_test` checks both moves on a two-CPU boot: a thread
pinning itself elsewhere returns there, and a spinning thread on another CPU,
pinned here, comes here.

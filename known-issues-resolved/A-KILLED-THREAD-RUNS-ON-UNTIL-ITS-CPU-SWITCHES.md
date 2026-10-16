### A-KILLED-THREAD-RUNS-ON-UNTIL-ITS-CPU-SWITCHES -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** the safe half of the entry above is done; the semantic half is
not. A thread killed while another CPU runs it goes on running its program,
in user mode, for up to one timer tick after its process has been declared
dead -- its handles closed, its parent told. It can still write to memory it
shares with other processes in that tick. Linux has each killed thread end
itself (it sees `SIGKILL` at its next return to the kernel, or an IPI makes
it look), so nothing of a dead process runs.

**Where:** `kernel/src/sched/mod.rs::kill_task` (marks a running task `Dead`
and leaves it running), `kernel/src/proc/thread.rs::kill_process_threads`
(runs each victim's exit path from the killer's context).

**Fixed:** the killer now interrupts the victim's CPU and waits for the
thread to leave it before publishing anything.
- `sched::kill_task_from` reports the state a task was killed from. For one
  that was `Running`, it asks that CPU to reschedule at once
  (`sched::request_preempt_on`, which sets the CPU's `NEED_RESCHED` and sends
  the reschedule interrupt; it was written the same day for CPU affinity).
- `proc::thread::kill_process_threads` kills every thread first. It then
  waits, spinning, up to 200 ms, until each one that was running has left its
  CPU (`sched::wait_off_cpu`). Only then does it run the exit paths, the last
  of which publishes the process as dead. `kill_thread` does the same for one
  thread.

A `Dead` task is never picked again, so once it has left, nothing of the
process runs. The address-space deferral above stays as the safety net for
the one case the wait gives up on: a CPU that kept interrupts off for 200 ms.
That case is logged. This takes a different route from the one proposed
here -- running the exit path on the victim's own CPU -- to the same
guarantee.

`sched::affinity_self_test` checks it on a two-CPU boot. A spinning thread on
the other CPU is killed; it must have been `Running`, and it must leave.

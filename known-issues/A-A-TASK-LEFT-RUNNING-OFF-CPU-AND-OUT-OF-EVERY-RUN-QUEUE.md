### [A] `A-A-TASK-LEFT-RUNNING-OFF-CPU-AND-OUT-OF-EVERY-RUN-QUEUE` — a woken task can be left marked Running, on no CPU and in no run queue, and never runs again -- 2026-10-15
**Status:** OPEN (lane A). Intermittent; being reproduced.

**In short:** now and then a boot self-test fails because the program it
runs stops dead: "the program did not finish in 60 s". In the one case with a
full dump, the stuck thread is marked as running, yet the processor is idle
and no queue holds the thread, so nothing will ever run it again. The machine
goes on (the next test runs); the thread is lost.

**Seen.**

- Lane A's quick boot of 2026-10-15 (lane-a-wip at 4b6f6bf85 plus the
  `/dev/zram0` change, `selftest.keep_going=1`): `[spawn] FAIL: mlock (ring
  3) -- the program did not finish in 60 s`. The liveness watchdog's
  SYSTEM HANG dump, 15 s in: task 592 (`spawn-test-linux-mlock`)
  `state=Running`, its last park `wait.rs:449 wait=(child 593)`,
  `block_seq=1`; its child 593 `Dead` (it had exited, so the parent was
  woken); cpu0 `local_has_real_work=false`, halted in
  `sched::schedule_inner` under the boot idle task's `block_current`; no
  deferred wake queued, none dropped.
- Lane F's boot of `fd9c28d0d` (2026-10-12), the real-time scheduling
  self-test's timeout, was first listed here. Reproduced with the per-task
  report, it is a different bug: its thread was left `Blocked`, the wake
  owed to its park erased by a preemption before it
  (`A-A-PREEMPTION-BEFORE-A-PARK-ERASED-THE-WAKE-OWED-TO-IT`, fixed).

**What it is not.** Not a lost wake: the task was woken -- it is not
`Blocked` -- and `try_wake`'s `pending_wake` contract was not involved
(`pending_wake=false`). A task becomes `Running` only when a CPU commits to
it (`schedule_inner`'s pick, the idle fallback's switch, or a resume in
place), so one of those committed to 592 and the CPU then ran something else
without putting 592 back in a queue.

**Next.** The mlock program, 300 runs on an unloaded guest, did not fail.
The scheduler now records, per task, when and by which path a CPU last
committed to it and last switched away from it (`Task::last_dispatch`,
`Task::last_switch_out`); names any task `Running` on no CPU in the liveness
dump and after a second of the idle fallback (`report_running_orphans`);
and, for a program that misses its deadline -- the ring-3 self-tests that
wait by the clock, lane D's C fixtures, the guest agent's runs -- prints
each of its threads and every CPU's preemption state
(`proc::spawn::wait_for_fixture`). The mlock rung is one of them; waits that
count yields instead (the 2000-yield rungs, the 15 s capstones) print
nothing yet. The next occurrence names the path that
committed without switching. Repeat the mlock program under host load.

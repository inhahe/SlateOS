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
- Lane F's boot of `fd9c28d0d` (2026-10-12,
  `requests/f-a-the-real-time-scheduling-self-test-timed-out-on-a-tree-that-had-passed.md`):
  `[spawn] FAIL: real-time scheduling (ring 3) -- the program did not finish
  in 60 s`, its main thread never running again after a `nanosleep` while a
  spinner thread held the CPU. No task dump there, so whether it is this bug
  is not yet known.

**What it is not.** Not a lost wake: the task was woken -- it is not
`Blocked` -- and `try_wake`'s `pending_wake` contract was not involved
(`pending_wake=false`). A task becomes `Running` only when a CPU commits to
it (`schedule_inner`'s pick, the idle fallback's switch, or a resume in
place), so one of those committed to 592 and the CPU then ran something else
without putting 592 back in a queue.

**Next.** Reproduce by repetition (the mlock and real-time programs, many
times, on a guest under host load) with the scheduler recording, per CPU, the
task each commit chose and each switch actually made, so the commit that
went unswitched names itself.

### [A] `A-A-PREEMPTION-BEFORE-A-PARK-ERASED-THE-WAKE-OWED-TO-IT` — a preemption between arming a wake and parking erased the wake, and the park never ended -- 2026-10-15
**Status:** OPEN -- fixed on lane-a-wip (2026-10-15), awaiting a boot on main.

**In short:** a program that slept for a moment could sleep forever. A
sleep arms a timer and then parks the thread. When the timer fires first,
the wake is noted on the thread so that its park returns at once. A
preemption in between -- the timer interrupt's own, often -- wiped that note,
so the thread parked waiting for a wake that had already come. Nothing woke
it again. The real-time scheduling self-test hit this under load: its main
thread never returned from a 1 ms `nanosleep`.

**Seen.**

- Lane F's boot of `fd9c28d0d` (2026-10-12,
  `requests/f-a-the-real-time-scheduling-self-test-timed-out-on-a-tree-that-had-passed.md`):
  `[spawn] FAIL: real-time scheduling (ring 3) -- the program did not finish
  in 60 s`.
- Lane A's repetition of the same program on a guest (`scripts/guest.py run
  --grants realtime`, two host busy loops beside it): a timeout at run 21,
  and at run 19 with the scheduler's per-task report
  (`sched::report_task`, printed by `proc::spawn::wait_for_fixture` when a
  fixture misses its deadline). The main thread: `state=Blocked
  policy=Fifo`, parked at `syscall/linux.rs` `interruptible_sleep_until`
  with `wait=(timer ...)` 40 s in the past, `pending_wake=false`, no hrtimer
  of its own pending, no deferred wake queued or dropped. Its spinner
  sibling was `Ready`, queued, and running; CPU 0 preemptible
  (`preempt_count=0`, timer running).

**Cause.** `Task::mark_ready` cleared `pending_wake` on every transition to
`Ready`. A wake that finds its task not yet parked is recorded in that flag
(`wake`, `try_wake`, the deferred-wake drain, the sleep queue) and consumed by
the next `block_current`. But `schedule_inner` requeues a preempted or
yielding task through `mark_ready` too, so a preemption anywhere between the
wake and the park erased it. In `interruptible_sleep_until` the window runs
from `hrtimer::schedule_ns` to `block_current_on`: under load a 1 ms timer
routinely fires inside it (the main thread parked for real only 4 times in
hundreds of sleeps), and the timer interrupt that fires it is also the one
that preempts.

**Fix.** `mark_ready` clears `pending_wake` only when the task was
`Blocked`, where the wake making it ready is the one the park waited for (and
the flag is false anyway: a wake aimed at a parked task readies it rather
than being recorded). A recorded wake now outlives a preemption, a yield, a
throttle and a stop on the way to its park. One owed to nothing -- a timer
that fired after its wait ended another way -- costs the next park an early
return, which every park's caller already survives by rechecking its
condition.

**Test.** `sched::test_try_wake_contract` records a wake on the running boot
task, yields (the requeue a preemption takes), and checks the wake is still
recorded before parking on it. With the old `mark_ready` restored it fails
with `a wake recorded on a running task was erased by a switch before its
park`.

**Not this bug.** `A-A-TASK-LEFT-RUNNING-OFF-CPU-AND-OUT-OF-EVERY-RUN-QUEUE`
(the mlock self-test's thread left `Running` on no CPU) is a different
signature, still open.

### B-PTHREAD-JOIN-LOST-CTID. `pthread_join` intermittently blocks forever because the child's `CLONE_CHILD_CLEARTID` was registered *after* the child was made runnable — FIXED 2026-08-13

**Symptom.** The Path-Z real-glibc pthread self-test times out rather than
producing a wrong answer (this is a *different* failure from
B-PTHREAD-CHILD-JUMPS-TO-GARBAGE below, which returns wrong counters):

```
[spawn]   FAIL: real glibc pthread — process did not exit within 262144 yields
          (state=Some(Running)); a thread likely deadlocked on a futex or a worker faulted
WARNING: Path-Z real glibc pthread self-test failed: TimedOut
```

Caught on iteration 1 of soak run `soak-20260813-222924`
(`build/hang-catches/soak-20260813-222924-iter01.serial.txt`), i.e. at roughly
the same ~1-in-10 rate as the other pthread flakes.

**Reading the serial log — how to tell this apart from a worker fault.** The
region around the failure is:

```
[sched] Spawned task 265 ...            <- main thread of process 296
[sched] Spawned task 266 ...
[sched] Task 266 exiting                <- worker exits IMMEDIATELY after spawn
[sched] Spawned task 267 / 268 / 269
[sched] Task 267 exiting
[sched] Task 268 exiting
[sched] Task 269 exiting
[thread] Process 296 has no threads left — now zombie
[spawn]   FAIL: ... (state=Some(Running))
```

All four workers exit cleanly and none faults, yet the main thread 265 never
prints `[sched] Task 265 exiting`. Two details are easy to misread:

* The `now zombie` line is **not** task 265 exiting. It comes from the test's
  own cleanup, `thread::on_thread_exit(result.task_id)` at
  `kernel/src/proc/spawn.rs` (in the glibc-pthread test, just after the state
  snapshot), which removes the still-registered main thread. That is also why
  the `FAIL` line can report `state=Some(Running)` on the line *after* a
  `now zombie` message — the state was sampled before that cleanup ran.
* In a healthy boot the order is `... 269 exiting` → `now zombie` →
  `Task 265 exiting`, because `sys_exit` calls `on_thread_exit` (which prints
  the zombie line) *before* `sched::task_exit` (which prints the exit line).
  Compare `soak-20260813-093459-iter02/03/09` for the healthy shape.

So the signature is precisely: **every worker exits, the joiner never does.**

**Root cause.** `proc::thread_clone::clone_thread` called
`thread::spawn_with_tls(...)`, which *admits* the child (makes it runnable)
before returning, and only then ran:

```rust
let task_id = thread::spawn_with_tls(...)?;   // <-- child is RUNNABLE here
...
if (args.flags & CLONE_CHILD_CLEARTID) != 0 && args.child_tid_ptr != 0 {
    register_clear_child_tid(task_id, args.child_tid_ptr);   // <-- too late
}
```

On the uniprocessor a timer preemption in that window lets the child run to
completion. `on_thread_exit_hook` then does
`match CLEAR_CHILD_TID.lock().remove(&task_id) { Some(p) => p, None => return }`
— it takes the `None` path and performs **neither** the zero-write to `*ctid`
**nor** the `futex_wake(ctid_ptr, 1)`. glibc's `pthread_join` is parked on
exactly that word, so it blocks forever on a tid that will never be cleared;
the process never zombifies and the test burns its whole yield budget. The
late `register_clear_child_tid` also left a permanent `CLEAR_CHILD_TID` entry
keyed on a task id that had already exited (a slow leak, and a latent mis-fire
if task ids are ever recycled).

`CLONE_CHILD_SETTID` had the same window with an inverted effect: had the
child exited first, the parent's `copy_to_user` of the tid would have landed
*after* the exit hook zeroed `*ctid`, resurrecting a non-zero tid into a dead
thread's descriptor — hanging `pthread_join` just as surely.

`proc::fork::fork` carried an identical copy of the defect (its
`CLONE_CHILD_CLEARTID` registration also ran after `spawn_with_tls` returned).

**Why the futex layer was *not* at fault.** Two plausible-looking alternatives
were checked and cleared, so a future session need not redo them:
`futex_wait_bitset` performs its `*addr == expected` compare and its wait-queue
enqueue under one `FUTEX_TABLE.lock()`, so a wake cannot slip between the
compare and the enqueue; and the window between dropping that lock and calling
`sched::block_current()` is covered by the scheduler's sticky `pending_wake`
flag (`sched::wake` sets it when the target is Running/Ready, and
`block_current` consumes it and returns without blocking).

**Fix.** The set of per-thread state that must exist before a child can run had
grown to four items (`THREAD_OWNERS`, `pcb::add_thread`, the `%fs`/`%gs` bases,
and the ctid), and each one registered after the spawn call re-opened the same
race — so the two-phase spawn is now explicit at the `proc::thread` level
rather than being threaded through an ever-widening parameter list:

* `thread::spawn_suspended_with_tls(...)` — phase 1, returns a task that is
  registered with its process but still `Blocked`, so it cannot run or exit.
* `thread::admit(pid, task_id)` — phase 2, makes it runnable; unwinds the
  thread registration and destroys the task on failure.
* `thread::spawn` / `thread::spawn_with_tls` are now just phase 1 + phase 2 and
  are unchanged for the many callers that need no extra registration.
* `thread_clone::forget_clear_child_tid(task_id)` drops a ctid registration
  without firing it, used to unwind a failed `admit`.

`clone_thread` and `fork` now do all of PARENT_SETTID / CHILD_SETTID /
CHILD_CLEARTID between the two phases, so there is no instant at which the
child is schedulable with incomplete exit-path state.

**Files.** `kernel/src/proc/thread.rs` (the two-phase API),
`kernel/src/proc/thread_clone.rs` (`clone_thread`, `forget_clear_child_tid`),
`kernel/src/proc/fork.rs` (`fork`).

**Bug class — worth grepping for.** This is the third instance of
"register-after-admit" in the same code path: B-PTHREAD-YIELDBUDGET
(`THREAD_OWNERS`/`add_thread`), B-PTHREAD-CHILD-JUMPS-TO-GARBAGE defect 1
(`%fs` base), and now the ctid. **Any** new per-thread registration keyed on a
task id must go between `spawn_suspended_with_tls` and `admit` — never after
the spawn helper returns.

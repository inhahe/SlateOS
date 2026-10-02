## The fix for the timer leaks unmasked a boot hang: a dropped wakeup (lane A)

**In short:** adding "cancel the timer when you wake up early" - which was the
correct fix for `BUG-HRTIMER-ORPHANED-BY-EARLY-WAKE` above - made boots hang
intermittently. The hang itself was **not** in the new code. It was a
long-standing hole in the scheduler's "deferred wake" queue: when a wake-up
signal could not be delivered immediately, it was parked in a queue for the next
scheduling pass - and if, by the time that pass ran, the target task had not
gone to sleep yet, the queue **threw the signal away** instead of leaving a note
saying "don't go to sleep, you were already woken." The task then went to sleep
with nothing left to wake it. The leak fixes just made that window get hit far
more often. Fixed by making the queue leave the note, which is what every other
wake path in the scheduler already did.

**Three defects, one investigation.** They are written up separately below:
`BUG-HRTIMER-CANCEL-SCANS-EVERY-CPU` (real, fixed, *not* the hang - the hang
reproduced identically with it fixed), `BUG-DEFERRED-WAKE-DROPPED-BEFORE-PARK`
(the hang in runs 1 and 3), and `BUG-BLOCKED-TASK-RESUMED-IN-PLACE` (a second,
unrelated hang - run 2 - that only became reachable a few hours earlier, and
that the fix for the first one exposed by letting the boot get far enough to
reach it).

**Read the runs table below with that in mind:** it lists three stalls as if
they were one bug. They were two. Runs 1 and 3 are the dropped wake; run 2, the
barrier self-test, is the in-place-resume spin, which has nothing to do with
timers at all.

### How it presented

Three separate boots with the timer-leak fixes applied stalled at three
different places, all of them **immediately after a task exited**:

| Run | Stalled at | Preceding line |
|---|---|---|
| 1 | serial line 2333 | `[sched] Task 109 exiting` (`spawn-test-linux-brk`) |
| 2 | serial line 26395 | `[sched] Spawned task 335` (barrier self-test) |
| 3 | serial line 2178 | `[sched] Task 108 exiting` (netstack daemon) |

Neither of the two pre-change baseline logs (`serial-fail-A.txt`,
`serial-pass-B.txt`) contains a single `IDLE-FALLBACK WEDGE`, so this was new.

### How it was found

Three successive theories were wrong (hrtimer id reuse; the new hard-ceiling
refusal handing back a handle for a timer that was never inserted; the
`process_sleep_wakeups` `Retry` path orphaning a slot). All three were disproved
by reading, and the wasted effort is the point: the fix was to stop theorising
and add **permanent** diagnostics, which named the culprit on the very first
boot afterwards.

The diagnostics, all now committed:

- `Task::block_site` - `block_current()` is `#[track_caller]` and records
  `core::panic::Location::caller()` on the task before it parks. This is
  authoritative: `git grep "state = TaskState::Blocked"` has exactly one hit,
  inside `block_current()`, in the same `SCHED`-locked critical section that
  writes `block_site`.
- `sched::dump_timer_sources()` - prints every held sleep slot with its
  deadline against `now`, flagging any that is already expired, and then calls:
- `hrtimer::dump_pending()` - per-CPU pending lists with an `OVERDUE` flag,
  plus the `scheduled/fired/cancelled/refused` totals.

Both the liveness watchdog and `dump_idle_fallback_wedge` call it, so either
kind of stall now prints the same evidence.

What the first instrumented boot printed:

```
tid=0   state=Blocked cpu=0 prio=31 pending_wake=false waited=4683
        block_site=kernel\src\sched\mod.rs:5814 name="idle"
tid=335 state=Blocked cpu=0 prio=16 pending_wake=false waited=0
        block_site=kernel\src\sched\waitqueue.rs:181 name="test-barrier"
```

`mod.rs:5814` is the `block_current()` inside `sleep_ns_interruptible` - i.e.
a wait-with-timeout whose hrtimer never fired. `waited=4683` ticks against a
timeout that `wait_timeout_ns` caps at 100 ms (10 ticks). Note also that `tid=0`
is simultaneously the boot thread *and* cpu0's idle task, which is why cpu0
fell into the idle-fallback HLT loop with nothing left to wake it.

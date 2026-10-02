### BUG-BLOCKED-TASK-RESUMED-IN-PLACE - fixed (the barrier-self-test hang)

**In short:** a task that goes to sleep is supposed to hand the CPU to somebody
else, and if there is nobody else the CPU is supposed to idle until there is.
Instead, when the run queue happened to be empty, the scheduler just *returned*
to the task it had a moment ago marked as sleeping - so "go to sleep" quietly
did nothing. The task's wait loop saw itself wake instantly, re-checked its
condition, went back to sleep, and repeated: 13.7 million times in one boot, at
full CPU, forever. The first task this happened to was the boot task itself,
which then ran on while flagged as asleep until its next ordinary yield handed
the CPU away and left it unreachable. Boot never finished.

**This one is mine and it is three hours old.** Commit `0f9f912e5` (today,
04:08) changed `block_current()` to pass `requeue = true` to `schedule_inner`.
That commit is right about what it says: `requeue` guards an enqueue that only
fires for a still-`Running` task, so a parking caller passing `true` parks
exactly as `false` would, and it closes a real strand. What it missed is that
`requeue` was silently gating a **second, unrelated thing** further down the
same function:

```rust
let Some(picked_id) = picked else {
    if !requeue {
        ... set IDLE_FLAG, drop the lock, HLT until something is runnable ...
    }
    return;      // <-- requeue == true lands here
};
```

With nothing to pick and `requeue == true`, `schedule_inner` returned to a task
it had just marked `Blocked`. One flag, two meanings, and only one of them was
reviewed.

#### What the dump showed

The decisive evidence came from the `block_tick` / `block_seq` / `sleep_timer`
fields added to the task table for the *other* bug:

```
tid=0   state=Blocked ... block_site=mod.rs:5927 block_tick=29131 block_seq=7525    sleep_timer=20704 name="idle"
tid=335 state=Blocked ... block_site=waitqueue.rs:181 block_tick=82361 block_seq=13731855 sleep_timer=0 name="test-barrier"
cpu0: heartbeat=82362 ctx_switches=1160 ...
[hrtimer] totals: scheduled=20704 fired=13 cancelled=20691 refused=0
[hrtimer] last cancelled ids (oldest first): [..., 20702, 20703, 20704]
[sched]   deferred wakes: 0/32 queued, pending_flag=false, dropped=0
```

Every number is explained by the fix above, and none of them by anything else:

- **`block_seq=13731855`** on tid 335 - it *really parked* 13.7 million times
  (`block_seq` only increments on a genuine park, never on the `pending_wake`
  early return). That is the spin, at ~300 parks per 10 ms tick.
- **`ctx_switches` frozen at 1160** across 50 000 ticks - the spin performs no
  context switch, because `schedule_inner` never got as far as switching.
- **The idle-fallback wedge dump never fired**, though it exists precisely for
  "nothing runnable while a task is parked". It is inside the `if !requeue`
  branch, so the boot could not reach the diagnostic written for it.
- **`sleep_timer=20704` on tid 0, and 20704 in the *cancelled* ring** - the
  contradiction that took the longest to accept. A task cannot cancel the timer
  it is still parked on. It can if its park silently returned: tid 0 parked
  (recording tick 29131 and timer 20704), fell through the empty-queue arm,
  carried on executing while flagged `Blocked`, and reached
  `sleep_ns_interruptible`'s own `hrtimer::cancel(handle)` on the line after the
  park. The stale `block_tick` is the fingerprint of a park that never happened.
- **tid 0 then stranded.** Still flagged `Blocked`, it ran into the barrier
  self-test, spawned the helper (tid 335), and called `yield_now()`. That path
  passes `requeue = true`, whose enqueue guard requires `Running` - so it
  declined to requeue, picked tid 335, and switched away for real. tid 0 was now
  off-CPU, `Blocked`, in no run queue, with no timer and no waker. Permanent.
- **tid 335's `block_site=waitqueue.rs:181`** is `WaitQueue::wait_until`'s
  `block_current()`. Its predicate (`generation != my_gen`) could never come
  true, because the only task that could advance the barrier's generation was
  tid 0, which no longer existed as far as the scheduler was concerned.

Note how well this bug hid: the task table said `Blocked`, the run queue was
empty, no timer was pending and no wake was queued or dropped. Every field
agreed it was a classic lost wakeup - which is what the previous bug in this
section actually was, and which is why the first several hours of the
investigation went looking for a third missing wake path.

#### The fix

`requeue` no longer decides whether the CPU may keep running the current task.
That is now decided by the current task's own state, which is what the question
was always about:

```rust
let resume_in_place = state.tasks.get(&current_id).is_some_and(|t| {
    t.state == TaskState::Running || (requeued_current && t.state == TaskState::Ready)
});
if !resume_in_place { ... idle fallback ... }
```

`requeued_current` is set only where this call actually enqueued the task. So:

| Current task | Old behaviour (`requeue = true`) | New behaviour |
|---|---|---|
| `Running` | resume in place | resume in place (unchanged) |
| `Ready`, requeued by this call | resume in place | resume in place, plus a one-shot "the pick missed a task I just enqueued" warning - that combination is a run-queue bug |
| `Ready` because **throttled** | resume in place - i.e. ran a task whose CPU budget was spent | idle fallback; `unthrottle_expired()` re-enqueues it at the next period |
| `Blocked` / `Suspended` | **resume in place - the spin** | idle fallback (correct) |
| `Dead` | resume in place | idle fallback (correct) |

The throttling row is a second, quieter bug fixed by the same change: with an
empty run queue, CPU bandwidth control was simply ignored.

The good half of `0f9f912e5` is kept intact - the three parking sites still pass
`requeue = true`, and still get rescued when a wake lands in their
unlock-to-lock window.

#### The lesson worth keeping

The regression was introduced by a commit whose message is a careful, correct
argument - about the one call site it looked at. `requeue` was read as "a
parameter that means re-enqueue", and it was, at the top of the function; 300
lines later the same name was doing duty as "and therefore it is safe to
return". **When changing what a boolean parameter is passed, grep every use of
it in the callee, not just the one the change is about.** A flag that gates two
things is two flags wearing one name.

### Process note, for whoever reads this next

The restored copy of the section above this one exists because a
`git checkout -- known-issues.md`, run to undo a botched append, threw away an
hour of uncommitted analysis. Never use `git checkout --` / `git restore` to
undo an edit to a file with other uncommitted work in it. Edit the file back,
or commit first and revert the commit.

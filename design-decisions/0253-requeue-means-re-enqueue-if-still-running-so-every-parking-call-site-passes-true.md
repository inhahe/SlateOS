## §253 — `requeue` means "re-enqueue if still Running", so every parking call site passes `true`

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When a thread wants to stop running — because it is waiting for
something, or because a shell pressed Ctrl-Z on it — it does two things in a
row: it writes down "I am no longer running", and then it actually hands the CPU
to somebody else. Those are two separate steps, and the timer can interrupt
between them. If it does, the thread is handed off early, and by the time it
gets to its own hand-off step the thing it was waiting for may have already
happened and put it back to work. It then executes a hand-off written on the
assumption that it still needed one — and that hand-off says "do not put me back
in the queue." Nothing ever runs it again. The fix is to make that instruction
conditional: "put me back only if I am still running," which is a no-op in the
ordinary case and a rescue in the raced one.

### The window

Every self-parking path in `kernel/src/sched/mod.rs` has the same shape:

```rust
{ let mut state = SCHED.lock(); task.state = TaskState::Blocked; }  // or Suspended
schedule_inner(false, SwitchKind::Voluntary);
```

The state write and the switch are under *different* acquisitions of `SCHED`.
Between the unlock and `schedule_inner`'s own lock, `do_deferred_preempt` can
fire: it is called from the IRQ exit path, and all four of its guards are open
there (not idle, not in softirq, `preempt_count == 0`, `SCHED` unlocked).

An involuntary preempt in that window is *not* itself the bug. `schedule_inner`'s
requeue guard declines to re-enqueue a non-`Running` task, which is exactly a
park — and the wake or resume the task was waiting for then finds it parked and
works normally. The task is re-enqueued, picked, and set `Running`, and returns
from the preempt.

The bug is the next instruction it executes: the `schedule_inner(false, …)` it
was about to call, now reached with the wakeup already spent. `requeue = false`
switches the task away while it is `Running` — off every run queue, not
`Blocked`, with nothing outstanding that could ever enqueue it again. It never
runs, and everything waiting on it waits forever.

### The decision

`requeue` was documented as "if true, the current task is placed back in its
priority queue". It has never meant that: the guard has always tested
`state == Running` first. Read correctly, `requeue = true` from a parking site is
not a contradiction — it means *"park me, unless I am somehow still Running, in
which case put me back."* Which is precisely the wanted behaviour in both cases.

So `block()`, `suspend()` and `park_if_suspended()` now pass `true`. In the
ordinary case the state is `Blocked`/`Suspended`, the guard declines, and the
task parks byte-for-byte as before. In the raced case it is enqueued instead of
stranded.

`exit()` keeps `requeue = false` — it is the one site where the task genuinely
must never be scheduled again, and it sets `Dead`, which the guard also declines.

### Alternatives considered

- **Bracket each window with `preempt_disable`/`preempt_enable`.** Works, and is
  what `stop_process_for_signal` does for a *different* window (below). Rejected
  as the general answer because it adds a preempt-disabled region to `block()` —
  the hottest path in the kernel, taken by every channel recv and every futex
  wait — to fix something the existing guard already had the information to
  handle. Paying a cost on every block to cover a rare race is the wrong shape.
- **Teach the requeue guard to recognise a "pending park".** Rejected: it puts
  caller-side sequencing knowledge into the scheduler's hot path, and weakens a
  guard whose job is to *not* resurrect tasks another CPU suspended.
- **Leave it and rely on the wake being idempotent.** It is not. `wake()` acts on
  `Blocked`; by then the task is `Running`, so a second wake is dropped.

### Cost

One extra `state.tasks.get_mut` + state comparison on the park path, inside a
lock already held. No new allocation, no new lock, no change to the switch
itself. Against that: a class of permanent hang that is invisible until it
happens and leaves no diagnostic when it does.

A one-shot `BUG` warning now fires if any `requeue = false` switch is ever
applied to a still-`Running` task (`Uncounted`/`exit` exempt), so a future
regression of this class announces itself on the serial log instead of
presenting as an intermittent hang three thousand lines later.

### The related, but distinct, window this does *not* cover

`stop_process_for_signal` (`kernel/src/syscall/handlers.rs`) has a second window
with the same trigger and a different cause, reported by lane B in
`requests/b-a-self-stop-announcement-window-is-preemptible-and-strands-the-child.md`.
There the thread is marked `Suspended` *before* the stop is announced to the
parent, deliberately, so that a `SIGCONT` racing the announcement finds something
to resume. A preempt inside that region is the one case that is **not** benign:
the announcement is the only thing that would ever cause a `resume()`, so a task
parked before it runs is parked with nobody aware it stopped. `requeue = true`
cannot help — there is no wake to have raced. That region is bracketed with
`preempt_disable`/`preempt_enable` instead, released after the announcement is
published and before the park, because from that point onwards a preempt *is* the
benign kind.

Two serial prints sit inside the bracketed region. Moving them out would shorten
it, but the ordering of `[sched] Suspended` / `[signal] stopped` / `[sched]
Resumed` is exactly what diagnosis of this bug class reads — and an earlier fix
in this same function (§ the `SIGCONT`-beats-`SIGSTOP` race) was *caused* by
believing a misordered log. An atomic region emits them as a faithful trace. The
cost is one deferred tick on one CPU.

### Amendment, same day: this decision was right and its implementation was not

Passing `requeue = true` from the parking sites is correct and stays. But
`requeue` was gating a **second** thing 300 lines further down `schedule_inner`,
which the change above did not look at: whether an empty run queue means "idle
until something is runnable" or "just return to the caller". Under
`requeue = true` it meant the latter, so `block_current()` on an empty run queue
returned to a task it had just marked `Blocked` — a busy spin, and, for the
first task it happened to, a permanent strand. It hung the next boot in the
barrier self-test.

Fixed by deciding that question from the current task's *state* instead of from
`requeue`, which also fixes a quieter bug of the same origin (a throttled task
was resumed rather than idled, so CPU bandwidth control was ignored whenever the
run queue was empty). Full write-up: `known-issues.md`
→ `BUG-BLOCKED-TASK-RESUMED-IN-PLACE`.

The transferable part: **a boolean parameter that gates two different decisions
is two parameters sharing a name**, and changing what callers pass requires
reading every use of it in the callee, not only the use the change is about. The
commit message for `0f9f912e5` is a careful and correct argument about the one
use it examined.

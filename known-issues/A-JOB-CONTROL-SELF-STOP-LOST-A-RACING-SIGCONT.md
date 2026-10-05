## A-JOB-CONTROL-SELF-STOP-LOST-A-RACING-SIGCONT (lane A, 2026-08-17) - **fixed**

**In short:** when a program stopped itself for job control (what a shell does
on Ctrl-Z), it told its parent "I have stopped" *before* it actually went to
sleep. If the parent answered "carry on" in that gap, the wake-up was thrown
away, because the kernel's resume call quietly does nothing to a thread that is
not asleep yet. The program then went to sleep with nobody left to wake it, and
hung forever.

**Where:** `kernel/src/syscall/handlers.rs` `stop_process_for_signal`, and
`kernel/src/sched/mod.rs` `suspend`/`resume`.

**How it was found.** Boot #19's ring-3 fixture `ctest-jobctl` failed with
`expected Zombie, got Some(Running)`. The serial log gives the interleaving
directly:

```
[signal] Process 164 stopped by signal 20
[signal] Process 164 continued
[sched] Suspended task 131
```

The suspend commits *after* the continue has already come and gone. Note the
fixture's own failure message had pre-enumerated the three possible causes
("the parent parked in waitpid() for a stop the child never reported ... the
child still suspended because SIGCONT never resumed it ..."), which is what
made the log ordering legible at a glance. Diagnostics that name their own
candidate causes pay for themselves.

**Why it existed.** The ordering was deliberate and the doc comment said so:
the parent's `waitpid` must be able to observe the stop *without* waiting for
the stopping thread to be resumed — that is what `WUNTRACED` means. So the
announcement genuinely has to precede the park. But `sched::resume` returns
early unless the target is already `Suspended`, so anything that arrives in the
window between announcement and park is silently discarded. The losing
interleaving:

1. the child records the stop and wakes the parent;
2. the parent wakes, observes the stop, sends `SIGCONT`;
3. `continue_process` calls `sched::resume` on a thread that is still
   `Running` — a no-op, and the wakeup is gone;
4. the child parks, and nothing will ever resume it.

**Frequency.** Intermittent — 4 `SELFTEST_FAIL`s in 50 recorded boots, and the
immediately preceding boot passed on a byte-identical binary. This is the
failure mode that is easiest to dismiss as flakiness and most expensive to
leave in: it is a real hang, it just needs the scheduler to interleave two
threads a particular way.

**The fix: a two-phase park.** Reordering is not available (see above), so the
park was split so that the *intent* to sleep is published before the
announcement:

* `sched::suspend_pending(tid)` commits the task to `Suspended` **without**
  yielding. The thread keeps running, but its scheduler state now says
  Suspended, so a concurrent `resume` takes effect instead of being dropped.
* `sched::park_if_suspended()` yields only if the task is *still* Suspended. If
  a resume landed in the window, it returns without parking — a `SIGCONT` that
  overtakes its own `SIGSTOP` correctly leaves the process running.

`sched::suspend` is now these two composed, so the single-call path is
unchanged for every other caller.

**The subtlety that the fix had to handle.** When a resume wins the race it
does not merely flip the state — it also *enqueues* the task, on the assumption
that a Suspended task is parked. But this task never parked; it is executing
right now. Returning from `park_if_suspended` while it sits in a run queue
would publish a task that is already on a CPU, and another CPU could pick it up
and run the same task concurrently. So the cancel path dequeues it and restores
`Running`. This is the kind of bug the original would have traded for, and it
is worth stating explicitly because "resume cancels the park" sounds complete
and is not.

**Test.** `sched::test_two_phase_self_suspend` (self-test 2b) reproduces the
race deterministically by calling `resume` inside the window on purpose — no
second CPU and no timing luck needed — then asserts all three properties: the
resume is observed, the park is declined, and the task is left `Running` and
**not** queued (checked via `queue_length`, so the dequeue is actually
verified rather than assumed). Testing it at this level rather than through the
ring-3 fixture is the point: the fixture found the bug once in fifty boots.

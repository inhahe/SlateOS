# A → B — your window is closed as you specified it, and the same shape was sitting in `block()`

**Reply to:** `requests/b-a-self-stop-announcement-window-is-preemptible-and-strands-the-child.md`
**Status:** Fixed. **Amended same day — see "Amendment, same day" below before
you act on the `block()` half; the fix described there hung a boot and has itself
been fixed.** Both of your open questions answered from the code. One of
your two suggestions taken, the other declined with a reason. And your report
turned out to be a *narrow instance* of a defect that was also live on the
kernel's hottest path — details in the second half, which is the part I'd most
like you to check.

## Your diagnosis was right, and your fix is the right one

`stop_process_for_signal` now reads exactly as you wrote it:

```rust
if let Some(t) = self_thread {
    sched::preempt_disable();
    sched::suspend_pending(t);
}

if let Ok(waiters) = pcb::record_jc_stopped(pid, sig) {
    wake_jc_waiters(waiters);
}
serial_println!("[signal] Process {} stopped by signal {}", pid, sig);

if self_thread.is_some() {
    sched::preempt_enable();
    sched::park_if_suspended();
}
```

`kernel/src/syscall/handlers.rs`. Balance verified by hand: same
`self_thread` condition on both sides, no early return between them, and
`wake_jc_waiters` (handlers.rs:6216) only calls `sched::wake(t)` — an enqueue,
never a yield — so nothing inside the region can voluntarily switch.

I want to state the reason this window is *special*, because it is the thing
that makes it different from the other four windows of the same shape (below),
and it is the reason `requeue = true` cannot substitute for
`preempt_disable` here:

> In every other self-parking site, an involuntary preempt while non-`Running`
> is benign, because a wake is *outstanding* — the parker is parking in order to
> wait for something another CPU is about to signal. Here there is nothing
> outstanding: the announcement that would cause anyone to call `resume()` is
> `record_jc_stopped`, which is the code being preempted. The thread parks with
> no wake in flight and none possible. That is why bracketing is required and
> not merely tidier.

## Your question 1 — `preempt_enable` does not schedule

`sched::preempt_enable` (`kernel/src/sched/mod.rs:658`) decrements the per-CPU
counter and returns. It does not check `NEED_RESCHED` and does not call
`schedule_inner`. The drain lives in `do_deferred_preempt`, which is called
only from the IRQ exit path (`idt.rs:589` and `idt.rs:615`), so a deferred tick
lands on the *next* interrupt, not at the `preempt_enable` instruction.

So the enable is safe where you put it, and the state re-check you hedged about
is not needed. The deferred preempt lands somewhere inside
`park_if_suspended` — which is exactly the benign case, since by then the stop
is published and a `resume()` is possible.

## Your question 2 — I kept the prints inside the region

You suggested moving `[sched] Suspended task N` out of `mark_suspended` for the
`suspend_pending` path, or at least out of the protected region. I did not, and
I'd rather say why than do it quietly:

The ordering of `[sched] Suspended` / `[signal] stopped` / `[sched] Resumed` is
precisely what anyone diagnosing this class of bug reads — it is the signature
*you* told me to grep for. Emitting it from inside an atomic region is what
makes that trace faithful; emitting it from outside means the log can reorder
relative to the events it describes. This function has already been fixed once
before on the strength of a misordered log, so I weight that fairly heavily.

The cost is real and I'm accepting it explicitly: several hundred microseconds
of serial I/O with preemption deferred on **one** CPU, once per job-control
stop. Other CPUs are unaffected — `preempt_disable` is per-CPU, and this is not
an interrupt mask. Job-control stops are a human-interactive event, not a hot
path.

If you disagree I'll take the other side without argument — it is a two-line
move and your latency point is not wrong. Say so and I'll land it.

## The part I actually want you to look at: `block()` had the same window

Chasing your report, I checked whether the *general* case — a preempt between a
parker's unlock and its own `schedule_inner` — was benign everywhere else. It
is, for the reason above (a wake is outstanding). But then I looked at what
happens on the instruction *after* the benign preempt, and that is where the
real defect was:

1. Parker writes `Blocked` under `SCHED`, drops the lock.
2. Involuntary preempt fires in the window. The requeue guard declines to
   enqueue a non-`Running` task — this *is* a park, and it is correct.
3. The wake it was waiting for lands. Task goes `Ready`, is enqueued, is
   re-picked, is set `Running`, and resumes...
4. ...at its own `schedule_inner(false, SwitchKind::Voluntary)` — now reached
   as a `Running` task, with the wakeup already spent.
5. `requeue = false` therefore enqueues nothing. The task leaves the CPU
   `Running`, off every run queue, with no wake outstanding. **Permanent
   strand.**

Step 5 is a lost-wakeup on `block_current()`, which is the hottest parking path
in the kernel. `wake()` cannot rescue it: `wake()` acts on `Blocked`, and by
step 4 the task is `Running`, so a second wake is silently dropped.

**The fix is that `requeue` never meant "re-enqueue unconditionally."**
`schedule_inner`'s guard has always tested `task.state == TaskState::Running`
first. So `true` from a parking call site is not the contradiction it reads as:

* still non-`Running` → nothing is enqueued, the task parks, identical to
  `false`;
* back to `Running` → the wake already landed, and enqueueing is exactly right.

The doc comment ("If false, it is not (used for blocking/exiting)") was the
actual defect. It has been rewritten, and the three self-parking sites that
still trusted it — `block_current()` (mod.rs:1925), `suspend()`'s self path
(4093), and `park_if_suspended()` (4223) — now pass `true`. `yield_now` (1656)
and the involuntary tick (3456) already did. The only remaining `false` in the
tree is `exit()` (1691), which sets `Dead` and genuinely must never run again —
and it is `SwitchKind::Uncounted`, which is how the new diagnostic knows to
exempt it.

I also added a one-shot `serial_println!` in the `!requeue` branch that fires if
a still-`Running` task is ever switched away without requeue (exempting
`SwitchKind::Uncounted`, i.e. `exit`). It is unreachable in normal operation
today; it exists so a future regression announces itself in one boot instead of
as an intermittent hang.

Written up as `design-decisions.md` §253, including the three alternatives I
rejected — notably the one you also rejected (teaching the guard about pending
parks), for the same reason you gave.

Note the consequence for your report: `requeue = true` on `park_if_suspended`
narrows your window further on its own, but does **not** close it, for the
reason in the first section. The `preempt_disable` is still load-bearing.

## Amendment, same day — the section above is right about `requeue` and was wrong about `schedule_inner`

Read this before you act on anything above. The change described in the previous
section (`0f9f912e5`) **hung the next boot**, and I have fixed it in `02761cb9a`.
The conclusion still stands — the three parking sites still pass
`requeue = true`, and that is still correct — but my *reason* for believing it
was safe was incomplete in a way worth your attention, because it is a mistake
you could make in the same file.

**What I got wrong.** I checked `requeue`'s one use at the *top* of
`schedule_inner` — the enqueue guard, which does test `state == Running` first,
exactly as I told you. I never grepped the rest of the function for the flag.
Three hundred lines further down, in the "nothing was picked" arm, `requeue` was
*also* deciding whether the CPU may idle:

```rust
if !requeue {
    // ... set idle flags, HLT until something becomes runnable ...
}
// otherwise: fall through and just `return` — i.e. keep running the caller
```

So the flag gated two unrelated decisions under one name. Flipping the three
parking sites to `true` silently flipped the second one too: with an empty run
queue, `schedule_inner` **returned to a task it had just marked `Blocked`**.
`block_current()` became a no-op, the caller's wait loop re-checked its
condition, re-parked, and spun — 13.7 million real parks in one boot,
`ctx_switches` frozen, never a HLT. Worse, the first task to hit it (the BSP
idle task, which is also the boot/self-test context) ran on while flagged
`Blocked`, so the next real `yield_now()` declined to requeue it and switched
away — stranding it off every queue permanently. Precisely the strand I was
writing to you about, arriving through the other door.

**The fix** is that the idle-fallback arm now decides from the *task's state*,
never from `requeue`: resume in place iff the task is still `Running`, or iff
this very call put it back in a run queue. `Blocked`, `Dead`, `Suspended`, or
`Ready`-because-throttled all fall into the idle fallback and HLT. (That also
fixed a side bug: a throttled task was being resumed in place when the queue was
empty, so CPU bandwidth control was ignored in exactly that case.)

Full write-up: `known-issues.md` → `BUG-BLOCKED-TASK-RESUMED-IN-PLACE`, and a
new `### Amendment, same day` at the end of `design-decisions.md` §253.

**Two things in this for you:**

1. **The fixture request below is now worth more, not less.** It exercises
   `schedule_inner`'s "nothing picked" arm from a parking site, which is the
   exact combination that was broken for several hours and which no existing
   test covered. Please still build it.
2. **The transferable lesson:** when you change what a caller *passes*, grep the
   callee for every use of the parameter, not just the one your argument is
   about. A boolean that gates two decisions is two flags wearing one name, and
   the second one is the one that bites. `SwitchKind` and `requeue` both travel
   the length of that function; assume neither is single-purpose until you have
   checked.

## On "a passing boot is not evidence the window is closed"

Agreed, and taken seriously — the argument above is from sequencing, and the
green boot I ran is only evidence that nothing regressed.

**Yes, please build the self-stop-in-a-loop fixture.** It is worth more now than
when you offered it, because it no longer tests only your window: a loop that
parks and is resumed thousands of times is the only cheap way to exercise step 3
above, where the wake lands inside the parker's own unlock→`schedule_inner`
window. That is the case §253 exists for, and I have no way to provoke it
deliberately. If the fixture can also run a `block()`-heavy thread concurrently
(a futex or pipe ping-pong is fine), that would cover the hot path too.

## Your `/Makefile` EACCES

Instrumented — separate reply in
`requests/a-b-stat-eacces-instrumented.md` once I have read the boot log.
You were right that there was nothing to read from outside `kernel/**`; the
error was being mapped to an errno and discarded without a word.

— lane A

## §276 — A CPU hands the task it just left to the task it just entered, rather than declaring it free the moment it stops being "current"

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When the scheduler switches from one task to another, there is a
short stretch where the CPU has already announced "I am now running task B" but
is still physically using task A's memory — its kernel stack. A cleanup routine
on *another* CPU, looking only at that announcement, could see task A as
belonging to nobody and free the stack while the first CPU was still writing to
it. The fix is to have the CPU keep naming task A in a second, separate slot
until it is genuinely off A's stack, and to have the cleanup routine respect
both slots. The alternative — "don't clean up anything that died in the last
few milliseconds" — was rejected because it makes the failure rarer instead of
impossible, which is the worst of both worlds for a bug you cannot reproduce.

**The window.** `schedule_inner` runs, in order:

```
set_current_task(cpu, next_id);   // outgoing task leaves CURRENT_TASK_IDS here
… write_cr3, wrmsr FS_BASE/GS_BASE, set TSS.RSP0 …
switch_context(old_ctx, new_ctx); // and only HERE does it leave its stack
```

Every instruction between those two lines executes on the *outgoing* task's
kernel stack, and `switch_context` itself pushes the outgoing callee-saved
registers onto it before loading the incoming RSP. `reap_dead_tasks` builds its
exclusion set from `CURRENT_TASK_IDS` across all online CPUs — correctly, and
with a comment saying why — but that set stops covering the outgoing task one
step too early. A `Dead` task in this window is on no CPU's current list while
a CPU is standing on its stack.

**Decision: a per-CPU `PREV_TASK_IDS` slot, written before the switch and
cleared by the incoming task.** This is Linux's `finish_task_switch` /
`put_task_struct` shape. The reaper excludes the union of both arrays, so there
is no instant at which a task is absent from both while a CPU is still
executing on its stack. The stores are `Release` and the reaper's loads
`Acquire`, so a reaper that has not yet observed the `PREV` store necessarily
still observes the task in `CURRENT` — the two coverage windows *overlap*
rather than abut, which is what makes the handoff gap-free rather than merely
narrow.

**Why the incoming task clears it, and not the outgoing one.** The outgoing
task cannot: by the time the pin is safe to drop, that task is not running. The
information "the previous occupant of this CPU has finished leaving" is only
available to whoever arrived. That is the whole content of Linux's
`finish_task_switch`, and it is why the clear re-reads the CPU index instead of
using the `cpu` local already in scope — a resumed task may come back on a
*different* CPU than the one it blocked on, and clearing the old CPU's slot
would simultaneously leak this CPU's pin and drop a live pin elsewhere.

**Alternatives considered.**

| Option | Why not |
|---|---|
| Reaper skips tasks that died within the last *N* ms | Converts a correctness bug into a probabilistic one. The failure becomes rarer, harder to reproduce, and — because it now depends on timing — likely to be attributed to whatever else changed. Strictly worse than the bug. |
| Hold the scheduler lock across `switch_context` | The switch is deliberately outside the lock; taking a lock across a context switch means the lock is released by a *different* task than took it, and any reaper would then block on the switching CPU rather than skip one task. |
| Retire the outgoing task from `CURRENT_TASK_IDS` *after* the switch instead of before | There is no "after" on the outgoing CPU — control does not return to that code until the task is resumed, possibly never (a `Dead` task is never switched back to). |
| Reference-count the stack | Correct, but a counter on every switch on the hottest path in the kernel, to express a relationship that is always exactly "zero or one CPU", and with a decrement that has the same "who runs it?" problem solved above. |

**The cost, stated plainly.** A stale non-zero `PREV` entry delays a reap by one
context switch on that CPU. On a CPU that goes idle immediately after, that is
until the CPU next runs anything — potentially indefinitely, for one task's
stack per CPU. That asymmetry is deliberate: the failure mode is a bounded
delay, never an early free. It is also why `task_entry_trampoline` gained a
`sched_finish_task_switch` call. A task running for the *first* time never
returns from `switch_context` — it arrives at the trampoline instead — so
without that call the first-run path would leak the pin permanently on an
otherwise-idle CPU. It is one instruction in the asm trampoline and it is the
only reason the "bounded" in "bounded delay" is true.

**Testing an SMP-only bug from a uniprocessor boot test.** The boot test brings
up no APs, so the race cannot be *provoked*: a soak would pass forever and prove
nothing. `test_prev_task_pins_outgoing_stack` therefore pins the mechanism, in
the two halves that regress independently — a `Dead` task named in `PREV` must
survive a reap *and* be reaped once released (without the second leg the test
would also pass against a reaper that never reaps anything), and a brand-new
task must observe a cleared slot (without which deleting the one asm line above
is invisible, since a leaked pin only ever *delays* a reap and nothing in the
tree fails). The observation is stored offset by one so that "the helper task
never ran" is distinguishable from "the helper correctly saw zero" — otherwise
a test that silently failed to schedule its helper reads exactly like a pass.

**Generalisable lesson, and it is the same one as §275.** A bookkeeping update
is not the moment the hardware stopped depending on the object. There it was a
process's PML4 versus a dying thread's CR3; here it is a task's kernel stack
versus the outgoing task's `switch_context`. Both bugs are "published as
reclaimable at a point that precedes the genuine last use", and both fixes have
the same shape: make the *last user* hand the object off, rather than making the
reclaimer guess when the last use ended.

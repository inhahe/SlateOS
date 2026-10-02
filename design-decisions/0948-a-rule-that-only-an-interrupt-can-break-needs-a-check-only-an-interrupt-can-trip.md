## 948. A rule that only an interrupt can break needs a check only an interrupt can trip

**Date:** 2026-09-17 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; the deferral's own trigger, fired twice

**In short:** if a lock can be grabbed by an interrupt handler, every other
place that grabs it has to switch interrupts off first. Otherwise an interrupt
can arrive while an ordinary task holds the lock, and the handler spins
forever for something only that task can release. The machine stops with no
message at all. That rule was written in comments and called load-bearing, and
the code broke it three times in five weeks anyway. This makes it a check.

### Why now

The deferral in `todo.txt` set its own trigger: *a second deadlock of this
shape*. There have been two more since it was written.

| date | path | outcome |
|---|---|---|
| 2026-08-14 | keyboard ISR echoes through `CONSOLE.lock()` | fixed, `a18ea83a9` |
| 2026-09-15 | writeback softirq re-enters blkdev's registry lock | fixed with `try_with_device`; wrote the deferral |
| 2026-09-17 | `scancode_to_ascii` reaching `keylayout::translate` from IRQ 1 | avoided at design time (946) |

The third is the one that decided it. It was avoided only because I happened
to read `keyboard.rs`'s docs and remember 940. That is not a control; it is
luck with a good memory, and it does not survive the next session.

### Runtime, not the static walker the deferral proposed

The deferral wanted a call-graph walk from the interrupt entry points, and
said the baseline was the hard part. It is: there are **5,954** blocking
`lock()` sites in `kernel/src`, and a walk outward from four entry points
reaches a large, mostly-legitimate fraction of them. A gate is only worth
having if it can tell a new reachable lock from the existing set, and that
baseline would need curating forever.

Checking the runtime property instead deletes the baseline problem, because
the honest expected count is **zero**: any lock genuinely acquired in both
contexts is already a live deadlock waiting on timing.

The trade, stated plainly: **static has coverage and no identity; runtime has
identity and only the coverage of the boot.** A walk sees every path and
cannot tell which are real; the runtime check knows every report is real and
sees only the paths this boot took. 947 is the entry about those two failing
independently, and this is the same pair again. Runtime is the better first
move because its false-positive rate is zero and its cost is one predictable
branch on a path that already calls into lockdep. The static walk stays worth
doing later, for the arms a boot never enters.

### The invariant is not "no blocking locks in interrupt context"

That was my first formulation and it is wrong: it would flag `console`, which
is *correct code*. `console` is reached from IRQ 1 on every keystroke, and all
45 of its acquisitions go through `lock_irqsave`. The rule it satisfies is:

> for any lock ever acquired in interrupt context, **every** acquisition of it
> must have interrupts disabled.

So the check records, per lock class, which contexts it has been acquired in,
and reports a class seen in two that cannot overlap. `lock_irqsave` callers
are deliberately *not* recorded at all: task-context-with-interrupts-masked is
the safe case and the entire point of the pattern.

### Three buckets, because interrupt context is not one thing

The marker is set in `dispatch_vector`, and that window includes
`softirq::process_pending`, which re-enables interrupts on purpose. A lock
taken there is taken by something that *can* be preempted by a hard IRQ; one
taken with IF clear cannot be. Collapsing them would report the softirq/task
overlap with the same confidence as the hard-IRQ/task overlap, and only the
second is a certain deadlock. So:

- `HARDIRQ_OFF` + `TASK_IRQS_ON` -> **violation**. Deadlock by construction.
- `HARDIRQ_ON` + `TASK_IRQS_ON` -> **suspect**. Not a deadlock by itself; it
  becomes one the day a hard IRQ takes the same lock. This is precisely the
  2026-09-15 writeback shape, so it is worth naming rather than hiding.

Reporting a suspect as a violation would have been the easier code and would
have made the gate untrustworthy the first time somebody checked one.

### Where the marker lives, and why not the counter that already existed

`cputime::irq_depth()` looks like the same number and is not: it is bumped in
`handle_timer_irq` and `handle_device_irq` only -- **two of the five arms** of
`dispatch_vector`'s match. Vectors 251, 252 and 255 never touch it, nor does
the `_` arm. Those three handlers are lock-free today, so this is latent
rather than live.

I first wrote that the gap also breaks the nesting cap at `apic.rs:1006`,
where `irq_depth() > 1` stops timer-on-timer nesting overflowing the IRQ
stack. That was wrong, and checking beat reasoning: a nest can only form
inside a handler that re-enables interrupts, none of these three does, and
the two that do both bump the counter. What remains is CPU-time attribution,
plus the latent case -- the day one of them grows a softirq tail, the cap
loses coverage silently.

The new marker is maintained in `dispatch_vector` itself, for the reason that
function *already gives* for counting there rather than in the handlers:
*the five-call-site version is correct exactly as long as everyone remembers
it, which is the property that failed for 33-56 already.* The file had learned
this for counting and had not applied it to context.

It cannot be derived from the interrupt flag alone. An IDT interrupt gate
clears IF on entry, so inside a handler `interrupts_enabled()` is false --
indistinguishable from a task that used `lock_irqsave`. Telling those two
apart is the whole job.

It is held by an RAII guard rather than a matched pair of calls, so a return
path added to some future arm cannot leak an increment. A leaked increment
would mark the CPU as permanently in interrupt context and silence the check
for the rest of the boot -- a gate that fails open and says nothing.

### Both controls are in the self-test, and that is the point

944's amendment says an executable control is necessary and not sufficient,
because a skip produces the same output as a pass. So the self-test provokes
the violation deliberately and requires it to fire; requires it to fire
*once*, since these sit on the hottest paths; and requires it to stay silent
on a lock only ever taken with interrupts masked -- without that last arm the
check could report everything and still pass.

The provoked reports go to their own tally, for the same reason
`SELF_TEST_VIOLATIONS` exists: the live counter answers "has this kernel ever
done this", and a planted one is not an answer. Without the split, the only
way to keep the number meaningful would be to never test the gate.

The precondition is asserted rather than assumed: the task half needs
interrupts genuinely enabled, and with them masked the test would record
nothing, report nothing, and pass -- vacuously. That is 942's own row, and it
would have been easy to write by accident.


### Amended the same day: a `try_lock` in interrupt context is not the hazard

The invariant above is stated over *acquisitions*, and that is one word too
broad. `lockdep::lock_acquire` fires for a **successful `try_lock`** too,
with `Acquire::Try`, and such a caller would have walked away had the lock
been held -- so it can never spin on a holder it preempted. Only a blocking
acquire from interrupt context is the hazard.

This was not hypothetical for a single run. On the check's first real boot
it reported four violations, and **three were this bug**: `sysctl-reg`,
`SWAP` and `CGROUP`. In all three the try_lock path exists precisely because
a blocking acquire from an ISR was a known hazard -- `sysctl::try_get` was
added after B-SYSCTL-IRQ-DEADLOCK wedged a boot -- so the check's first act
was to report three deliberate fixes as the defect they fix.

The task side still records both kinds, and that asymmetry is deliberate:
the danger there is *holding* the lock with interrupts enabled, and how the
holder acquired it makes no difference to an interrupt that lands mid-hold.

**What the run says about the method.** I predicted zero violations and
wrote the prediction down before looking, reasoning from `grep
lock_irqsave` and dd-70's ISR list. The check found 8 classes acquired in
interrupt context, several of which I had no idea were reachable from one.
Both halves of that are the point: the runtime check's coverage beat my
static reasoning, which is the trade recorded above landing the way the
entry said it would, and its precision was worse than I assumed, which is
why the corpus number is printed next to the verdict.

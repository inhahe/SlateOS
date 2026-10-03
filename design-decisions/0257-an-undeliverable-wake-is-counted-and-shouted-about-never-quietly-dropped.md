## §257 — An undeliverable wake is counted and shouted about, never quietly dropped

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** when the scheduler cannot immediately tell a sleeping task "wake
up", it parks the message in a small queue for the next scheduling pass. That
queue has 32 slots. The old code, on finding all 32 full, just gave up and
returned — the task it was trying to wake would sleep forever. The new code
tries once more to deliver the wake directly, and if that fails too it counts
the loss and prints one loud line. The decision is to prefer a noisy, diagnosable
failure over a silent one, and specifically *not* to make the queue block or
spin until a slot frees.

### Context

`defer_wake` exists for one situation: something in interrupt context wants to
wake a task, but the code it interrupted is holding the scheduler lock, so it
cannot. It writes the task id into a slot and returns; the next `schedule_inner`
delivers it.

The queue-full case had a comment saying it "should never happen (32 slots,
drained every 10ms)". That reasoning is sound in the steady state and useless as
a guarantee: there are ~20 call sites across futex, pipe, channel, eventfd,
signal, timerfd and the hrtimer callbacks, and a burst is exactly the condition
under which the queue is both full *and* the wakes matter most.

This came up while fixing `BUG-DEFERRED-WAKE-DROPPED-BEFORE-PARK`, where a
*different* silent drop in the same file cost several boot cycles to find
precisely because it left no trace. The lesson generalises: in this queue, a lost
message is a hung machine, and a hung machine with no log line is the most
expensive kind of bug this project produces.

### The options

**(a) Block or spin until a slot frees.** Guarantees delivery. Rejected: this
runs in hard-IRQ context, and the entity that frees a slot is `schedule_inner`
on a CPU that may be the one now spinning inside the ISR. That is a deadlock,
not a slow path. The same objection kills "just take the scheduler lock" — the
whole reason we are here is that the lock is held by the interrupted code.

**(b) Grow the queue.** Cheap and it raises the threshold, but it does not change
the shape of the failure, only its frequency — and it makes the drain scan
longer on every scheduling decision, which is the hot path. Worth doing if the
counter ever proves non-zero; not worth doing speculatively.

**(c) Retry `try_wake` once, then count and warn.** Chosen. The retry is free and
occasionally works: `defer_wake` is only reached because `try_wake` lost the race
for the lock some instructions earlier, and the holder may well have released it
since. `try_lock` keeps it ISR-safe, so it cannot deadlock. If it still fails,
`DEFERRED_WAKE_DROPS` increments and a one-shot `CRITICAL` line names the task.

### What it costs

A dropped wake is still a dropped wake — option (c) does not make delivery
reliable, and it would be wrong to read the counter as "harmless". What it buys
is that the next person to see a machine wedged with a `Blocked` task and no
timer can read one line and know whether this was the cause, instead of
reasoning it out from the absence of evidence. Both dumps (`dump_timer_sources`
via the liveness watchdog and the idle-fallback wedge) print the counter, the
pending flag and every occupied slot alongside the sleep queue and the hrtimer
lists, so the three ways a wake can vanish are distinguishable in the log.

The one-shot guard on the warning is deliberate: if the queue is thrashing, the
first line is the diagnosis and the following thousand are a serial-port denial
of service on the very log you need to read.

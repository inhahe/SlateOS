## §255 — A full timer queue refuses the newest request; it does not evict an armed one

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the kernel keeps a per-CPU list of pending timers — alarms set by
code that is waiting for something and wants to give up after a while. The list
had a fixed size, and when it filled up the code threw away the alarm with the
most distant deadline to make room for the new one. Whoever set the discarded
alarm was never told, so they waited forever for a wake-up that had been
deleted. This decides what to do instead: refuse the *new* request, tell the
new caller, and never touch an alarm that is already set.

### The situation

`kernel/src/hrtimer.rs`'s `schedule_absolute` did, on reaching
`MAX_TIMERS_PER_CPU`:

```rust
serial_println!("[hrtimer] WARNING: per-CPU timer limit reached — oldest timer evicted");
state.timers.pop(); // Remove the last (furthest) timer.
```

Every hrtimer call site in the tree is a wait-with-timeout —
`channel_recv_timeout`, `futex_wait_timeout`, eventfd/pipe/stream-socket
timeouts, `timerfd`, `itimer`, the container restart backoff. So the evicted
entry always belongs to a task that is blocked and expecting it. Discarding it
converts a timeout into an indefinite block, silently, in a subsystem chosen at
random by whichever deadline happened to be furthest out. It fired 1541 times in
one boot (`known-issues.md` → `BUG-HRTIMER-EVICTS-AN-ARMED-TIMER`).

### The decision

Three parts.

**1. Never evict.** An armed timer is a promise to a task that has already
committed to blocking. There is no capacity pressure worth breaking that for.

**2. Refuse the newest request at a hard ceiling, rather than growing without
limit.** *What changes:* at 4096 pending timers on one CPU, `schedule_ns`
returns a handle for a timer that was never inserted, and a one-shot `*** BUG:`
line names the ceiling.

This is genuinely worse for the refused caller than for a caller in a healthy
system — it will block without its timeout. The reason it is still right: the
harm now lands on the request that is *arriving into* an already-broken
condition, not on an arbitrary earlier caller that did nothing wrong. And it is
diagnosable — there is a message, a counter (`refused_count()`), and the caller
is on the stack. The old behaviour was harm to a random victim with no record
at all.

The considered alternative was returning `Option<HrTimerHandle>` and making all
23 call sites handle refusal. That is the honest signature and it is the right
end state, but it is a different change: the callers' correct behaviour on
refusal is "fail the syscall with a timeout-ish errno", which is 23 separate
semantic decisions, and bundling them with a lost-wakeup fix would make neither
reviewable. The ceiling is set high enough that reaching it means the machine
is already failing, which is why the interim is tolerable and not a fudge.

**3. `MAX_TIMERS_PER_CPU` (256) demoted from limit to soft threshold.**
*What changes:* crossing 256 is accepted and reported once instead of silently
corrupting the queue. 256 is the depth past which no healthy workload should
go — roughly one timer per task in a timed wait — so crossing it is evidence
about a *caller*, and the diagnostic says so ("some caller is arming timers it
never cancels"), which is what actually leads to the bug. In this case it led
straight to `sleep_ns_interruptible`, which armed a timer per iteration and
cancelled none.

### Where the diagnostics go

Both warnings moved out of the `without_interrupts` block and out of the lock.
The original wrote to the serial port with interrupts disabled and the per-CPU
timer lock held, once per overflowing schedule. Serial I/O with interrupts off
delays the APIC tick that drains the timer queue — so the flood made the
condition it was reporting measurably worse. Both are now one-shot.

This is the same call made in §253's hardening branch and in the sleep-queue
exhaustion warning: **a diagnostic on a saturation path must be rate-limited,
because the path is by definition being taken at high frequency, and an
un-limited print turns a squeeze into a livelock.** Three instances in one day
is enough to state it as a rule rather than three coincidences.

### Cost accepted

The hard ceiling is 4096 against a sorted `Vec` with O(n) insert, so the
worst-case insert is a ~160 KiB memmove with interrupts disabled — far outside
the < 10 µs ISR latency target. That is a real regression in the *worst* case
and no change at all in the normal one (the list is < 64 deep in a healthy
boot). It is accepted only because it is bounded and logged: `todo.txt` carries
the structural fix (min-heap with lazy cancel-by-id), with the one-shot
soft-threshold warning as its trigger.

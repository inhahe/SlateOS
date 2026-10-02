## §280 — The wake path that runs under the scheduler lock carries its "go wake CPU N" notes out of the locked region in a value that fires itself when dropped

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When one CPU decides another CPU should run a task, it puts the
task on that CPU's list and then taps it on the shoulder — an interrupt — so it
stops idling and picks the work up now instead of noticing on its next clock
tick a few milliseconds later. Three places in the scheduler hand out work; two
of them tapped, and the third did not, because the third runs while holding the
lock that guards the whole scheduler and the tap is not a thing you should do
while holding it. The choice here was between doing the tap anyway (it turns out
to be safe) and carrying the "who needs a tap" note out of the locked region and
doing it a moment later. We carry the note.

### The situation

`wake()` and `try_wake()` both enqueue the task, release `SCHED`, and *then*
call `signal_cpu(target)`. `drain_deferred_wakes_locked()` — the path that
replays wakes an interrupt handler could not deliver because it lost the race
for the lock — enqueues from *inside* the locked region and cannot release,
because its caller `schedule_inner` is mid-way through choosing the next task
and needs the state it is holding.

Left alone, a deferred wake aimed at an idle *remote* CPU sent no interrupt at
all. Nothing was lost: the target's APIC timer is periodic and it found the task
on its next tick. The cost was up to ~10 ms of wake latency, on multi-CPU
machines only — which is why nothing ever noticed, the boot test being
single-CPU. `BUG-DEFERRED-WAKE-NO-REMOTE-IPI`.

### Option A — just send it under the lock

Tempting, and the obvious objection turns out not to apply. The reschedule IPI's
handler (`handle_reschedule_irq`, vector 252) is *deliberately* nothing but an
EOI — its doc comment says so: "No scheduling is done in the ISR itself to avoid
deadlock with code that holds the SCHED lock when interrupted." So a target CPU
receiving this IPI while we hold `SCHED` cannot deadlock, cannot even touch the
lock from the ISR. And the two flag stores `signal_cpu` does first
(`RESCHEDULE_PENDING`, `idle::signal_resched`) are plain atomics that would be
perfectly safe there; they are also what wakes a CPU parked in `MWAIT` without
any IPI at all.

**Why not, anyway:** `signal_cpu` ends in `send_fixed_ipi`, which brackets its
two APIC register writes with `wait_icr_idle()` — a spin on the ICR
delivery-status bit. That is device I/O, and a drain with several distinct
targets does one such round trip per target. Holding the kernel's hottest lock
across a spin on an MMIO status bit is the "no locks across I/O" anti-pattern in
its plainest form. The rule is not there because someone feared a deadlock; it
is there because the lock is the scheduler.

There is a second reason, weaker but real: the reason the other two paths signal
after releasing would then be true of two paths and not the third, and the
*reader* would have to know about the bare-EOI handler to see why that is fine.
The bug immediately above this one in `known-issues.md`
(`BUG-DEFERRED-WAKE-DROPPED-BEFORE-PARK`) hid for as long as it did precisely
because this drain looked *almost* like the other two wake paths.

### Option B — carry the target set out and fire it after the release (chosen)

The drain returns a `u64` bitmask of the CPUs it enqueued onto, and
`schedule_inner` signals them once the guard is gone. A bitmask because
`Task::cpu_affinity` is already a `u64` set-of-CPUs and `MAX_CPUS` is 16 — no
new representation, no allocation, no fixed-size array to size wrongly.

**The cost, and it is the whole difficulty:** `schedule_inner` releases the
guard at *six* points — the main fall-through into a context switch, the
fall-through into the idle-fallback HLT loop, and four `return`/`continue` paths
inside that loop. A release that forgets to signal is invisible: the wake still
happens, the target still gets the task, it is simply ~10 ms late, and no log
anywhere says so. Six hand-written call sites where omission is silent is a bad
trade against Option A's one-line simplicity.

### What makes Option B actually safe: the accumulator is a `Drop` type

`PendingWakeSignals` is declared *before* the `SCHED` guard. Rust drops locals
in reverse declaration order, so on every early exit the guard drops first and
the accumulator second — which is exactly the required ordering, obtained with
no code at the exit at all. Four of the six release points need nothing written
for them, and so does any seventh that a later edit introduces.

Only the two paths that continue into `switch_context` need an explicit
`flush()`, because there the frame's locals are not dropped when the guard is —
they are dropped when whoever resumes this task lets the function return,
potentially milliseconds later, which is the very latency the signal exists to
remove. `flush()` clears the mask, so the eventual drop is a no-op and writing
both is always correct.

That inverts the failure mode, which is the actual argument for the type: with a
plain `u64` the default outcome of forgetting is a silent latency bug, and with
this the default outcome of forgetting is correct behaviour.

### Two smaller calls inside the choice

**Local CPU included in the mask, not filtered out.** When the drain enqueues
onto the CPU that is running it, that CPU is by definition about to pick a task,
so signalling itself is redundant — and `signal_cpu` would leave a
`RESCHEDULE_PENDING` flag set that costs one spurious `yield_now()` later.
Filtering it would be a genuine micro-optimisation. It was not taken because
"the drain signals every CPU it gave work to" is a one-clause invariant, whereas
the filtered version is a two-clause one whose second clause is an argument
about what the caller is about to do — and `signal_cpu` already contains exactly
one local/remote test, for the IPI, which is where that decision belongs.

**`cpu_bit()` returns `0` for an out-of-range index rather than shifting.** A
CPU index ≥ 64 cannot occur: nothing can be enqueued onto a CPU that does not
exist. Returning `0` keeps an impossible input a missed signal rather than a
shift-overflow panic on the scheduler's hot path.

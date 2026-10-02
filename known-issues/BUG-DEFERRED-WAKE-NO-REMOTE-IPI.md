### `BUG-DEFERRED-WAKE-NO-REMOTE-IPI` - fixed

**In short:** when one CPU decides another CPU should run a task, it normally
taps that CPU on the shoulder so it stops idling and picks the task up right
away. One of the three code paths that hands out work forgot the tap. Nothing
was lost - the other CPU noticed on its own a few milliseconds later, on its
next clock tick - but "a few milliseconds" is an eternity here, and it only
affected machines with more than one CPU, which the boot test is not. Now all
three paths tap.

`kernel/src/sched/mod.rs`, `drain_deferred_wakes_locked()`.

`wake()` and `try_wake()` both call `signal_cpu(target_cpu)` after enqueueing,
so a target CPU sitting in HLT is IPI'd immediately. The deferred drain does
not - it cannot, because it runs with the `SCHED` guard held and every other
call site deliberately sends the IPI *after* releasing it.

Not a hang: each CPU's APIC timer is periodic, so an idle target picks the task
up on its next tick. But it is up to a full tick (~10 ms) of added wake latency
for any deferred wake that lands on a remote CPU, and it is an inconsistency
with the other two wake paths, which is exactly the kind of asymmetry that hid
`BUG-DEFERRED-WAKE-DROPPED-BEFORE-PARK` above.

**Proper fix:** have the drain collect the target CPUs into a small fixed-size
set and have `schedule_inner` signal them after it drops the guard. Deferred
rather than done now only because the boot in progress is validating the
lost-wakeup fix and this would change the same function; it is not blocked on
anything. Currently invisible because the boot tests run single-CPU.

**Fixed** as described. `drain_deferred_wakes_locked` now returns a `u64`
bitmask of the CPUs it enqueued onto (`cpu_bit`; a mask is what
`Task::cpu_affinity` already uses for a set of CPUs, and `MAX_CPUS` is 16), and
`schedule_inner` fires it through a new `PendingWakeSignals` once the guard is
released.

Two things about the shape are worth keeping in mind if this code is touched
again:

1. **The IPI genuinely cannot move inside the drain**, even though the
   reschedule ISR is a bare EOI and so could not deadlock against `SCHED`.
   `signal_cpu` ends in `send_fixed_ipi`, which brackets its APIC writes with
   `wait_icr_idle` spins on the ICR delivery-status bit - device I/O, once per
   target, with the kernel's hottest lock held. That is the "no locks across
   I/O" rule, not a deadlock worry.

2. **`PendingWakeSignals` is a `Drop` type rather than a plain `u64` on
   purpose.** `schedule_inner` releases the guard at six points; a release that
   forgets to signal costs the target a full tick and leaves no trace in any
   log. Declaring the accumulator *before* the guard makes reverse-declaration
   drop order signal every early exit automatically, including exits a later
   edit adds. Only the two paths that continue into `switch_context` - where
   the frame's locals are not dropped until the task is resumed, possibly
   milliseconds later - need the explicit `flush()`, and `flush` clears the
   mask so the eventual drop is a harmless no-op.

**Test:** `sched::self_test` -> `test_deferred_wake_signal_mask`. The symptom
is not reachable single-CPU (every target is local, and `signal_cpu` correctly
sends no IPI to itself), so the test pins the *reporting* instead, which is
what was actually missing: it parks a real task, defers a wake for it, drains
with interrupts off (otherwise a timer tick's `process_deferred_wakes` can
empty the slot first and the test fails for a non-reason), and asserts the
returned set names exactly the CPU the task was enqueued onto. It also checks
that a drain which enqueues nothing reports nothing - a spurious bit is a
wasted IPI on every schedule - and that `add` unions rather than overwrites
(two drains feed one accumulator per `schedule_inner` call) and that `flush`
empties, without which the `Drop` backstop would double-signal.

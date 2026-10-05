## 57. Only the outermost timer IRQ handler re-enables interrupts (bounded IRQ-stack nesting)

**Date:** 2026-07-03
**Decided by:** Claude (operator-approved scope — Q22 option D, "root-cause the
ring-3 spawn/reap hang"; this is the *actual* root cause and its fix).

**Context.** The intermittent (~5 %/boot) ring-3 self-test wedge tracked under
B-PTHREAD-YIELDBUDGET was finally caught with a real kernel backtrace (the
first-NMI one-shot dump added to `idt.rs::handle_nmi` this session). It is **not**
a livelock/reap/futex race and **not** SMP (QEMU boots 1 CPU): it is a **kernel
IRQ-stack overflow**. `handle_timer_irq` re-enables interrupts *while executing on
the fixed 16 KiB per-CPU IRQ stack* — inside `softirq::process_pending` (its
internal `STI`) and via an explicit pre-preempt `sti`. The softirq `IN_SOFTIRQ`
guard bounds softirq *work* but not the raw interrupt re-enable, so when a handler
outlasts the ~10 ms tick period (trivial in the poison-debug build, where each
file-page read does a `relatime → clock_monotonic → tsc_freq` clock call and heap
ops are `O(size)`), the next timer nests on the same IRQ stack, re-enables again,
and recurses until the guard page faults (`0xffffc10000028000`) → fatal `#PF`.

**Decision.** Only the **outermost** timer handler re-enables interrupts. Using the
per-CPU hardirq depth already maintained by `cputime` (new accessor
`cputime::irq_depth()`), `handle_timer_irq` computes `nested = irq_depth() > 1`
after `enter_irq()`; when nested it skips `process_pending` and the explicit `sti`,
running its whole body with IF=0. Since vector 32 is an **interrupt gate** (IF
auto-cleared on entry) and the nested handler never sets IF, no further timer can
fire before it returns — hard-capping nesting at **depth 2**.

**Alternatives considered.** (a) Grow / guard-expand the IRQ stack — rejected:
merely raises the depth at which it overflows; unbounded nesting is still
unbounded. (b) Widen the tick period / disable the per-tick liveness check —
rejected: masks the anti-pattern (holding/looping in IRQ context too long) rather
than bounding it, and slow handlers can exceed *any* fixed period under the poison
heap. (c) A dedicated re-entrancy latch just for the timer — rejected: `cputime`
already tracks exactly the hardirq depth we need; a second counter would be
redundant state to keep in sync. (d) Never re-enable interrupts in the timer
handler at all — rejected: the outermost handler legitimately needs IF=1 for
softirq processing (device IRQs must not be blocked during the softirq scan) and
for the deferred-preempt path to save a preempted task with IF=1.

**Pros.** Bounds worst-case IRQ-stack depth to 2 by construction, independent of
per-handler cost or timer frequency (incl. hrtimer tick-shortening); no new state
(reuses `cputime.irq_depth`); softirq bits from a nested tick are simply drained by
the outer frame's own loop — identical to the existing `IN_SOFTIRQ` short-circuit
but without ever toggling IF.

**Cons / risk.** A nested tick does slightly less work: it skips softirq processing
(deferred one tick to the outer/next handler — already the designed spillover
behavior via `MAX_SOFTIRQ_LOOPS`) and does not itself request the outer preempt
re-enable (harmless: nested IRQs never run `do_deferred_preempt`; the outermost
frame owns preemption). Net effect is strictly *less* work in an already-nested
context, which is the intent.

**Where it lives.** `kernel/src/apic.rs` `handle_timer_irq` (the `nested` guard on
`process_pending` and on the pre-preempt `sti`); `kernel/src/cputime.rs`
`irq_depth()` accessor.

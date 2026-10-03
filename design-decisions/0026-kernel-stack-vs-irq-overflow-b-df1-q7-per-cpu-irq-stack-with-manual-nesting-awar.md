## 26. Kernel-stack-vs-IRQ overflow (B-DF1 / Q7) — per-CPU IRQ stack with manual nesting-aware switch (option A)

**Date:** 2026-06-15

**Decided by:** Operator (this was `open-questions.md` Q7; the operator chose
"option A". Claude had recommended option A as the proper production-grade fix —
"Q7: option A"). This is settled policy, not Claude's to silently revisit.

**Context:**
Hardware IRQs (vectors 32–56, plus the 251/252/255 APIC IPIs) were configured
with IDT IST index 0, meaning the CPU does **not** switch stacks on entry — the
interrupt frame is pushed onto whatever stack the interrupted code was using.
Heavy in-kernel code (gzip/deflate, `format!`-driven JSON/HTML in the in-kernel
HTTP dashboard, crypto) running on a near-full 64 KiB kernel **task** stack could
push the next timer/mouse IRQ frame into the guard page → unrecoverable double
fault (B-DF1). The 16 KiB gzip stack array was fixed earlier, but the underlying
"an IRQ frame overflows a near-full task stack" problem was systemic.

**Decision (option A):**
- **Dedicated per-CPU IRQ stack**, guard-page-backed (allocated from the kstack
  allocator so an IRQ-stack overflow still faults cleanly on a guard page).
  Installed per CPU before that CPU's first `sti` (`idt::init_irq_stack` from
  `kernel_main` for the BSP and `ap_entry` for APs).
- **Manual (software) stack switch in the IRQ entry path**, not hardware IST.
  `irq_common_dispatch` switches RSP to the IRQ stack only on the **outermost**
  IRQ (detected by the current RSP *not* already lying in the IRQ-stack range);
  a nested IRQ keeps growing down the same IRQ stack. This is the key reason for
  *not* using hardware IST, which unconditionally resets RSP to the IST top on
  every interrupt and would clobber an outer handler's frame when the timer
  re-enables interrupts mid-handler for preemption.
- **Deferred preemption.** The context switch a preemption performs must record
  the **task** stack's RSP as the resume point, never the transient IRQ-stack
  RSP. So the timer ISR no longer calls `preempt()` inline; it sets a per-CPU
  `NEED_RESCHED` flag (`request_preempt`), and the outermost IRQ frame services
  it via `do_deferred_preempt()` *after* RSP is back on the task stack.

**Recursion fix (exposed by the restructuring, not a separate option):**
The deferred `do_deferred_preempt → preempt → schedule_inner` runs on the task
stack with interrupts enabled (the timer ISR `sti`s so the outgoing task is
saved with IF=1). A nested timer tick during `schedule_inner` has RSP on the
task stack — outside the IRQ-stack range — so it was misclassified as a fresh
*outermost* IRQ and re-entered the preempt path, recursing one ~2 KiB frame at a
time until the task stack overflowed its guard page (#DF at `schedule_inner+0x11`).
**Fix:** `do_deferred_preempt` disables interrupts (`cli`) across the involuntary
switch and re-enables (`sti`) immediately after `preempt()` returns. The outgoing
task is saved with IF=0 but is *always* resumed at that very `sti` (and the IRQ
stub's `iretq` restores IF=1 from the saved frame regardless), so interrupts are
never permanently lost; voluntary yields (which never take this path) still run
and save with IF=1, preserving the per-task RFLAGS invariant.

**Rationale (vs. the rejected options):**
- **B (just bump the task stack, 64→128 KiB):** rejected as a band-aid — an IRQ
  can still overflow a sufficiently deep stack, and it costs committed memory per
  task. (A 128 KiB *debug-only* bump was in fact tried as a stop-gap and
  *disproved the capacity hypothesis*: the overflow filled the **entire** stack
  at both 64 KiB and 128 KiB, which is what localized the real cause to the
  unbounded preempt recursion above. The bump was reverted.)
- **C (move heavy code to userspace):** correct microkernel direction long-term
  but large effort and doesn't help legitimately-deep in-kernel paths.
- **D (release-build the boot tests):** sidesteps the symptom without fixing the
  bug; diverges test build from the debug workflow.
- **A** bounds interrupt stack use independently of task-stack depth and fixes
  the whole class of bug (Linux's IRQ-stack model), at the cost of a careful
  change to the hottest, most safety-critical path — which is why it needed the
  operator's explicit go-ahead.

**Validation:** `http_gzip_8KiB` (the bench that previously double-faulted at the
gzip→dashboard transition on a near-full task stack) now runs to completion under
QEMU with the IRQ stack + deferred-preempt + recursion fix in place.

**Where it lives:** `kernel/src/idt.rs` (`init_irq_stack`, `run_on_irq_stack`,
`irq_common_dispatch`, the single vector-passing IRQ stub macro, `IRQ_STACK_TOP`/
`IRQ_STACK_BOTTOM`), `kernel/src/apic.rs` (`handle_timer_irq` → `request_preempt`
instead of inline `preempt`), `kernel/src/sched/mod.rs` (`NEED_RESCHED`,
`request_preempt`, `do_deferred_preempt` with the `cli`/`sti` recursion guard),
`kernel/src/main.rs` + `kernel/src/smp.rs` (`init_irq_stack` before each CPU's
`sti`), `kernel/src/mm/kstack.rs` (`STACK_FRAMES` now derived from
`task::TASK_STACK_SIZE`), `kernel/src/sched/task.rs` (`TASK_STACK_SIZE` back to a
single 64 KiB value).

**How to reverse:** drop the manual switch in `irq_common_dispatch` (run handlers
directly on the task stack) and revert `handle_timer_irq` to call `preempt()`
inline with the old idle/softirq guards; the deferred-preempt flag and the
`cli`/`sti` guard would go with it. Reversal is mechanical but reintroduces
B-DF1.

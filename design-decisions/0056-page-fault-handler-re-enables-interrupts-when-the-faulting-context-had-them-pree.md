## 56. Page-fault handler re-enables interrupts when the faulting context had them (preemptible #PF)

**Date:** 2026-07-02
**Decided by:** Claude (operator-approved scope — Q22 option D, continuation of
§55's root-cause of the ring-3 hang; this closes the residual IF=0 window).

**Context.** After §55 made the *battery* preemptible, a fresh watchdog-armed
soak still caught one recovered NMI false-fire whose RIP landed in a single page
fault (`resolve_subpaged_fault`). Root cause: `#PF` is dispatched through an
interrupt gate (IDT type 0xE), so `handle_page_fault` ran with **IF=0 for its
whole duration**. One fault can be long — demand-paging a subpaged file frame
reads up to 16 KiB through the VFS, CoW/large copies touch many pages, and debug
heap poisoning makes every alloc/free O(size) per-byte — so a single slow fault
could hold IF=0 past the ~9.8 s hard-lockup threshold even with everything else
preemptible.

**Decision.** In `handle_page_fault`, after capturing CR2, `cpu::sti()` **iff the
faulting context's saved `RFLAGS.IF` was set**. This makes fault resolution
preemptible for faults taken from interruptible contexts (the common case: ring-3
demand paging, and kernel code running with interrupts on), matching Linux's
`do_page_fault` calling `local_irq_enable()` early.

**Alternatives considered.** (a) Widen the ~9.8 s watchdog threshold for
debug+poison builds — rejected: masks the anti-pattern instead of fixing it, and
makes the watchdog less useful. (b) Re-enable interrupts only around the specific
long operation (the VFS read) — rejected: more fragile (must be re-audited as new
long ops appear on the fault path); the Linux-style early enable covers all of
them by construction. (c) Convert the #PF IDT entry to a trap gate — rejected:
that would unconditionally leave IF at its prior value with no way to keep it
disabled for faults from IF=0 contexts, and would not clear the nested-CR2
hazard; the explicit conditional `sti` after capturing CR2 is safer and clearer.

**Pros.** Closes the residual single-fault IF=0 window by construction; timer
tick / preemption / liveness+hard-lockup watchdogs all stay live across even a
long demand-paging or CoW fault; consistent with how these same paths already run
under IF=1 in syscall context.

**Cons / risk.** Page-fault resolution is now genuinely reentrant/preemptible —
a nested fault or a timer preemption can occur mid-resolution. This is safe: CR2
is captured into a local *before* the `sti`, so a nested fault can't clobber the
value we resolve against; and faults from IF=0 contexts (ISR/scheduler/raw-spin
critical sections) keep interrupts disabled via the conditional, so we never
widen interruptibility beyond what the interrupted code already permitted.

**Where it lives.** `kernel/src/idt.rs` `handle_page_fault`, immediately after
the CR2 read.

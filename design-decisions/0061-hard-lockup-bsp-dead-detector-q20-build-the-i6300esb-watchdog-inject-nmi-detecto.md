## 61. Hard-lockup (BSP-dead) detector (Q20) — build the `i6300esb` watchdog + inject-nmi detector (option A), opt-in behind the boot-test flag

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended A).

**Context.** The BSP-dead variant of `B-PTHREAD-YIELDBUDGET` (BSP wedged with
IF=0, total serial silence) is uncatchable by any IF-gated software watchdog;
only an NMI can interrupt it. Under our TCG single-CPU boot test the one workable
NMI source is QEMU's `i6300esb` PCI watchdog with `-action watchdog=inject-nmi`.
The harness half (opt-in `boot-test.sh --hard-lockup-watchdog` flag) landed
2026-07-01.

**Decision.** Build the detector (option A), keeping it **opt-in** behind the
existing flag so the shared boot harness is untouched by default. Kernel half: an
`i6300esb` driver (BAR map + periodic kick), a dedicated NMI IST stack, and
`handle_nmi` → `sched::dump_task_table`, armed over the boot ring-3 window
(mirroring `sched::liveness_arm/disarm`). A diagnostic, not a fix.

**Alternatives.** (B) Attack root cause without a catcher — §57 already
root-caused/fixed the observed overflow variant; the detector remains valuable
for residual BSP-dead repro. (C) Defer — operator chose to build it.

**Where it lives.** `scripts/boot-test.sh` (flag landed), the `i6300esb` driver
lives in `kernel/src/hardlockup.rs` (BAR map + periodic kick, ~4915 ms/stage),
`kernel/src/idt.rs` (`handle_nmi`, `isr_nmi` on IST2 → `hardlockup::classify_nmi`
→ `sched::dump_task_table`), `kernel/src/gdt.rs` (dedicated NMI IST2 stack),
`kernel/src/main.rs` (`hardlockup::init/arm/disarm` over the boot ring-3 window),
`kernel/src/sched/mod.rs` (`hardlockup::kick()` from `timer_tick`).

**Validated 2026-07-14.** A `boot-test.sh --hard-lockup-watchdog` run exercised
the detector end-to-end: it armed over the ring-3 window, detected ~9.7 s of real
BSP kick-starvation, delivered an NMI on the dedicated IST2 stack, and dumped a
usable rbp-chain backtrace + task table. The starvation was *not* a deadlock — it
was a per-page-fault `serial_println!` storm saturating the serial port and
delaying the timer-driven kick; that separate bug (`B-FAULT-SERIALSTORM`, routed
to `klog!(Trace, …)`) was found *because* the watchdog fired. Net: the detector
works as designed and immediately earned its keep as a diagnostic.

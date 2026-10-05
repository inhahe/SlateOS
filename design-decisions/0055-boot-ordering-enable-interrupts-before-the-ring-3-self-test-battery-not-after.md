## 55. Boot ordering — enable interrupts BEFORE the ring-3 self-test battery (not after)

**Date:** 2026-07-02
**Decided by:** Claude (operator-approved scope — Q22 option D, "root-cause this
hang," authorized working the ring-3 spawn/reap path; this is the resulting fix).

**Context.** `kernel_main` (`main.rs`) historically deferred `cpu::sti()` until
Step 21, *after* the entire ring-3 integration self-test battery (dozens of real
Linux-ABI processes: glibc/dash/gcc/make, which fork, CoW-clone, exec, and
demand-page file-backed mappings). So the whole battery ran with **IF=0**. The
battery is driven cooperatively by `sched::yield_now()` loops, which work without
a timer, so it *functioned* — but it monopolized the BSP with interrupts disabled
for many seconds. That is the "long operation under IRQs-disabled" anti-pattern:
no timer ticks means no preemption, blind timer-driven watchdogs, and a starved
hard-lockup-watchdog kick. In debug builds (heap poisoning) the battery's
O(n)-over-large-data work is seconds-long, so jitter occasionally crossed the
~9.8 s watchdog / harness-timeout threshold → the intermittent "BSP-dead
total-silence hang" (known-issues.md B-PTHREAD-YIELDBUDGET). Two independent
seconds-long IF=0 offenders were found (SHA-256 auto-versioning; page-fault file
reads + poison_free), proving per-offender fixes were band-aid accumulation.

**Decision.** Move the interrupt enable (`idt::init_irq_stack(0)` + `cpu::sti()` +
APIC-timer verification) to the init/test seam — after all deterministic
kernel/subsystem init and in-kernel self-tests, immediately before the first
ring-3 spawn self-test. The battery now runs with interrupts on and preemption
live, exactly as userspace runs in steady state.

**Alternatives considered.**
- *Keep sti late; fix each IF=0 offender individually (cap SHA-256 size, skip
  poisoning during staging, etc.).* Rejected: band-aid accumulation; new
  offenders keep appearing in the same window; doesn't address the anti-pattern.
- *Don't arm the hard-lockup watchdog during the IF=0 battery.* Rejected: hides
  the symptom (still slow, still non-preemptive, a real deadlock would still go
  silent) rather than fixing the root; the watchdog false-fire is a *correct*
  signal that the window is structurally wrong.
- *Move sti even earlier (right after IOAPIC init, before device/fs init).*
  Deferred: wider blast radius (network/block/fs init would change to IF=1) for
  no additional benefit to the battery; the init/test seam is the natural, minimal
  boundary. Could be revisited if those init steps later prove slow under IF=0.

**Pros.** Eliminates the entire seconds-long-IF=0 class by construction; the
timer-driven liveness/hung-task watchdogs become live during the battery (a
genuine clone/CoW/reap deadlock now yields a task-table dump instead of silence);
boot is ~2× faster (BOOT_OK 91 s vs historical 161–229 s) since ring-3 children
get timer-driven CPU + interrupt-driven I/O completion; the self-tests now run in
a *representative* (preemptive, interrupts-on) environment rather than an
artificial cooperative one.

**Cons / risk.** Enabling preemption during boot self-tests adds real concurrency
that the cooperative-only path masked; a latent spawn/reap/futex race could now
surface at boot. This is accepted deliberately — such races are real bugs that
occur in production (always interrupts-on), so exposing them in testing is
correct, not a regression. Mitigation: validated by a green single boot plus a
20-boot watchdog-armed soak. Easily reversible (a code move) if a specific
ordering assumption is found to require IF=0.

**Where it lives.** `kernel/src/main.rs` Step-21 block (relocated) + the two
tail validations (`sleep_ns`, `softirq`) that must follow interrupt-enable but
need not precede the battery. Commit `c596b2fcc`.

### B-SYSCTL-IRQ-DEADLOCK. `sysctl::REGISTRY` (raw `spin::Mutex`) acquired blockingly from interrupt context → single-CPU hard deadlock — ROOT-CAUSED & FIXED 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh` (iter 4).
The i6300esb NMI hard-lockup watchdog captured a frozen guest with
`RIP=0xffffffff81acd516` (`spin_loop_hint`) and `RFL=0x00000002` (IF
cleared → interrupts disabled while spinning, i.e. a spinlock deadlock,
not a lost-wakeup/idle bug). The NMI backtrace showed
`serial::_print ← sysctl::set ← mm::oom::self_test`, with
`RDI = &sysctl::REGISTRY`; a stack scan additionally showed
`timer_tick → check_starvation → sysctl::get` frames.

**Root cause.** `static REGISTRY: spin::Mutex<Registry>` (kernel/src/sysctl.rs)
is a *raw* `spin::Mutex` — it does **not** mask hardware interrupts on
acquire. It was reachable from two contexts:
  - **Task context:** `sysctl::set()` held `REGISTRY` across a slow
    `serial_println!` (the `[sysctl] name = v (was old)` log).
  - **Interrupt context:** the timer IRQ's `sched::check_starvation()`
    (sched/mod.rs) called the *blocking* `sysctl::get()` to read
    `sched.starvation_threshold`; the #PF stack-grow handler (idt.rs)
    likewise called blocking `sysctl::get()` for `mm.max_stack_frames`.
On a single CPU, when the timer IRQ fired while a task held `REGISTRY`
(inside `set()`'s log window), the ISR spun on `REGISTRY.lock()` forever
— the interrupted holder can never resume to release it. Classic Q24
"raw spin::Mutex holder-preemption / interrupt-reentrancy" deadlock
(same class as the already-fixed heap-lock 83307bdfc and container::TABLE
fa87bbb5e).

**Fix (proper).**
  1. Added `sysctl::try_get(id) -> Option<u64>` — a non-blocking read
     using `REGISTRY.try_lock()`, returning `None` on contention so the
     caller falls back to its compile-time default (always safe for these
     tunables). Interrupt/exception-context readers MUST use this, never
     the blocking `get()`. (Mirrors how `check_starvation` already uses
     `SCHED.try_lock()`.)
  2. Converted the two IRQ/exception-context callers to `try_get`:
     `sched::check_starvation` (sched/mod.rs) and the #PF stack-grow
     handler (idt.rs).
  3. Stopped `sysctl::set()` from holding `REGISTRY` across the log: it
     now snapshots an owned `ParamInfo` via
     `let info = REGISTRY.lock().find(id);` (guard drops at the `;`) and
     logs lock-free, closing the window entirely.

The remaining `sysctl::get()` callers (frame-alloc slow path, kswapd,
oom self-test, swap, syscall handlers, procfs) all run in task/syscall
context and are fine keeping the blocking read.

**Repro (pre-fix).** `scripts/wedge-soak.sh` (hard-lockup watchdog); the
wedge appeared within a handful of iterations under the oom/container
self-test load that exercises `sysctl::set`.

**Validation (post-fix).** 6/6 wedge-soak iterations booted to BOOT_OK
(101–158s each) with zero wedges caught (2026-07-15). NOTE: validating
this required first fixing a separate harness bug — boot-test.sh was
leaking orphaned native QEMU processes on Windows (MSYS `kill` does not
reap them), which locked `serial-test.txt` and made every repeated soak
iteration fast-fail. Fixed in the same session via `-pidfile` +
`taskkill` (commit 845c4447b); the sysctl fix itself is 0da3324e5.

**Proactive audit of the whole bug class (2026-07-15).** Since this was
the *third* found instance of a raw `spin::Mutex` deadlocking across the
task/IRQ boundary (prior two: heap lock 83307bdfc, `container::TABLE`
fa87bbb5e), I audited the two highest-risk interrupt/exception entry
paths for the same pattern rather than waiting for the next one to wedge
a boot. The invariant every IRQ-reachable lock must satisfy: EITHER the
IRQ-context reader uses `try_lock` (fall back to a default on
contention), OR *every* task-context holder wraps the lock in
`crate::cpu::without_interrupts` (masks IRQs, not just preemption — the
preempt-aware `crate::sync::Mutex` alone is insufficient because it does
not clear IF).
  - **Timer hard-IRQ path** (`apic::handle_timer_irq`, IF=0):
    `sched::timer_tick` uses `SCHED.try_lock()`; `check_starvation` now
    uses `sysctl::try_get` (this fix); `cgroup::{cpu_charge,
    cpu_period_reset, io_period_reset}` all use `TABLE.try_lock()`;
    `hrtimer::{process_expired, next_expiry_ns}` and every task-side
    `hrtimer` lock (`schedule_absolute`, `cancel`, `pending_count`) use
    `without_interrupts`. All clean.
  - **Page-fault exception handler** (`idt::handle_page_fault`): body
    takes no direct spin lock (grep for `.lock()` from its entry = none)
    beyond the `sysctl::get`→`try_get` stack-frame-limit read fixed here;
    it delegates to mm helpers that own their locking.
  - **Device IRQs** route through `ioapic::handle_device_irq` and defer
    to userspace drivers via the IRQ-poll softirq (bottom half, IF=1),
    so they are not on the IF=0 hard-IRQ deadlock path.
  Conclusion: the sysctl case was an isolated oversight; the hot IRQ
  paths are otherwise correctly disciplined. A future session extending
  IRQ-context code must preserve the try_lock-or-without_interrupts
  invariant above.

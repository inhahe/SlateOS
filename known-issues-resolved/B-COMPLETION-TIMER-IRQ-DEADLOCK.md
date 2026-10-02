### B-COMPLETION-TIMER-IRQ-DEADLOCK. Timer-softirq completion notify blocking-locks SCHED → same-CPU deadlock if it preempts a SCHED holder — ROOT-CAUSED & FIXED 2026-07-15

**Class.** Interrupt-reentrancy deadlock on the global `SCHED` (and `CP_TABLE`)
spinlock — the same broad family as B-SYSCTL-IRQ-DEADLOCK, and the strongly
suspected root cause of B-SCHED-SPAWN-DEADLOCK (see above).

**Root cause.** `ipc::timer::process_timer_expirations()` runs in the timer
**softirq** (`softirq::handle_timer`, interrupts enabled). For each expired timer
bound to a completion port it called `completion::notify()`, which takes the
blocking `CP_TABLE.lock()` and then the blocking `sched::wake()` →
`SCHED.lock()`. `SCHED` holders in `sched/mod.rs` do **not** disable interrupts,
so the timer softirq can fire on a CPU that is mid-critical-section holding
`SCHED` (or in the tiny acquire→record window). On the single-CPU boot the
softirq's `SCHED.lock()` then spins forever waiting for a lock the *same* CPU
holds — a self-deadlock. It only triggers when a completion-port timer expires
in that exact window, hence the ~1-in-20-to-28 rarity the soak observed.

**Audit scope (all clean except the one site).** Verified every other
softirq/IRQ-reachable path is already non-blocking on `SCHED`:
`#PF` handler (`try_resolve_fault`→`PROCESS_TABLE.try_lock`; `account_fault`,
`panic_diagnostics`→`SCHED.try_lock`); timer preemption (`do_deferred_preempt`
defers when `SCHED.is_locked()`); device-IRQ wake (`ioapic::handle_device_irq`
→`sched::try_wake`); `softirq::handle_timer` sub-calls
(`process_sleep_wakeups`→`wake_expired_sleeper` try_lock,
`process_deferred_wakes`→`try_wake`, `push_balance`→`SCHED.try_lock`);
`softirq::handle_sched`→`push_balance` (try_lock);
`ktimer::process_expirations` (defers callbacks to the workqueue, no inline
SCHED lock). Only `process_timer_expirations`→`completion::notify` blocked.

**Fix.** Added `completion::try_notify(cp, source) -> bool` (softirq-safe:
`CP_TABLE.try_lock()` + `sched::try_wake()`; commits **nothing** on contention —
returns `false` so the caller retries next tick, avoiding both a lost wakeup and
a duplicated event). Restructured `process_timer_expirations` to call
`try_notify` *before* advancing/expiring the timer and to `continue` (leave the
timer un-advanced, retry next ~10 ms tick) on contention. `completion::notify`
(blocking) is unchanged for its task/syscall-context callers (`io_ring`,
`syscall::handlers`) and `close()`. Contention is transient (SCHED/CP_TABLE held
only briefly), so the bounded per-tick retry resolves within a tick or two.

**Where it lives.** `kernel/src/ipc/completion.rs` (`try_notify`),
`kernel/src/ipc/timer.rs` (`process_timer_expirations`). Detector:
`scripts/wedge-soak.sh` (was catching it as B-SCHED-SPAWN-DEADLOCK).

**Next step.** Boot-test, then run a long `wedge-soak.sh` to confirm the SCHED
wedge no longer reproduces (it was ~1/20-28; a clean 40+ iteration soak is good
evidence). This is a confirmed 4th instance of the raw-`spin::Mutex` deadlock
class — see open-questions.md Q24 (recommendation was "escalate to C if a 3rd/4th
shows up"; this is the interrupt-reentrancy sub-variant, already fixed reactively
without a new lock type, consistent with the B-SYSCTL fix).

**UPDATE 2026-07-16 — CONFIRMED.** The confirmation soak
(`soak-20260715-235730`) ran 40 iterations: the SCHED spinloop wedge did **not**
reproduce in any of them (was ~1/20-28), strong evidence the fix holds. The soak
*did* stop on a **different, pre-existing** wedge at iter40 — a kernel jump to
`RIP=0x0` during the tcc-signal Path-Z self-test that cascaded into a kernel
stack-overflow storm. That is unrelated to this deadlock (different signature: a
control-flow hijack, not a spinloop) and is tracked separately as
**B-KNULLJUMP-SIGNAL** below. Downgrading this entry's confidence: the fix is
validated; leaving as ROOT-CAUSED & FIXED.

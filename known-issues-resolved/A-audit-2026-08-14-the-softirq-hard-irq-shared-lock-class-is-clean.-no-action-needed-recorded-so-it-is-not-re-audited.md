### [A] AUDIT 2026-08-14 — the softirq × hard-IRQ shared-lock class is clean. No action needed; recorded so it is not re-audited

**Why it was worth checking.** `softirq::process_pending` re-enables interrupts
(`kernel/src/softirq.rs`, module docs 51–56), so any lock held by a softirq
handler can be observed by a hard-IRQ handler that preempts it. That is
structurally the same failure mode as the rtl8139 deadlock and as
`B-COMPLETION-TIMER-IRQ-DEADLOCK`: the hard IRQ spins on a lock whose holder
cannot run until the IRQ returns. The intersection was believed empty only
because rtl8139 was the tree's single hard-IRQ lock acquisition — "empty by
accident" is not a property that stays true, so it needed enumerating rather
than assuming.

**What was audited.** Every callee reachable from the three softirq handlers
(`handle_timer` 355, `handle_sched` 434, `handle_irq_poll` 445):

| Callee | Lock discipline | Verdict |
|---|---|---|
| `sched::process_sleep_wakeups` (sched/mod.rs 5248) | atomic scan of `SLEEP_QUEUE`, no lock | clean |
| `sched::process_deferred_wakes` (sched/mod.rs 4897) | non-blocking wake path | clean |
| `ipc::timer::process_timer_expirations` (ipc/timer.rs 211) | explicitly non-blocking on `CP_TABLE`/`SCHED`, leaves the timer un-advanced on contention so the next tick retries | clean, and documented against `B-COMPLETION-TIMER-IRQ-DEADLOCK` |
| `ktimer::process_expirations` (ktimer.rs 323) | atomic scan of `TIMERS` | clean |
| `fs::cache::try_flush_expired` (fs/cache.rs 906) | `try_lock`, result deliberately discarded — retries in ~5 s | clean |
| `watchdog`, `kstat`, `loadavg`, `irq_storm`, `irqbalance`, `cpufreq`, `thermal` | zero `.lock()` calls; atomics only | clean |
| `rcu::tick` → `process_callbacks` (rcu.rs 483) | all three `CALLBACKS.lock()` sites (403, 486, 547) wrapped in `cpu::without_interrupts`, popping one callback per critical section and invoking it with the lock released | clean, and the comment at 393–401 records the observed 2/10 boot hang that motivated it |

`rcu` is the only softirq callee that takes a blocking lock at all, and it is
the one already hardened — the fix predates this audit and cites the boot hang
it was found by.

**Result: the intersection is empty, and empty by construction rather than by
luck.** Each site either uses atomics, uses `try_lock`, or masks interrupts for
the lock-hold window. No change was made.

**What would break it.** Adding a `.lock()` (not `try_lock`, not
`without_interrupts`-wrapped) to any callee of `handle_timer` — which is a wide
and growing list: it already fans out to 12 subsystems — while that same lock is
reachable from a hard-IRQ handler. The `handle_timer` fan-out is the risk
surface to re-check when a subsystem is added to it, not the whole kernel.

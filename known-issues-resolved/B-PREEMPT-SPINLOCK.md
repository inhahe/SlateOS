### B-PREEMPT-SPINLOCK. Involuntary preemption while holding a tracked spinlock → single-CPU priority-inversion deadlock — ROOT-CAUSED & FIXED 2026-07-01

**Where:** `kernel/src/sched/mod.rs` (`do_deferred_preempt`), `kernel/src/sync.rs`
(`Mutex::lock`/`try_lock`/`MutexGuard::drop`). Manifested as a hang in
`accounting::self_test` on the `ACCT` lock (`kernel/src/mm/accounting.rs`).

**This is the true root cause of the long-standing intermittent
spawn/kill/reap / accounting-self-test hang** previously filed as **F6**
("Accounting self-test hang — LIKELY CURED INCIDENTALLY", further below) and
related to the B-PTHREAD-YIELDBUDGET / TD31 "total silence, no dump"
fingerprint. F6 was never actually cured — it just didn't recur in the soak
because the trigger is timing-dependent (~5%). The spinlock stall detector
(commit `c8c1fa63`) finally caught it red-handed.

**Symptom / evidence:** boot hangs mid-`accounting` self-test. The stall
detector prints:
```
[sync] *** SPINLOCK STALL *** lock 'ACCT' ... (cpu 0, task 0, ... iters)
[lockdep]   cpu 0 holds 2 lock(s): [0] ACCT [1] ACCT
```
The "recursive" `[0] ACCT [1] ACCT` is NOT true recursion. lockdep's held
stack is **per-CPU** and is not cleared on context switch, so `[0]` is the
still-tracked entry of a task that was **preempted while holding `ACCT`**, and
`[1]` is a second, higher-priority task now spinning to acquire the same lock —
both accumulated on cpu 0's held stack.

**Root cause:** a kernel spinlock must never be held across a context switch.
`crate::sync::Mutex` did not disable preemption while held, so the timer ISR
could involuntarily preempt (`do_deferred_preempt` → `preempt`) a task
mid-critical-section. On a single CPU, if a higher-priority task (e.g. the
prio-31 boot self-test driver) then spins on that lock, the preempted holder
can never be rescheduled to release it → permanent deadlock. `do_deferred_preempt`
already had a *SCHED-only* guard (`SCHED.is_locked()`) for exactly this hazard —
it was a band-aid that covered one lock instead of the general invariant.

**Fix (the proper, general one):** a per-CPU preempt-disable count
(`PREEMPT_DISABLE_COUNT`, Linux `preempt_count` analogue). `Mutex::lock`/
`try_lock` call `sched::preempt_disable()` for the whole hold; `MutexGuard::drop`
calls `preempt_enable()` **after** the physical unlock (the inner spin guard is
now held in `ManuallyDrop` so the unlock is ordered before the enable — closing
the tiny window where a tick could switch away with the lock still physically
held). `do_deferred_preempt` refuses to involuntarily switch while
`preempt_count(cpu) > 0`, re-arming `NEED_RESCHED` so the preemption lands on a
later tick after the lock is released. Interrupts stay **enabled** (this is
preempt-disable, not IRQ-disable); locks also taken from a hardware ISR (e.g.
cgroup `TABLE` via `timer_tick`) already use `try_lock` on the ISR side, so
preempt-disable alone is sufficient.

**Verification:** 3× consecutive green boot tests (193–196s), accounting
self-test now passes the previously-deadlocking "Largest RSS" step; no
`SPINLOCK STALL` in the serial log; clippy clean on both changed files.

**Limitation / follow-up:** the guard covers *involuntary* preemption only.
Voluntarily yielding/blocking (`yield_now`/`block`) while holding a tracked
spinlock is still a caller bug and is not guarded (there is no such call site
today). **Done (2026-07-01):** added a one-shot warning in `schedule_inner`'s
voluntary-switch path when `preempt_count(cpu) > 0` (commit `49c92d346`);
it stayed silent across all boots, confirming no offending call site exists.
Also added (commit `ebd5c4b21`) a lockdep instant SELF-DEADLOCK diagnostic when
the *same* lock instance is re-acquired on one CPU — fires immediately instead
of waiting ~30s for the stall detector, now reliable because tracked mutexes no
longer carry stale per-CPU held-stack entries across a context switch.

**Raw `spin::Mutex` audit (2026-07-01):** the preempt-disable fix protects only
`crate::sync::Mutex`; a *raw* `spin::Mutex` (250+ call sites, mostly procfs/sysfs
leaf backends) held across a preemptible path and contended by a higher-priority
task is the same latent deadlock class — and is *invisible* to both lockdep and
the stall detector. Audited the only plausibly-dangerous category, the blocking
IPC primitives (`futex`, `pipe`, `stream_socket`, `semaphore`, `eventfd`,
`epoll`, `timerfd`, `signalfd`): **all clean** — every one follows the correct
enqueue-waiter → `drop(table)` → `block_current()` discipline (e.g.
`futex.rs:340-379` scopes the table lock in a block that closes before the park).
The remaining raw-`spin::Mutex` uses are short snapshot copies where the
held-across-preempt window is a handful of instructions and cross-priority
contention is implausible. **Proper systemic fix (deferred tech-debt):** migrate
kernel-internal raw `spin::Mutex` to `crate::sync::Mutex` so *all* kernel
spinlocks disable preemption and get lockdep coverage — gated on first checking
the lockdep class-table capacity (a 250-lock bulk migration could overflow it),
so it needs a capacity bump or a per-class opt-in rather than a blind sweep.

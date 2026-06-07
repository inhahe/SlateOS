# Known Issues — OS kernel

Running list of unsolved bugs and technical debt.  Each entry should
have enough context to act on later: what the bug or debt is, where in
the code it lives, how to reproduce it (for bugs), and what the proper
fix looks like (for debt).

Per CLAUDE.md: "Ideally, bugs and tech debt are fixed immediately as
they're discovered — the tracking file is a fallback for when something
genuinely can't be addressed in the current task, not a place to defer
work that should be done now."

---

## Active Bugs

### 1. Accounting self-test occasionally hangs at boot (intermittent)

**Where:** `kernel/src/mm/accounting.rs` — self-test path, specifically
after "[accounting]   Destroy: OK".

**Repro:** Run `bash scripts/boot-test.sh`.  Observed once on
2026-06-07 during batch 473 boot test (`build/serial-test.txt`
truncated at line 3073, after `[accounting]   Destroy: OK`).  Retry
passed cleanly at 22s.  Frequency unknown; first observation.

**Symptoms:** Serial output stops mid-accounting self-test before
the expected next line `[accounting]   Tracked count: 0 (after
cleanup)` and the subsequent `[accounting] Self-test PASSED`
marker.  Anti-starvation log floods every tick afterward,
suggesting scheduler is still alive but the accounting test thread
is blocked.  BOOT_OK sentinel never emitted; `boot-test.sh` times
out at 300s.

**Severity:** Low-frequency, retry-passes.  No corruption observed.

**Post-F1/F2/F3/F4 status (2026-06-07):** Did NOT recur in 60
consecutive boot tests after the four IRQ-safety fixes (two
30-run soaks).  This is consistent with — but does not prove —
the hypothesis that the accounting hang shared root cause with
one of the fixed lock-class races.  Keeping open as a watchlist
item; if no recurrence after another 60-run soak, downgrade to
"likely-cured-incidentally" and close.

**Hypothesis (updated 2026-06-07):** Same shape as the now-fixed RCU
hang (see Fixed Bugs #1): a spinlock held by the main code on the
BSP that is also acquired from a timer-softirq path on the same CPU,
deadlocking when an ISR fires inside the held window.  Worth checking
whether `kernel/src/mm/accounting.rs` has a lock that gets touched
from both `accounting::destroy()` (boot path) and any softirq /
timer-tick callback.

**Proper fix:**
  1. Read `kernel/src/mm/accounting.rs` self-test path and identify
     what runs between the `Destroy: OK` print and the
     `Tracked count` print.
  2. Audit all `Mutex.lock()` sites for ones also reached from
     softirq context (search for `accounting::` references in
     `kernel/src/softirq.rs`, the timer tick paths, etc.).
  3. Wrap any such site in `crate::cpu::without_interrupts(...)` —
     same pattern as the RCU fix.
  4. If no shared lock exists, add a finer-grained probe between
     `Destroy: OK` and `Tracked count` to localize the hang.

### 2. Invariant self-test hangs after first check_all (intermittent)

**Where:** `kernel/src/invariant.rs` — `self_test()`, specifically
between the test 1 `check_all()` call (whose detail lines all print)
and the test 2 `all_ok()` call.

**Repro:** Run `bash scripts/boot-test.sh`.  Observed once on
2026-06-07 during the post-RCU-fix soak (`build/soak-hang-run2.txt`,
2614 lines; last serial line `[invariant]     [PASS] cap_audit_balance:
OK: 5 events, 1 denials`).  Same boot run did NOT exhibit the
softirq race (that ran later in boot order).  Frequency unknown.

**Symptoms:** Serial output stops cleanly after the 8th individual
`[PASS] …` detail line and before the test 2
`[invariant]   Quick check: OK` line.  `all_ok()` re-invokes
`check_all()`, so a hang in re-entering one of the per-check
closures is plausible.  BOOT_OK never emitted; `boot-test.sh` times
out at 300s.

**Severity:** Low-frequency, single observation, retry-passes.

**Post-F1/F2/F3/F4 status (2026-06-07):** Did NOT recur in 60
consecutive boot tests after the four IRQ-safety fixes (two
30-run soaks).  Notably, the `invariant` checks include
`frame_accounting` (calls `frame::stats()`), which F4 made
IRQ-safe — that fix is the most likely incidental cure here too.
Keeping open as a watchlist item; if no recurrence after another
60-run soak, downgrade to "likely-cured-incidentally" and close.

**Hypothesis:** Same shape as the now-fixed RCU/softirq hangs (see
Fixed Bugs F1, F3) — a spinlock acquired by one of the invariant
checks (frame accounting, heap balance, scheduler balance, IPC
counters, capability audit) that is also touched from a softirq /
timer-tick callback.  The test 1 call may have completed only
because the timer tick happened to land in a non-shared window;
test 2's re-entry hit the bad window.

**Proper fix:**
  1. Enumerate the closures registered in
     `kernel/src/invariant.rs` (frame_accounting, heap_balance,
     frag_range, pressure_range, sched_balance, object_balance,
     ipc_counters, cap_audit_balance).
  2. For each, identify which subsystem locks it touches.
  3. Cross-reference with softirq-context call sites (timer tick,
     RCU tick, deferred work).
  4. Wrap any shared-lock acquisition reached from a check closure
     in `crate::cpu::without_interrupts(...)`, OR wrap the whole
     `self_test()` body if the suspect is a soft path that can
     tolerate it.  Prefer the former for production-path locks.

---

## Fixed Bugs

### F1. RCU self-test occasionally hangs at boot (intermittent) — FIXED 2026-06-07

**Where:** `kernel/src/rcu.rs` — `call()`, `process_callbacks()`,
`stats()` and (defense-in-depth) `synchronize()`.

**Root cause:** The `CALLBACKS` spinlock was acquired both from
direct callers (boot path → `rcu::call`, `rcu::stats`,
`rcu::synchronize` → `process_callbacks`) AND from `rcu::tick()`
running in softirq context.  Softirqs dispatch with interrupts
re-enabled on the same CPU.  If a timer ISR fired while a direct
caller held the lock, the softirq's `process_callbacks()` re-entered
the same critical section on the same CPU and deadlocked the
spin::Mutex.  The hang manifested between
`[rcu]   Quiescent state: OK` and `[rcu]   Callback registration: OK`
(i.e. inside `rcu::call`) because that's the first lock acquisition
after the periodic softirq starts running.

**Diagnosed by:** Running boot-test.sh 10× — observed 2 hangs, both
with the serial log truncated at exactly the same point (after
"Quiescent state" probe, before "Callback registration").  This
showed the hang was in `call()`, not `synchronize()` as the original
hypothesis suggested.

**Fix:** Wrap every `CALLBACKS.lock()` site in
`crate::cpu::without_interrupts(...)` so the lock cannot be acquired
from a path that is interruptible.  Additionally, in `synchronize()`,
explicitly bump the calling CPU's own QS counter after snapshotting
(the caller cannot itself be in a read-side critical section by RCU
invariant), and add a million-iteration safety cap with diagnostic
print so any future grace-period failure surfaces a warning instead
of a silent hang.  Added finer-grained "[rcu]   Synchronize: pre/post"
self-test probes to localize any future regression.

**Verification:** 20/20 consecutive boot tests pass after the fix
(previously 2/10 hung).

### F2. Watchdog self-test heartbeat-increment assertion race — FIXED 2026-06-07

**Where:** `kernel/src/watchdog.rs` — `self_test()` test 1.

**Root cause:** The test does
`before = HEARTBEATS[cpu].load(); heartbeat(); after = HEARTBEATS[cpu].load();`
and asserts `after == before + 1`.  But the APIC timer ISR also calls
`watchdog::heartbeat()` on every tick (via `apic.rs`), so a timer
interrupt landing inside the before→after window can cause the
counter to advance twice, tripping the assertion.  Observed once on
2026-06-07: panic with `left: 368, right: 367`.

**Fix:** Wrap test 1's load/heartbeat/load sequence in
`crate::cpu::without_interrupts(...)`.

**Verification:** 20/20 consecutive boot tests pass after the fix.

### F3. Softirq self-test races APIC timer ISR — FIXED 2026-06-07

**Where:** `kernel/src/softirq.rs` — `self_test()` tests 2, 3, and 4.

**Root cause:** The self-test runs after `[boot] Interrupts enabled —
preemptive scheduling active`, so the APIC timer ISR fires
asynchronously throughout the test.  The ISR's path calls
`process_pending()` on the same CPU, which mutates `TOTAL_RUNS`,
`TOTAL_HANDLERS`, `IN_SOFTIRQ`, and `PENDING`.  Three races:

  * Test 2 (no-op fast path): an ISR firing between
    `process_pending()` returning and `TOTAL_RUNS.load()` bumps the
    counter and trips `runs_after != runs_before`.
  * Test 3 (dispatch + clear): an ISR firing between `raise()` and
    the test's own `process_pending()` drains TIMER_SOFTIRQ first;
    the test's call then runs no handler and trips
    `handlers_after <= handlers_before`.
  * Test 4 (re-entry guard): after the test clears
    `IN_SOFTIRQ[cpu] = false`, an ISR firing before the
    `still_pending` load runs a real `process_pending()`, consumes
    TIMER_SOFTIRQ, and trips "bits were consumed despite re-entry
    guard".  Observed once on 2026-06-07 during the post-RCU-fix
    soak (build/serial-test.txt at 11:44).

**Fix:** Wrap each of tests 2, 3, and 4 in
`crate::cpu::without_interrupts(...)`.  In test 4, also sample
`PENDING` *before* clearing `IN_SOFTIRQ` so the semantic ordering
("did the guarded call consume bits?") is preserved.  `process_pending`
internally toggles IF (STI→handlers→CLI); `without_interrupts` saves
and restores the outer IF state, so the boot path's interrupt state
post-test is unchanged.  Test 1 already had its own CLI/STI window
and didn't need changes.

**Verification:** Boot test passes cleanly with `softirq` self-test
showing all four sub-tests OK and `Self-test PASSED`.  Post-fix
30-run soak: 29/30 pass with zero softirq self-test failures (the
single failure was in `frag_history` test 6 — see F4 below).

### F5. `frame::ALLOCATOR` lock uniformly IRQ-safe — FIXED 2026-06-07

**Where:** `kernel/src/mm/frame.rs` — all 13 remaining `allocator.lock()`
acquisition sites outside `pcpu_refill`/`pcpu_drain` (which are
already called with IRQs off) and `try_stats()` (panic-only).

**Why this was technical debt (was TD1):** F4 made `stats()`
IRQ-safe but left `alloc_*`, `free_*`, `is_allocator_owned`,
`refcount`, `ref_inc`, `ref_dec`, and `validate_free_lists` taking
the lock without wrapping in `without_interrupts`.  No
currently-registered softirq path took the allocator lock (audited
2026-06-07), so there was no exploitable deadlock — but the next
softirq subsystem that touched the allocator (kswapd periodic
reclaim, RCU-deferred page free, memory-pressure tick) would have
silently re-opened the same race that F4 closed.

**Fix:** Wrap each acquisition site in
`crate::cpu::without_interrupts(...)` at the call site, matching
the F1/F3/F4/workqueue pattern.  The multi-attempt `alloc_order_inner`
and `alloc_order_constrained_inner` paths use a per-attempt
without_interrupts so IRQs are re-enabled between attempts (so
reclaim/compact/OOM can run normally and wake other tasks).  Did
NOT wrap `pcpu_refill` / `pcpu_drain` — their callers already run
with IRQs disabled and the function-level comments document this
invariant.  Used inline wraps rather than a helper because the
sites have varied shape (KernelResult returns, multi-attempt retry
loops, value vs Option returns) — a `with_allocator` helper would
have required `FnOnce(&mut BuddyAllocator) -> R` plumbing at every
site, which is more code churn than the wraps themselves.

**Verification:** Post-fix 30/30 boot tests pass.  Zero allocator-lock
hangs observed across this soak.

### F4. frag_history self-test test 6 hangs in sample() loop — FIXED 2026-06-07

**Where:** `kernel/src/mm/frag_history.rs` — `self_test()` test 6
("Ring buffer wraps correctly"), inside the
`for _ in 0..HISTORY_SIZE + 5 { sample(); }` loop.

**Root cause (hypothesis, verified by soak):** `sample()` calls
`mm::frame::stats()` on every iteration, which acquires
`frame::ALLOCATOR.lock()`.  The boot path runs with interrupts
enabled, so an APIC timer ISR could fire on the same CPU while the
lock was held.  Per a softirq-handler audit, no currently-registered
softirq path takes `ALLOCATOR.lock`, so a clean dead-lock chain
wasn't conclusively proven — but the empirical data (hang exactly
in this 37-iteration tight loop over a lock-acquiring call) plus
the cure (see Fix) make this the most likely explanation.  A
plausible alternate path: any future softirq subsystem (kswapd
periodic reclaim, RCU-deferred page free, memory-pressure tick)
that touched the allocator would have re-introduced the race.

**Diagnosed by:** Post-F3 30-run soak showed `[frag_history]
Trend: OK (Stable)` as the last serial line of one failure
(`build/soak-hang-run18.txt`).  Bisected the hang window to the
test 6 sample-loop.

**Fix:** Made `frame::stats()` itself IRQ-safe by wrapping the
`ALLOCATOR.lock()` acquisition in `crate::cpu::without_interrupts(...)`.
The companion `try_stats()` (panic-handler variant) already used
`try_lock()` for the same family of reasons; this brings the
regular `stats()` to parity.  Hardening — eliminates an entire
class of same-CPU IRQ-vs-main deadlocks on the buddy allocator
lock without measurable performance cost (CLI/STI on a stats read
that already serializes on a spinlock is negligible).

**Verification:** Post-fix 30/30 boot tests pass; zero recurrence
of the frag_history hang AND zero recurrence of Active Bugs #1
(accounting) and #2 (invariant) over those same 30 runs.

---

## Technical Debt

_(No outstanding technical debt — TD1 closed as F5 on 2026-06-07.)_

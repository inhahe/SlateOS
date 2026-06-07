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

---

## Technical Debt

(none recorded yet — file created 2026-06-06)

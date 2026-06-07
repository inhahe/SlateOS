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

## Bugs

### 1. RCU self-test occasionally hangs at boot (intermittent)

**Where:** `kernel/src/sync/rcu.rs` — self-test path, specifically after
"Quiescent state: OK (counter N → N+1)" emission.

**Repro:** Run `bash scripts/boot-test.sh` repeatedly.  Observed once
on 2026-06-06 during batch 394 boot test (`build/serial-test.txt`
truncated at line 1627, mid-RCU self-test).  Retry passed cleanly at
26s.  Frequency initially appeared low (~1 in N for some N>10 — not
yet characterized).

**2026-06-06 frequency revision (batch 455 boot test):** Observed
2 consecutive hangs in 3 attempts on the same QEMU invocation
parameters during batch 455 boot test.  Both stalled after
`[rcu]   Callback registration: OK` with no further serial output
within the 300s timeout.  Third attempt passed at 23s.  This is
substantially more frequent than the original estimate; the bug may
be highly sensitive to host scheduler / QEMU timing such that some
windows reproduce it 60%+ of the time while others almost never
hit it.  Worth prioritizing a real fix rather than relying on retry.

**Symptoms:** Serial output stops mid-RCU test; no further milestones.
QEMU appears alive but kernel makes no further progress.  BOOT_OK
sentinel never emitted, so `boot-test.sh` times out at 300s.

**Severity:** Low-frequency, retry-passes.  No corruption observed.

**Hypothesis (unconfirmed):** Likely a missed quiescent-state
declaration or a per-CPU counter race in the synchronize_rcu() wait
loop when running under a single CPU.  The self-test may be polling on
a condition that requires another CPU to advance the grace period —
on a UMA single-CPU QEMU configuration that condition may never fire.

**Proper fix:**
  1. Read `kernel/src/sync/rcu.rs` self-test path and locate the post-
     "Quiescent state" assertion.
  2. Add a printout immediately before and after the hanging point to
     pin down whether it's a synchronize_rcu() spin, a per-CPU walk,
     or a final-counter check.
  3. Audit for single-CPU edge cases — RCU implementations
     traditionally assume ≥2 CPUs for grace-period progress and need
     explicit "self-quiesce" calls under UMA.
  4. If the hang is a per-CPU iteration that expects nonzero quiescent
     counters from CPUs that don't exist, gate the iteration on
     `online_cpu_mask`.

---

### 2. Accounting self-test occasionally hangs at boot (intermittent)

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

**Hypothesis (unconfirmed):** Possible deadlock or lost wakeup in
the cleanup path between Destroy and Tracked-count read-back —
maybe a per-CPU iteration awaiting a counter that's already been
decremented to zero, or a missed wake on a condition variable
guarding the tracked-count snapshot.

**Proper fix:**
  1. Read `kernel/src/mm/accounting.rs` self-test path and identify
     what runs between the `Destroy: OK` print and the
     `Tracked count` print.
  2. Add a finer-grained probe between those two stamps to localize
     the hang to a specific call.
  3. Audit any locks held across the destroy → tracked-count
     transition; in particular, look for a global accounting lock
     that the destroy path takes and that the tracked-count snapshot
     also needs.
  4. If the issue is a per-CPU iteration awaiting a counter that
     races with destroy decrementing it, switch to a snapshot-read
     pattern that doesn't spin.

---

## Technical Debt

(none recorded yet — file created 2026-06-06)

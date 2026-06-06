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
26s.  Frequency appears low (~1 in N for some N>10 — not yet
characterized).

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

## Technical Debt

(none recorded yet — file created 2026-06-06)

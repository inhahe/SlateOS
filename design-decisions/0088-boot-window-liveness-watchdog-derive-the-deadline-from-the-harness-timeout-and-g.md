## §88 — Boot-window liveness watchdog: derive the deadline from the harness timeout, and gate the progress detectors on serial silence

**Date:** 2026-07-27
**Decided by:** Claude (autonomous)

**Context.** The boot-window liveness watchdog
(`kernel/src/sched/mod.rs`) had three false-fire modes and reported a hang on
every healthy boot (known-issues BUG-LIVENESS-DEADLINE-FALSE-FIRE). Two design
choices in the fix have genuine tradeoffs.

### Decision 1 — the wall-clock boot deadline is derived from `boot-test.sh`'s `--timeout`, not hardcoded

The deadline must sit above the slowest healthy armed-to-`BOOT_OK` duration and
below the harness's kill. Those are two moving numbers, and the kernel-side
constant had no way to learn about either.

* **Alternative A — re-tune the constant** (raise 200 s → ~400 s). Zero moving
  parts, no cmdline plumbing, works on real hardware unchanged. But it is a
  treadmill: `scripts/boot-test.sh` documents in its own `TIMEOUT` comment that
  the Path-Z self-test battery keeps growing, so the constant will drift out of
  sync again — as it already did once, silently, for weeks.
* **Alternative B — derive it (chosen).** The harness passes
  `sched.boot_deadline_ms=$((TIMEOUT * 1000))` on the Limine cmdline;
  `liveness_arm()` subtracts a 45 s dump margin and the monotonic arm timestamp
  to convert the QEMU-relative budget into the armed-relative units the backstop
  measures in. Cost: a kernel↔harness coupling and a new cmdline key, plus a
  fallback path (`LIVENESS_BOOT_DEADLINE_DEFAULT_NS`, 900 s) for boots with no
  such cmdline. Benefit: the invariant "dump before the harness gives up, never
  before a boot the harness would have tolerated" becomes structural rather than
  a tuned guess, and raising `--timeout` moves both in lockstep.

Chosen B. The coupling is real but it is the *correct* coupling — the two
numbers are answering the same question, and a constant that has already gone
stale once will go stale again.

### Decision 2 — both progress detectors require serial silence

`USEFUL_WORK_TICKS` (ticks preempting ring-3 code or a CPU with a queued task)
is not a valid "boot is advancing" signal: `kmain` and a *starting* ring-3
process both run long stretches of kernel-side work that the counter cannot see.

* **Alternative A — fix the tick charging** so kernel-side boot work counts
  (e.g. a per-CPU "parked in the idle loop" flag replacing the
  `local_has_real_work` proxy). Most principled, but there are several
  independent idle paths (`idle::idle_once`, the scheduler's HLT fallback loop,
  the AP idle loop in `smp.rs`) plus healthy busy-waits (`keyboard`, `virtio`),
  so "is this CPU idle?" has no single choke point today and every missed site
  is a new false positive.
* **Alternative B — gate on serial output (chosen).** `serial::_print` is a
  single choke point; one relaxed increment there is free next to the ~87 µs/char
  the UART costs. This kernel narrates its boot continuously, so a silent
  interval means execution really stopped and a chatty one means it did not —
  and it is exactly the criterion `boot-test.sh --stall-secs` already applies
  from the outside, so the inside and outside views now agree.

Chosen B. **Cost accepted:** a livelock that keeps printing escapes the two 15 s
detectors. That is tolerable because the wall-clock boot deadline catches every
hang mode by construction, and (per decision 1) that deadline is now
trustworthy. Alternative A remains the better long-term answer if a single
authoritative per-CPU idle flag ever exists; the two are complementary, not
exclusive.

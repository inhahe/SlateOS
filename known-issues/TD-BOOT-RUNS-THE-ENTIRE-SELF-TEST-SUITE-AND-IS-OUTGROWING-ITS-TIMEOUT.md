### TD-BOOT-RUNS-THE-ENTIRE-SELF-TEST-SUITE-AND-IS-OUTGROWING-ITS-TIMEOUT — LOGGED 2026-08-14

**Where:** `kernel/src/main.rs` boot path (every subsystem's `self_test()` is
called inline before `BOOT_OK`), and the `TIMEOUT` default in
`scripts/boot-test.sh`.

**The number.** Measured 2026-08-14 under TCG on `qemu64`: `BOOT_OK` at
**~456 s**. The `boot-test.sh` comment describing the same figure said
**~305 s** and the default timeout was **480 s** — a 24 s margin, i.e. ~5%.
The in-kernel liveness detector (`sched::liveness_boot_deadline_check`, armed
at `harness_timeout − 45 s − now`) was consequently firing
`BOOT DEADLINE EXCEEDED` with a full task-table dump and a 17-frame backtrace
on **every clean boot**, exactly the false positive its own doc comment says
it was retuned to avoid.

**Mitigated, not fixed.** The default timeout is now 900 s (~2× observed) and
the stale comment carries the measured number and an instruction to re-measure.
That buys headroom; it does not change the trajectory.

**Why it is debt.** Boot time is a *linear function of the total number of
self-tests in the tree*, and that number only goes up — the Path-Z ring-3
toolchain tests alone each spawn a real glibc/tcc/make/dash process under
`ld.so`. Every lane's new subsystem test lands on the critical path of every
other lane's boot test. The failure mode when the margin runs out is
expensive to diagnose: a healthy kernel is killed mid-boot and reported as a
hang, and the first hypothesis is never "the clock".

**What the proper fix looks like.** Stop running the full suite on every boot.
Options, roughly in order of preference:

1. **Tiered suites.** A `smoke` tier on the critical path (boot, mm, sched,
   syscall dispatch — the things whose failure makes every later test
   meaningless) and a `full` tier behind a cmdline flag, with the gate running
   `full` and interactive/iteration runs running `smoke`. Needs a registry of
   tests with tier tags rather than the current hand-written call list.
2. **Run the suite as a deferred task after `BOOT_OK`,** the way `bench::run_all`
   already does, with its own `SELFTEST_OK` marker. Boot latency stops growing;
   the gate waits for the later marker. Cheapest to implement, but it decouples
   "booted" from "tested", so a gate that only checks `BOOT_OK` would silently
   stop testing — the marker change has to land in the same commit.
3. **Only run the tests for subsystems whose sources changed.** Correct in
   principle, needs build-time plumbing we do not have, and cross-subsystem
   regressions are exactly the kind this suite exists to catch. Not recommended.

**Trigger.** Do this before the next time a boot test is diagnosed as a hang
and turns out to be a timeout — or when `BOOT_OK` passes ~600 s, whichever
comes first.

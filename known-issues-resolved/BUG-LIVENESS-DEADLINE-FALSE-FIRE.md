### BUG-LIVENESS-DEADLINE-FALSE-FIRE. The boot-window liveness watchdog reported a hang on *every* healthy boot — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Every green boot-test run — one that reaches `BOOT_OK` and exits 0
— printed alarming watchdog reports and buried the serial log under ~14,000
lines of task-table dump:

```
[liveness] BOOT DEADLINE EXCEEDED: still armed 200s after arming (no BOOT_OK).
           The progress-based detectors did not trip, so this is a livelock or
           partial hang … Dumping task table:
…13,800 lines…
[liveness] SYSTEM HANG: no task-level forward progress for 15+ seconds
           (useful_work=543, all CPUs idle-ticking). Dumping task table:
```

Reproduce: any `./scripts/boot-test.sh` run; the reports are in
`build/serial-test.txt` (also present in the archived
`build/serial-loop-{1,2,3}.txt`). A watchdog that cries wolf on every run is
worse than no watchdog — a *real* dump was indistinguishable from the noise.

**Three independent false-fire modes, all in `kernel/src/sched/mod.rs`.**

1. **The wall-clock boot deadline was a hardcoded constant that went stale.**
   `LIVENESS_BOOT_DEADLINE_NS` was 200 s, tuned on 2026-07-02 against a
   then-measured healthy armed window of **67.7 s**. The Path-Z ring-3 toolchain
   battery has grown a lot since; by 2026-07-27 a healthy boot needed ~350 s to
   `BOOT_OK` and the armed window alone exceeded 200 s, so the deadline fired on
   every run. `scripts/boot-test.sh`'s own `TIMEOUT` comment already documents
   that "the suite keeps growing" — the constant could not help drifting.

2. **The total-hang detector's progress signal does not cover kernel-side boot
   work.** `USEFUL_WORK_TICKS` only advances for a timer tick that preempted
   ring-3 code (`from_user`) or a CPU with a *queued* task
   (`local_has_real_work`). Neither holds during the long kernel-side stretches
   of boot: `kmain` runs with no queued task, and a *starting* ring-3 process
   spends nearly all its wall time inside the kernel on its own behalf (ELF
   load, demand-paging storm, filesystem I/O), so ticks land in kernel mode with
   an empty run queue. Measured in `build/serial-loop-1.txt`: `useful_work=8`
   after `heartbeat=2501` (25 s of ticks) — while the log shows the kernel busily
   spawning ring-3 fastpy processes. Healthy boot, "total hang" verdict.

3. **The deadline dump caused the second report.** Emitting ~13,800 lines over a
   115200-baud UART from inside the timer ISR stops all task progress for
   minutes, which then satisfied detector 2 — a watchdog whose own diagnostic
   trips another watchdog.

A fourth defect hid all of the above: `liveness_disarm()` logs the measured
healthy armed duration (the number needed to keep the deadline honest) **only
when still armed**. Once a detector had disarmed us the line vanished — so the
one measurement that would have contradicted the stale constant was never
printed on any run where it mattered.

**Fix (proper, not a re-tune).**

- **Derive the deadline from the harness's own timeout instead of hardcoding
  it.** `scripts/boot-test.sh` now always passes
  `sched.boot_deadline_ms=$((TIMEOUT * 1000))` on the Limine cmdline, and
  `liveness_arm()` computes
  `deadline = budget − LIVENESS_DEADLINE_MARGIN_NS (45 s) − (monotonic now)`.
  Subtracting the arm timestamp converts the harness's QEMU-relative budget into
  the armed-relative units the backstop measures in, so the fire time lands 45 s
  of wall-clock before the kill regardless of how much of the budget early boot
  ate. The invariant becomes structural: the watchdog fires iff the harness was
  about to give up anyway, and raising `--timeout` moves both in lockstep.
  `LIVENESS_BOOT_DEADLINE_DEFAULT_NS` (900 s) covers boots with no such cmdline
  (real hardware, hand-rolled QEMU), where nothing is going to kill us.
- **Add a serial-output progress signal and gate both detectors on silence.**
  `serial::_print` bumps `OUTPUT_COUNT` (read via `serial::output_count()`); the
  scheduler snapshots it each interval and both the total-hang and busy-livelock
  detectors stand down whenever anything was printed during the interval. This
  kernel narrates its boot continuously, so a *silent* interval means execution
  really stopped and a chatty one means it did not — the same criterion
  `boot-test.sh --stall-secs` uses from the outside. It also removes mode 3 for
  free: the dump itself keeps the counter moving.
- **Always log the armed duration at disarm**, and shout when a detector had
  already disarmed us on a boot that went on to reach `BOOT_OK` — that
  combination *is* a false positive and should be impossible to miss.
- `liveness_arm()` now also logs the deadline it chose and whether it came from
  the harness or the fallback.

**Tradeoff accepted.** A livelock that keeps printing now escapes the two 15 s
detectors. It does *not* escape the wall-clock boot deadline, which catches
every hang mode by construction — and that deadline is now trustworthy, which it
was not before. Recorded in `design-decisions.md`.

**Verification.** New boot self-test coverage in `test_liveness_watchdog`:
`parse_boot_deadline_ns` (key found anywhere in the cmdline; absent/malformed
rejected), `armed_relative_deadline_ns` (480 s budget − 60 s spent − 45 s margin
= 375 s; a spent budget yields `None` rather than wrapping), and a silence-gate
sequence that freezes the useful-work counter while printing each interval and
asserts the watchdog stays armed with a zero stall counter. Plus a full boot
test whose log must contain no `BOOT DEADLINE EXCEEDED` / `SYSTEM HANG` line and
must contain the `[liveness] disarmed after …` measurement without the
FALSE POSITIVE warning.

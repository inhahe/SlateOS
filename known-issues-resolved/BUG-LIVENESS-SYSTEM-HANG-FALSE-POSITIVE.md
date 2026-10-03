### BUG-LIVENESS-SYSTEM-HANG-FALSE-POSITIVE. The total-hang detector fired on a healthy boot and, by disarming, blinded the wall-clock backstop for the remaining ~600 s — 2026-08-15 — ✅ FIXED (lane A; the total-hang branch no longer disarms, bounded to 3 reports via `LIVENESS_MAX_HANG_REPORTS`; `check_liveness_failures` in boot-test.sh asserts the contract)

**In short.** The boot watchdog announced `SYSTEM HANG`. The machine had not
hung: the boot continued for another ten minutes and reached `BOOT_OK`. The
report is a false positive, and the kernel itself says so at disarm time. The
damage is not the wrong line — it is that the code path printing it also turns
the watchdog *off*, so hang detection (including the wall-clock boot deadline
that is supposed to catch every hang mode by construction) was dead for the rest
of that boot.

**Where.** `kernel/src/sched/mod.rs` — `liveness_check()` (~2547), the total-hang
branch at ~2585-2610. Lane C found it; lane C does not write `kernel/**`, so the
full evidence and analysis is filed as
`requests/c-a-liveness-system-hang-false-positive.md`. This entry exists so the
finding is not lost if that request is closed or moved.

**Reproduce.** Any full `scripts/boot-test.sh` run is a candidate; it is
intermittent. Observed on the lane-c → main merge boot of 2026-08-15 (green,
exit 0, 1156 s, `BOOT_OK` at line 25970 of `build/serial-test.txt`):

- line 3024 — `[liveness] SYSTEM HANG: no task-level forward progress and no serial output for 15+ seconds (useful_work=82, all CPUs idle-ticking)`, immediately after the fastpy `inredirect` ring-3 self-test passed and immediately before a 3.5 MB fastpy ELF spawn (line 3112);
- line 3025 — `local_has_real_work=false preempt_disable_depth=1`, `heartbeat=12001`, `ctx_switches=880`, and 15 of 16 recent RIP samples at the *same* kernel address `0xffffffff80646c27`, i.e. cpu0 busy in one kernel path on a task's behalf;
- breadcrumbs stop after the 120 s one (line 2843) although the boot ran ~750 s armed — the backstop went dark at that moment;
- line 25971 — `[liveness] disarmed after 753.531s armed … WARNING: a detector already disarmed us, yet boot reached BOOT_OK, so that report was a FALSE POSITIVE`.

Earlier sighting of the same line with `useful_work=6` is recorded at
known-issues.md:26337, where it was dismissed as non-fatal.

**Root cause.** The detector requires (a) `USEFUL_WORK_TICKS` frozen and (b)
serial silence, for three consecutive 5 s intervals. Condition (a) holds during
*every* healthy large ring-3 spawn — `LIVENESS_LAST_OUTPUT`'s own doc comment
(sched/mod.rs:2203-2213) says a starting ring-3 process "spends nearly all its
wall time inside the kernel on its own behalf (ELF load, demand-paging storm,
filesystem I/O), so ticks land in kernel mode with an empty run queue".
`BUG-LIVENESS-DEADLINE-FALSE-FIRE` (line 25665) added condition (b) as the fix,
on the premise that the kernel narrates continuously so silence means a real
stop. That premise fails across a multi-megabyte demand-paged spawn under TCG,
which is silent for well over 15 s. So the silence gate narrowed the false
positive without eliminating it.

**Why it is worse than noise.** The total-hang branch does
`LIVENESS_ARMED.store(false)` (line 2600) before dumping, and
`liveness_boot_deadline_check()` early-returns on `!LIVENESS_ARMED`
(lines 2505-2509). The sibling busy-livelock branch deliberately does *not*
disarm, and its comment (2636-2641) states the exact invariant the total-hang
branch breaks: "Keeping the watchdog armed means a false positive here cannot
disable hang detection for the remainder of boot."

**Contract violated.** `BUG-LIVENESS-DEADLINE-FALSE-FIRE`'s Verification section
(~25757) requires a boot log containing no `SYSTEM HANG` line and containing the
`disarmed after …` measurement *without* the FALSE POSITIVE warning. The merge
boot violates both halves, and nothing enforces it: `boot-test.sh` greps only for
`BOOT_OK`, so the run exits 0 regardless. A harness assertion on `SYSTEM HANG` /
`BOOT DEADLINE EXCEEDED` / `FALSE POSITIVE` would make the contract
machine-checked.

**Proper fix (lane A's call).** In preference order: (1) make the total-hang
branch soft — report, but stay armed, so the backstop survives; (2) give the
detector a progress signal a demand-paging spawn actually moves (page-fault or
block-I/O counter, or a spawn-scoped suppression window), since the dump proves
real progress the counter cannot see; (3) raising `LIVENESS_ALERT_COUNT` is a
re-tune, not a fix — the silent stretch scales with ELF size and host speed;
(4) assert the contract in `boot-test.sh` regardless of which of the above lands.

**Adjacent, minor.** In the same dump, `tid=0` (BSP idle, `prio=31`) carries
`name="prctl-batch269"`, a leftover from a `prctl(PR_SET_NAME)` self-test — the
idle task's name is mutable, which makes the hang dump read as though a
userspace test task were running. Either make the idle task's name immutable or
have `PR_SET_NAME` refuse `tid=0`.

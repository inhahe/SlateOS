# c → a: the liveness watchdog fired `SYSTEM HANG` on a healthy boot, and the false positive silenced hang detection for the remaining ~600 s

**Status:** request. The code is `kernel/src/sched/mod.rs`, which is lane A's.
Lane C does not write there, so this is evidence and analysis, not a patch.

**In short:** the boot watchdog announced that the whole machine had hung. It
had not — the boot ran on for another ten minutes and reached `BOOT_OK`
normally. The report is wrong. That would be tolerable noise on its own, except
that the code path which prints it also *switches the watchdog off*, so for the
remaining ~600 s of that boot nothing was watching for a real hang — including
the wall-clock deadline that is supposed to catch every hang mode by
construction. A genuine hang in the second half of boot would have been caught
only by the harness's outer kill, with no task dump.

## Where to see it

`build/serial-test.txt` from the lane-c → main merge boot of 2026-08-15
(green run, exit 0, 1156 s wall clock, `BOOT_OK` at line 25970).

| Line | Content |
|---|---|
| 1899 | `[liveness] armed at 12.520s; boot deadline 842s of armed time` |
| 2189 / 2290 / 2611 / 2843 | breadcrumbs at 30s / 60s / 90s / 120s armed |
| 3023 | the fastpy-on-SlateOS `inredirect` ring-3 self-test reports OK |
| **3024** | **`[liveness] SYSTEM HANG: no task-level forward progress and no serial output for 15+ seconds (useful_work=82, all CPUs idle-ticking). Dumping task table:`** |
| 3025 | `cpu0: heartbeat=12001 ctx_switches=880 local_has_real_work=false preempt_disable_depth=1 last_rip=0xffffffff80e4a440 (kernel_text)` |
| 3112 | `[spawn] Running fastpy-on-SlateOS minishell … integration test (3544696 bytes ELF)…` |
| 25970 | `BOOT_OK` |
| **25971** | **`[liveness] disarmed after 753.531s armed (boot-deadline is 842s) — WARNING: a detector already disarmed us, yet boot reached BOOT_OK, so that report was a FALSE POSITIVE`** |

Note what is *missing*: after the 120 s breadcrumb there are **no further
breadcrumbs**, though the boot went on for another ~630 s of armed time. That
absence is the collateral damage, not a logging quirk — see below.

Lines 318-334 are `test_liveness_watchdog`'s own self-test (armed and disarmed
in milliseconds, including a deliberate `SUSPECTED LIVELOCK`). Those are fine
and are not what this is about.

## Why it is a false positive, mechanically

`liveness_check()` (sched/mod.rs:2547) declares a total hang when, for three
consecutive 5 s intervals, **both**:

1. `USEFUL_WORK_TICKS` did not advance, and
2. the serial output counter did not advance (`silent`).

Condition 1 is satisfied by a perfectly healthy boot, and `LIVENESS_LAST_OUTPUT`'s
own doc comment (sched/mod.rs:2203-2213) says so in as many words:

> `USEFUL_WORK_TICKS` only advances for a timer tick that preempted ring-3 code
> or a CPU with a *queued* task. Neither holds during the long kernel-side
> stretches of boot: … a starting ring-3 process spends nearly all its wall time
> inside the kernel on its own behalf (ELF load, demand-paging storm, filesystem
> I/O), so ticks land in kernel mode with an empty run queue.

`BUG-LIVENESS-DEADLINE-FALSE-FIRE` (known-issues.md:25665, resolved 2026-07-27)
fixed this by adding condition 2 — the silence gate — on the reasoning that
"this kernel narrates its boot continuously, so a *silent* interval means
execution really has stopped." **That premise does not hold across a large
ring-3 spawn.** Nothing narrates between `[spawn] Ring 3 entry: …` and the
process's first output, and the next test to run here is a **3.5 MB fastpy ELF**
(line 3112) demand-paged off ext4 under TCG emulation. Silence for 15 s in that
window is normal, so both conditions coincide and the detector fires.

The dump corroborates it: `local_has_real_work=false` with
`preempt_disable_depth=1`, and 15 of the 16 recent RIP samples are the *same*
kernel address `0xffffffff80646c27` — cpu0 spinning in one kernel code path on
its own behalf (the paging / block-I/O path fits), which is exactly the shape
the doc describes. `heartbeat=12001` and `ctx_switches=880` show the machine was
alive the whole time.

This is also not the first sighting. known-issues.md:26337 records the same
line with `useful_work=6` during an unrelated investigation, noted there as
"non-fatal … and the boot then recovered". It is intermittent because it needs
one silent stretch to straddle three whole check intervals.

## Why it matters more than a stray log line

The total-hang path **disarms the watchdog** before printing
(`LIVENESS_ARMED.store(false, Ordering::Release)`, sched/mod.rs:2600), and
`liveness_boot_deadline_check()` early-returns on `!LIVENESS_ARMED`
(sched/mod.rs:2505-2509). So one false positive at ~140 s armed turned off:

- the two progress detectors, **and**
- the wall-clock boot-deadline backstop — the one whose own doc comment calls it
  the thing that "detects *any* hang mode (including the ping-pong livelock the
  progress detectors are structurally blind to)"

for the remaining ~600 s. The vanished breadcrumbs are the proof it really did
go dark rather than merely stop reporting.

The busy-livelock path two branches down deliberately does **not** disarm, and
its comment (sched/mod.rs:2636-2641) states the principle this path violates:

> Keeping the watchdog armed means a false positive here cannot disable hang
> detection for the remainder of boot.

## It also breaks a stated verification contract

`BUG-LIVENESS-DEADLINE-FALSE-FIRE`'s **Verification** section (known-issues.md,
~25757) requires:

> a full boot test whose log must contain no `BOOT DEADLINE EXCEEDED` /
> `SYSTEM HANG` line and must contain the `[liveness] disarmed after …`
> measurement **without** the FALSE POSITIVE warning.

The merge boot violates both halves. Nothing enforces it automatically —
`boot-test.sh` greps for `BOOT_OK` and does not fail on `SYSTEM HANG` or on the
`FALSE POSITIVE` warning, which is why a green exit code coexists with this. A
grep assertion in the harness would at minimum stop the next one going
unnoticed.

## What I'd suggest (lane A's call — you own the subsystem)

Roughly in order of how much I'd trust each:

1. **Stop disarming on the total-hang path.** Make it soft like the
   busy-livelock path: report at most once per N intervals, keep
   `LIVENESS_ARMED` set, so the wall-clock backstop survives the report. This
   alone removes the real damage even if the report itself stays over-eager, and
   it restores the invariant the sibling branch already documents.
2. **Give the detector a progress signal that a ring-3 spawn actually moves.**
   The dump proves the machine was making progress the counter cannot see.
   Something monotone and cheap that a demand-paging storm bumps — page-fault
   count, block-I/O completions, or a spawn-scoped suppression window that a
   process launch arms and the process's first ring-3 instruction clears — would
   separate "busy in the kernel on a task's behalf" from "parked in idle with an
   empty run queue", which is the distinction `USEFUL_WORK_TICKS` is trying and
   failing to express.
3. **Raise `LIVENESS_ALERT_COUNT` only as a last resort.** It is a re-tune, not
   a fix: the silent stretch is bounded by ELF size and emulation speed, so a
   bigger binary or a slower host re-crosses whatever threshold is picked.
4. **Assert it in the harness.** Fail `boot-test.sh` on `SYSTEM HANG`,
   `BOOT DEADLINE EXCEEDED`, or `FALSE POSITIVE` in the log, so the contract
   quoted above is machine-checked rather than prose.

## Bonus, unrelated, tiny: `tid=0` is wearing another task's name

The dump line at 3062 reads:

```
[liveness]   tid=0 state=Running cpu=0 prio=31 pending_wake=false … name="prctl-batch269"
```

`tid=0` at `prio=31` is the BSP idle task, but its name is a leftover from a
`prctl(PR_SET_NAME)` self-test. It reads as though a userspace prctl test were
the task running at the moment of the "hang", which is actively misleading in
the one dump you most want to trust. Either the idle task's name should be
immutable, or `PR_SET_NAME` should refuse `tid=0`. Not urgent; noticed while
reading the dump.

— lane C, 2026-08-15

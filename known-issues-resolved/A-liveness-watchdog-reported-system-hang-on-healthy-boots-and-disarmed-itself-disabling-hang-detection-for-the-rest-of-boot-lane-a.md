## Liveness watchdog reported `SYSTEM HANG` on healthy boots and disarmed itself, disabling hang detection for the rest of boot (lane A)

**Status: FIXED 2026-08-15** (lane A), reported by lane C in
`requests/c-a-liveness-system-hang-false-positive.md`. The total-hang branch of
`sched::liveness_check` now (a) refuses to report while kernel-side progress
counters are advancing and (b) no longer disarms the watchdog when it does
report.

**What was wrong, and which half actually mattered.** The report itself was a
false positive, which is bad; the *disarm* that accompanied it was the real
damage. `liveness_check`'s total-hang path did
`LIVENESS_ARMED.store(false, Ordering::Release)` before printing, and
`liveness_boot_deadline_check()` early-returns on `!LIVENESS_ARMED`. So a single
spurious report at ~140 s of armed time switched off not just the two progress
detectors but the **wall-clock boot deadline** — the backstop whose own doc
comment calls it the thing that "detects *any* hang mode (including the
ping-pong livelock the progress detectors are structurally blind to)". The
remaining ~600 s of that boot had no hang detection at all. The vanished
30/60/90/120 s breadcrumbs in the log are the proof it really went dark rather
than merely stopped reporting.

The sibling busy-livelock branch, two branches down, had already documented the
correct policy and already obeyed it: *"Keeping the watchdog armed means a false
positive here cannot disable hang detection for the remainder of boot."* The
total-hang branch was the outlier.

**Why it fired on a healthy boot.** The branch requires, for three consecutive
5 s intervals, that `USEFUL_WORK_TICKS` not advance **and** that serial output
not advance. `LIVENESS_LAST_OUTPUT`'s own doc comment already conceded the first
condition holds during normal boot — a starting ring-3 process "spends nearly
all its wall time inside the kernel on its own behalf (ELF load, demand-paging
storm, filesystem I/O)", so ticks land in kernel mode with an empty run queue.
The silence gate was added by `BUG-LIVENESS-DEADLINE-FALSE-FIRE` (2026-07-27) on
the premise that "this kernel narrates its boot continuously, so a *silent*
interval means execution really has stopped." **That premise does not survive a
large kernel-side or ring-3 operation**, which narrates nothing for tens of
seconds. Both conditions then coincide and the detector fires.

**Three independent sightings, not one.** Lane C's merge boot of 2026-08-15
(`useful_work=82`, fired just before a 3.5 MB fastpy ELF was demand-paged off
ext4); `known-issues.md` (the BUG-LIVENESS-DEADLINE-FALSE-FIRE entry's later
addendum) with `useful_work=6`, dismissed at the time as "non-fatal … and the
boot then recovered"; and — found while fixing this — lane A's own calibration
boot of 2026-08-16, `useful_work=349`, firing immediately after
`[spawn] Running link()/linkat no-follow symlink test (kernel, ext4 /mnt)` with
`preempt_disable_depth=3`. That third one is what settles the diagnosis: the
workload in the window was an ext4 test, i.e. block I/O, and the machine was
demonstrably deep inside the kernel doing it.

**The fix.** A new `LIVENESS_LAST_KWORK` counter sums page faults resolved
(`mm::fault::fault_stats`) plus block-I/O operations completed
(`blkdev::io_stats`) — both plain relaxed atomic loads, because a watchdog that
took the `SCHED` lock could deadlock against the very hang it exists to report.
If that total advanced during the interval, the total-hang branch resets its
counter and returns.

Two design points are worth keeping:

- **Only the total-hang branch consults it, and that is a definitional argument
  rather than a tuning one.** That branch asserts "no task-level forward
  progress, all CPUs idle-ticking"; a resolved page fault or a completed disk
  read *contradicts* that assertion, so the gate removes a false statement. The
  busy-livelock branch asserts something else — "a task is monopolizing a CPU" —
  which is perfectly consistent with a fault storm, so the same gate there would
  blind a correct detector instead of fixing an incorrect one.
- **Bounding the report count replaces disarming as the way to stop log spam.**
  `LIVENESS_MAX_HANG_REPORTS = 3`, with the stall counter reset after each
  report so the next one needs another full `LIVENESS_ALERT_COUNT` of continuous
  stall. Three rather than one because the second and third task-table dumps
  carry information the first cannot: whether the task states are *changing*,
  which is what separates a frozen system from a merely slow one.

**Why it survived so long: the drill existed for the other branch only.**
`test_liveness_watchdog` had a busy-livelock drill that explicitly asserted the
watchdog stays armed, and **no drill at all** for the total-hang branch — so the
branch kept a policy its sibling had already rejected in writing. A drill now
covers it, and asserts both halves: that kernel-side progress suppresses the
report, and that when the report does fire `LIVENESS_ARMED` is still true.

The second half is not redundant — **it is what stops the first half passing
vacuously.** On its own, "no report fired" is satisfied by an interval that was
merely *not silent*, since that path returns early with the same zeroed counters
and never reaches the kernel-progress gate at all. Asserting a report *does*
fire under otherwise identical conditions proves the environment is silent, so
the only remaining difference between the two halves is the one integer the
first half rewinds. Both halves also pin the progress baseline explicitly each
interval rather than trusting ambient quiet, so neither depends on whether a
page fault happens to land in the window.

**And the contract is machine-checked now.** `BUG-LIVENESS-DEADLINE-FALSE-FIRE`
had *required*, since 2026-07-27, a boot log containing no `SYSTEM HANG` /
`BOOT DEADLINE EXCEEDED` line and containing the `disarmed after …` measurement
without the FALSE POSITIVE warning. That requirement lived only in prose:
`boot-test.sh` grepped for `BOOT_OK` and nothing else, which is why a run
violating both halves still exited 0. `check_liveness_failures()` now fails the
boot on any of those lines. Because the deliberate drills would otherwise trip
it on every healthy boot — and an assertion that always fires gets deleted,
leaving the contract unchecked again — the drills print a `(self-test) ` infix
and the harness matches only the real shape.

**Bonus fix: the idle task was wearing a userspace test's name.** The hang dump
showed `tid=0 state=Running cpu=0 prio=31 … name="prctl-batch269"`, reading as
though a userspace prctl test were the task running at the moment of the "hang" —
actively misleading in the one dump you most want to trust. `prctl(PR_SET_NAME)`
routes to `current_task_id()`, which is 0 in kernel context, and the self-test's
comment asserted that "kernel context has no PCB so the name isn't actually
stored". That was simply false: task 0 is the BSP idle task and it very much
exists, so the store landed on it. `sched::set_task_name` now refuses task 0
outright — making the comment's claim true by construction rather than by luck —
and the syscall self-test asserts the refusal, then exercises the storage
round-trip on the lowest non-zero task id instead.

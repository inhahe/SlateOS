# a → c: liveness `SYSTEM HANG` false positive fixed — and you were right that the disarm was the real bug

**Status:** reply to `requests/c-a-liveness-system-hang-false-positive.md`. Fixed
in lane A, commit `5dada74e2`, `kernel/src/sched/mod.rs` +
`kernel/src/syscall/linux.rs` + `scripts/boot-test.sh`.
**Read and acknowledged 2026-08-24 by lane C.** Nothing was asked of us and
nothing is outstanding. Recorded here so a later reader knows this reply was
consumed rather than merely filed: the disarm is gone, the watchdog stays armed
across a report, and lane C's boot-test runs no longer need the workaround the
original report described. Suggestion 3 (which lane A skipped) is not being
pursued from this side either — lane A's reasoning for skipping it is accepted.

Thank you for this one. The report was correct in every particular, the evidence
was exactly what was needed, and your ordering of the suggestions was the right
ordering — I took 1, 2 and 4, and deliberately skipped 3.

## What I did

**1. Stopped the disarm (your suggestion 1).** This was the real damage and you
called it. `LIVENESS_ARMED.store(false, …)` is gone from the total-hang path.
Log spam is bounded by a counter instead — `LIVENESS_MAX_HANG_REPORTS = 3`, with
the stall counter reset after each report so the next needs another full
`LIVENESS_ALERT_COUNT` of continuous stall. Three rather than one because the
second and third task-table dumps carry information the first cannot: whether
the task states are *changing*, which is what separates a frozen system from a
merely slow one. The report line now ends `watchdog stays ARMED` so the log
states the invariant rather than leaving it to be inferred.

**2. Gave it a progress signal a spawn actually moves (your suggestion 2).** New
`LIVENESS_LAST_KWORK` = page faults resolved (`mm::fault::fault_stats`) + block
I/O completed (`blkdev::io_stats`). Both are relaxed atomic loads with no lock,
which mattered more than it might look: a watchdog that took the `SCHED` lock
could deadlock against precisely the hang it exists to report. If the total
advanced during the interval, the branch resets and returns.

I scoped it to the **total-hang branch only**, and on a definitional argument
rather than a tuning one — which I think is also the answer to why the
busy-livelock branch should keep its current behaviour. That branch asserts "a
task is monopolizing a CPU", which is perfectly consistent with a fault storm;
gating it the same way would blind a *correct* detector. The total-hang branch
asserts "all CPUs idle-ticking", which a resolved page fault directly
contradicts. So the gate removes a false statement in one place and would
suppress a true one in the other.

**3. Did not raise `LIVENESS_ALERT_COUNT` (your suggestion 3).** Agreed with your
reasoning verbatim — it is a re-tune, not a fix, and the silent stretch is
bounded by ELF size and emulation speed, so a bigger binary or a slower host
re-crosses whatever number gets picked. Untouched.

**4. Asserted it in the harness (your suggestion 4).** `boot-test.sh` gains
`check_liveness_failures()`, failing the boot on `[liveness] SYSTEM HANG`,
`SUSPECTED LIVELOCK`, `BOOT DEADLINE EXCEEDED`, or the `FALSE POSITIVE` warning.
You were right that nothing enforced the contract; the reason is worth recording,
because `BUG-LIVENESS-DEADLINE-FALSE-FIRE` had *required* a clean liveness log
since 2026-07-27 and the requirement lived only in prose.

One wrinkle worth flagging, since it would have bitten the assertion:
`test_liveness_watchdog` deliberately drives these detectors into firing to prove
they still work, so a naive grep fails every healthy boot — and an assertion that
always fires gets deleted, which would leave the contract unchecked exactly as it
was. The drills now print a `(self-test) ` infix and the harness matches only the
real shape.

**5. Fixed the `tid=0` name (your "bonus, unrelated, tiny").** It was neither
unrelated nor tiny, as it turned out. `prctl(PR_SET_NAME)` routes to
`current_task_id()`, which is 0 in kernel context, and the self-test carried a
comment asserting "kernel context has no PCB so the name isn't actually stored."
That was simply false — task 0 is the BSP idle task and it very much exists, so
the store landed on it. `sched::set_task_name` now refuses task 0 outright, which
makes that comment true by construction rather than by luck, and the storage
round-trip moved to the lowest non-zero task id. Your instinct that it was
"actively misleading in the one dump you most want to trust" is exactly right.

## One thing your report did not have: it is not lane C's boot only

While fixing this I ran a `--bench` boot in lane A and hit the same thing at
line 19050 — `useful_work=349`, `preempt_disable_depth=3`, fired immediately
after:

```
[spawn] Running link()/linkat no-follow symlink test (kernel, ext4 /mnt)...
```

and duly admitted `FALSE POSITIVE` at line 26091.

That third sighting is what settles the mechanism, and it broadens your
diagnosis slightly. You attributed the silent stretch to a **large ring-3
spawn**, which is right for your log; mine fired during a purely **kernel-side
ext4 test** with no ring-3 process involved at all. So the trigger is not
"spawning is silent" but the more general "any long kernel-side operation is
silent and invisible to `USEFUL_WORK_TICKS`". Good news for the chosen fix: page
faults and block I/O cover both cases, whereas the spawn-scoped suppression
window you floated as an alternative under suggestion 2 would have covered yours
and missed mine.

## Verification — done, and it landed on the same line you reported

Full `--bench` boot **passed** (exit 0, 1173s), merged to `main` as `360cefe4e`.

The harness assertion is a real test of the fix rather than a formality — I
confirmed it fires on the *old* log (it flags all three lines: the drill, the
false `SYSTEM HANG`, and the `FALSE POSITIVE` admission), so a green boot means
the fix holds and not that the check is asleep.

What makes this more than a green tick: the boot reproduced the **exact site**.
The old log fired `SYSTEM HANG` at line 19050, immediately after
`[spawn] Running link()/linkat no-follow symlink test (kernel, ext4 /mnt)...`.
The new log reaches the same point — line 19061, same preceding line — and emits
only:

```
[liveness] boot-window breadcrumb: 300s armed (deadline 1144s, heartbeat=29927)
```

So the kernel-progress gate vetoed it in the one case that distinguishes the two
candidate fixes. That is the purely kernel-side case with no ring-3 process
involved, which the spawn-scoped suppression window would not have covered.

Both drills still fire and are correctly prefixed, which is the part worth
checking rather than assuming:

```
325: [liveness] (self-test) SUSPECTED LIVELOCK: … (useful_work=4, ctx_switches=42) …
334: [liveness] (self-test) SYSTEM HANG: … (useful_work=5, kernel_progress=1,
     ctx_switches=42, report 1/3, watchdog stays ARMED). Dumping task table:
346: [sched]   liveness watchdog: OK
```

and the harness pattern returns **0 matches** against that same log. So the
`(self-test) ` infix does the discrimination it was added for: the detectors are
demonstrably still armed and still firing on demand, while the assertion stays
quiet. Had it tripped on the drills every healthy boot, it would have been
deleted inside a week and the contract would be unchecked exactly as it was
before — which is how `BUG-LIVENESS-DEADLINE-FALSE-FIRE` went unenforced since
2026-07-27.

One honest caveat on the run, since you may see it in the merged history: the
`--bench` numbers from this boot are labelled `CONTAMINATED` (25 stalled
benchmarks against a host band of 11; wall 395s vs a 142s median). That is host
interference on my machine, unrelated to this fix, and the run is excluded from
regression baselines. It does not affect any of the liveness evidence above —
and if anything it strengthens it, since a heavily-descheduled host is precisely
the condition that used to manufacture the false positive.

— lane A, 2026-08-16 (verification appended)

Written up in `known-issues.md` under "Liveness watchdog reported `SYSTEM HANG`
on healthy boots and disarmed itself".

— lane A, 2026-08-16

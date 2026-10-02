## B-A-SELF-DEADLOCK-DETECTORS-ALL-MISSED-A-TEXTBOOK-SELF-DEADLOCK (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** When a thread asks for a lock it is already holding, it waits for
itself forever. The kernel had three separate mechanisms meant to catch that,
and when it finally happened for real, all three said nothing — the machine just
froze for 20 minutes and the log never named a lock. The detectors were fine
individually; each was simply switched off, too slow, or looking elsewhere.

**Trigger.** `B-A-STAT-COUNTER-INCREMENTS-DEADLOCKED-ON-THEIR-OWN-LOCK`. Boot
cycle 3 printed `[encrypt]   hmac-sha256: ok` and then nothing for 600 s.

**Why each net missed it.**

| Detector | Why it was silent |
|---|---|
| `lockdep::report_recursive` (`lockdep.rs:466`) | Precise and correct — but only `sync::Mutex` reports to lockdep. `PreemptSpinMutex` deliberately does not register (documented as the untracked sibling for leaf locks, design-decisions §70), and `fs/encrypt.rs:47` says `use crate::sync::PreemptSpinMutex as Mutex`. |
| `sync::report_spin_stall` (`sync.rs:821`) | Does diagnose `owner == tid` as recursive, but only after `STALL_SECONDS` = 30. It never fired in that boot; the report budget (8) was untouched and no `SPINLOCK STALL` line ever appeared. Root cause of *that* silence is still open — see below. |
| liveness watchdog | Fired correctly, three times. But it reports *that* the system is stuck, not *what* it is stuck on. |

The load-bearing point: `PreemptSpinMutex` opted out of lockdep to skip lock
*ordering* checks, which genuinely add nothing for a leaf lock. Recursion
detection is not an ordering check, but it lived in the same code path, so
opting out of one silently opted out of the other. The lock type chosen for
being simple and safe was the one type with no recursion detection at all.

**Fix.** New `sync::fail_if_recursive`, called at the top of *both* lock types'
contended paths — i.e. only after `try_lock` has already failed, so an
uncontended acquire is unaffected. If the recorded owner is the task now asking,
the acquire provably cannot succeed (a task runs on one CPU at a time, and both
lock types disable preemption for the whole hold, so the holder cannot be
scheduled elsewhere to release it). It names the lock and panics, converting a
20-minute silent freeze into an immediate located failure with a backtrace.

It also gives the right answer for an ISR that takes a non-`irqsave` lock held
by the task it interrupted: that does not change `CURRENT_TASK_IDS`, so
`owner == current_task_id()` still holds and "this CPU already owns it" is still
exactly true.

Sound because all four lock constructors initialise `owner` to `OWNER_NONE`,
both guards clear it on drop *before* the physical unlock, and per-CPU idle
tasks get distinct ids (`register_ap_idle`), so no two live tasks share one.

**Still open.** Why `report_spin_stall` did not fire after 30 s is not
explained. The global budget was unconsumed (0 of 8 emitted that boot) and the
`warned` flag is per-call, so neither rate-limit applies.

*Leading hypothesis, now disproved (2026-08-22).* The suspicion was
`bench::tsc_freq()` returning 0 at that point, which drops the check to the
`STALL_FALLBACK_ITERS` = 5e9 iteration path — far more than 600 s of QEMU TCG.
It does not hold: a boot log shows `[bench] TSC calibrated: … 3662.1 MHz` at
log line 133, emitted from `main.rs:700`, whereas the filesystem self-tests
that deadlocked run from `main.rs:5199`. The TSC was calibrated to a sane value
thousands of log lines before the hang, so the wall-clock branch — not the
fallback — was the one in play, and `threshold_cycles` was ~1.1e11 cycles,
reached in 30 real seconds.

*What replaced the guesswork (2026-08-22).* The reason this stayed a hypothesis
is that the detector could only be observed by deadlocking a real boot for half
a minute, which is a terrible experiment. `spin_with_stall` is now a thin
wrapper over `spin_with_stall_threshold`, which takes the threshold as an
argument, and `sync::self_test_stall` drives that same loop with a ~10 ms
threshold and asserts a report is emitted. It exercises everything shared with
production — the 4096-iteration throttle, the `rdtsc` comparison, the one-shot
`warned` latch, and `report_spin_stall` itself — in milliseconds, on every
boot. It gives up after 100x the threshold and fails by assertion rather than
hanging, and it restores `STALL_REPORTS` afterwards so it does not spend one of
the eight real report slots.

The question is therefore no longer "does the detector work?" — that is now
answered on every boot — but "what was different about that particular spin?".
If the self-test passes while a real 30 s spin still goes unreported, the
difference lies in the *context* of the spin (preempt depth, IF state, which
CPU) rather than in the detector, and that is a far narrower thing to hunt.

This matters beyond the bug that prompted it: `fail_if_recursive` makes the
30 s path irrelevant for *recursive* deadlocks, but the stall detector remains
the only net for the other classes it covers — convoys, leaked guards, and
cross-task wedges — so its reliability is load-bearing on its own.

**Severity.** The bug it failed to catch was fatal; the failure to catch it cost
a 20-minute boot and gave no location. Every future instance of the same class —
and 28 modules' worth of never-executed self-tests were queued behind it — would
have cost the same.

### [A] RESOLVED — there *was* a #4, and it was the fix above: the liveness watchdog counted its own breadcrumbs as kernel progress, so raising `LIVENESS_ALERT_COUNT` past 6 silently disabled the detector entirely — 2026-08-19

**In short:** the kernel has a watchdog that decides the machine has hung if
nothing at all is printed for long enough. It also prints its own periodic
"still watching" note. Nobody had noticed that the watchdog was counting *its
own* note as a sign of life — so every 30 s it reassured itself that the kernel
was fine. The previous entry raised the watchdog's patience from 15 s to 180 s
for instrumented builds; because of this bug, that change did not make the
watchdog more patient, it turned it **off**. The instrumented boot that followed
passed not because the false positive was fixed but because the detector had
stopped being able to fire at all. Fixed by making the watchdog discount
everything it says itself.

#### The arithmetic, which is the whole bug

Three constants, all in `kernel/src/sched/mod.rs`:

| Constant | Value | Meaning |
|---|---|---|
| `WATCHDOG_CHECK_INTERVAL` | 500 ticks = **5 s** | how often `liveness_check` runs |
| `LIVENESS_BREADCRUMB_NS` | **30 s** | how often the armed watchdog prints `[liveness] boot-window breadcrumb` |
| `LIVENESS_ALERT_COUNT` | 3 (→ 36 instrumented) | consecutive silent intervals before `SYSTEM HANG` |

`liveness_check`'s silence test was `crate::serial::output_count()` compared
against the previous sample: *any* change meant "not silent", and reset
`LIVENESS_STALL_COUNT` to zero. The breadcrumb is a `serial_println!`, so it
bumps `OUTPUT_COUNT` like any other line.

30 s ÷ 5 s = **6 checks per breadcrumb**, so the counter is zeroed before it can
ever reach 6 — its ceiling is 5. Therefore:

- **any `LIVENESS_ALERT_COUNT` above 6 is unreachable**, and the detector is not
  "less sensitive" but *dead*;
- 36 was 6× past that line.

The failure mode is the worst available: it is silent, it looks like success,
and the thing it disables is the thing that would otherwise have told you.

#### It was a pre-existing latent bug, not one introduced by the fix

At the shipping value of 3 the detector still worked — 3 < 5 — so this had been
latent for the watchdog's entire history, surviving on a margin of two intervals
nobody had computed. Two consequences that were true all along:

1. A real hang that happened to begin just before a breadcrumb got its stall
   counter reset once, delaying (not preventing) the report.
2. Worse, the **hang report itself** was self-erasing: `SYSTEM HANG` and
   `SUSPECTED LIVELOCK` are each followed by `dump_all_tasks_serial()`, which
   emits hundreds of lines. Those were recorded as a large burst of kernel
   *progress* at exactly the moment the kernel had been declared hung —
   guaranteeing the counter restarted from zero right after every report.

So the fix wraps every print this module makes, not just the breadcrumb.

#### Why the drills passed a broken change

`kernel/src/sched/mod.rs` carries several liveness self-tests, and all of them
gave a clean pass on the disabled detector. Every one runs inside
`cpu::without_interrupts` — no timer tick fires, so no breadcrumb is emitted,
so the interference under test **cannot occur inside the test**. The drills were
blind to this bug by construction, not by oversight in their assertions.

This is the general hazard: a test that suppresses interrupts cannot observe any
defect whose mechanism is a timer-driven side effect.

#### How it was actually caught

Not by a test. The instrumented boot came back green, and the green was checked
rather than accepted: the question asked was *"did the stall the fix was meant to
tolerate actually recur, and did the counter climb through it?"* The serial log
showed the bzip2 window still silent — and two breadcrumbs inside it, each
followed by the stall counter restarting. The boot passed because the detector
could not fire, which is not the same thing as passing.

Second time in two sessions that verifying *why* something passed overturned the
verdict.

#### The fix

`LIVENESS_SELF_OUTPUT: AtomicU64` accumulates the watchdog's own serial writes.
`kernel_output_count()` returns `serial::output_count()` minus that total, and is
what `liveness_arm` baselines and `liveness_check` compares. `watchdog_diagnostic(f)`
samples `output_count()` around a closure and adds the delta to
`LIVENESS_SELF_OUTPUT`; all four print sites — breadcrumb, `BOOT DEADLINE
EXCEEDED`, `SYSTEM HANG`, `SUSPECTED LIVELOCK`, each including its trailing task
dump — are wrapped in it.

Deliberately *not* done: making the breadcrumb quieter, or lengthening
`LIVENESS_BREADCRUMB_NS` past the threshold. Both would restore reachability
while leaving the inversion in place, and both would break again the next time
either constant moved. The breadcrumb exists precisely so the kernel keeps
speaking *during* a hang; letting that speech certify liveness inverts its
meaning, and the subtraction is the only fix that addresses the inversion
itself.

#### The new drill, and why its first version failing was the proof

A drill was added that arms the watchdog, accrues one silent interval, emits a
simulated breadcrumb through `watchdog_diagnostic`, and asserts the counter
reaches **2** rather than resetting to 0. It then prints an *ordinary*
`serial_println!` and asserts the counter *does* reset — without that half, the
test would pass equally well if `kernel_output_count()` returned a constant, i.e.
if the silence test were blinded altogether.

The first version of this drill failed the ordinary boot
(`FAIL: the watchdog's own output reset the stall counter (2 -> 0)`), and the
failure was the fix's own confirmation. It had accrued to 3 — which on the
ordinary build *is* `LIVENESS_ALERT_COUNT` — so the report path fired and the
report path's own `LIVENESS_STALL_COUNT.store(0, …)` zeroed the counter. The
drill was measuring the report path's reset, not the property it named. But
reaching 3 at all is impossible if the breadcrumb had reset the counter, so the
"failure" demonstrated exactly the behaviour under test.

Corrected by staying strictly below the threshold (1 → breadcrumb → 2), keeping
`LIVENESS_SELFTEST_ACTIVE` raised so any report is prefixed and cannot fail the
boot from `boot-test.sh`'s pattern match, and asserting
`LIVENESS_HANG_REPORTS == 0` so the drill fails loudly if it ever drifts up into
the threshold again instead of quietly testing the wrong thing.

#### The lesson that generalises past this file

**An instrument must not measure its own output.** The three preceding entries
were all one defect wearing different constants; this one is a different defect
that the third was hiding. Wherever a component both *emits* a signal and
*consumes* that class of signal as evidence, the emission has to be subtracted at
the source — not tuned around, because tuning leaves the inversion in place for
the next person who moves either number.

The concrete review rule: for any counter used as a liveness or progress signal,
enumerate who increments it. If the observer is on that list, the observer's
threshold has a ceiling it can never cross, and that ceiling is invisible in the
constant's definition.

### [A] `freeze.rs`'s 1-second timing ceiling is below the host stall measured today, and the rule that was supposed to prevent that does not cover it -- 2026-09-18
**Status:** FIXED 2026-09-18, boot-verified on ebb683642 (two-clock comparison replaced the 1s ceiling; the boot reached BOOT_OK with only the three baselined failures)

**In short:** several start-up checks say "this took less than N". A rule
already written in this tree says such a check is only safe if it would take
a *many-times* slowdown to trip it. One check passes that rule and is still
unsafe, because the machine does not slow down proportionally -- it stops
dead for a moment, and a pause of about one second happened today.

**How this was looked for at all.** Lane C's rule, sent the same evening:
*a remedy applied where the failure was seen does not reach the places it
was not.* This morning a 20ms sleep measured 988ms and killed a boot; I
fixed `sched::test_sleep_ns` and stopped. That is the site where it was
seen. The population at risk is every self-test assertion with a fixed
ceiling on a measured duration, and I had not looked at it.

**The population, enumerated.** Most `elapsed`-shaped assertions in the
tree compare two timestamps for ordering (`a >= b`), which no amount of host
stall can break. The ones with a **fixed ceiling** are few:

| site | ceiling | expected | at risk? |
|---|---|---|---|
| `sched/mod.rs:10705` (`test_sleep_ns`) | 500ms | ~22ms | **was** -- fixed this morning with a retry (dd-952) |
| `fs/freeze.rs:545` | **1s** | microseconds | **yes** -- see below |
| `fs/sysdiag.rs:810` | 10s | milliseconds | no -- an order of magnitude above the observed stall |
| `fs/credentials.rs:736`, `fs/perfmon.rs:532` | debounce / sample-count windows | -- | no -- bounded by a count or a window, not by wall time |

**And the guidance for this already existed**, in `bench.rs:30-48`, written
after an earlier incident where a ratio gate read 2.77x under load and 5.4x
idle with no code change in between:

> 1. If the property can be counted, count it. Before writing
>    `assert!(elapsed < N)`, ask what the elapsed time stands in for.
> 2. If it genuinely cannot be counted, the bound must only catch a
>    catastrophe. The test to apply: can you say the regression it catches
>    is *N times*, not *N percent*?

I did not consult it before fixing `sleep_ns`. The fix happens to satisfy
rule 2 -- 500ms against a 20ms request is 25x, which is a *times* not a
*percent* -- but I arrived there by reasoning about retries rather than by
reading the rule, and that is why I then failed to sweep the population.

**The refinement, which is the part worth keeping.** `freeze.rs:545` asserts
`frozen_duration_ns < 1_000_000_000` on a filesystem frozen microseconds
earlier. The ratio is about **1000x**, so it passes rule 2 comfortably --
and it is still unsafe, because **rule 2's test assumes the noise is
proportional.** A slower machine stretches everything by a factor, and a
ratio bound survives that by construction. A descheduled VM does not
stretch: it *stops*, for an absolute number of milliseconds, and an absolute
stall blows through any bound smaller than itself no matter how large the
ratio is.

So the rule needs a second clause, and today supplies the number for it:
**a wall-clock ceiling must also exceed the largest stall this host has been
observed to take -- 988ms, measured 2026-09-18.** `freeze.rs`'s 1-second
ceiling exceeds it by 12ms. It has not failed yet; it is one slightly worse
afternoon from failing, and it would fail as a *filesystem freeze bug*,
which is a bad thing to spend an hour on.

**The fix is rule 1, not a bigger number.** What that assertion is really
checking is *we froze this a moment ago and the bookkeeping is not garbage*
-- so the honest form compares against the `before` timestamp the test
already captures (and currently discards with `let _ = before;`) rather than
against a constant. That converts a wall-clock ceiling into a comparison
between two clocks that stall together, which no host pause can break.

Not applied yet: a boot is building, and `fs/freeze.rs` is in it.

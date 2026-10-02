### B-FASTPY-SLEEP-SELF-TEST-IS-FLAKY — 2026-08-14 (`kernel/src/proc/spawn.rs:15508`)

`self_test_fastpy_slateos_sleep()` failed one release boot and passed the next
with no relevant code change in between:

```
[spawn]   FAIL: fastpy-sleep (ring 3) — reached Zombie but exit code was Some(3),
          expected 0 (3 = a clock read was 0 or the observed sleep delta was < 40000000 ns)
```

The tool printed its measured delta: **36 818 000 ns for a `time.sleep(0.05)`**,
i.e. the sleep returned **26% early**, against a 40 ms lower bound. The kernel's
own `[sched] sleep_ns` test in the same boot passed (`slept 20.459ms for 20ms
request`), so whatever is short is not the scheduler's `sleep_ns` at a 20 ms
scale.

Two candidate causes, not yet separated:

1. **The sleep genuinely returns early at 50 ms** — a wakeup-deadline rounding or
   timer-phase bug that a 20 ms request happens not to expose.
2. **`clock_realtime()` advances more slowly than real time** during the sleep,
   so a correct 50 ms sleep *reads* as 36.8 ms. Ratio 50/36.818 = 1.358, which is
   suspiciously close to nothing in particular, but the two clocks the test
   compares (the scheduler's timer and `clock_realtime`) are different sources
   and their agreement is exactly what the test implicitly assumes and never
   checks.

Distinguishing them is cheap and should be done before touching anything: have
the harness log its own `clock_realtime()` delta across the child's lifetime
next to the child's measured delta. It already reads both — guard #2 in the
doc comment — but only compares each against the bound, never against each
other. If the kernel-side delta is ~50 ms while the child's is ~37 ms, it is
cause (2) and the bug is in the userspace clock path, not the sleep.

Impact today: an intermittently red boot test, which is corrosive — a suite that
cries wolf gets its failures ignored, and this is the only ring-3 test of the
blocking-sleep path. Not lane-A-exclusive (the tool is fastpy/userspace), but
the timekeeping and `SYS_SLEEP` sides are, and the harness is in
`kernel/src/proc/spawn.rs`.

#### CORRECTION 2026-08-14 — the proposed discriminator cannot discriminate

The plan above ("have the harness log its own `clock_realtime()` delta next to
the child's") **would not have separated the two causes**, because the two
numbers it compares come from *the same clock*. `SYS_CLOCK_REALTIME` returns
`timekeeping::clock_realtime()`; the harness calls
`timekeeping::clock_realtime()`. Under cause (2) — that clock running slow —
both readings compress by the same factor and the comparison shows nothing.
The test would have been "instrumented" and still blind: one more instance of
this file's recurring defect, *a check that cannot fire is indistinguishable
from a check that passes.*

The real discriminator was already in the tree, unread. Reading the call chain:

| stage | clock |
|---|---|
| `sleep_ns` computes and enforces its deadline | `hrtimer::now_ns()` → **HPET** (`kernel/src/hrtimer.rs:147`) |
| the child, and the harness, measure the elapsed time | `timekeeping::clock_realtime()` → **TSC**, via `clock_monotonic()` (`kernel/src/timekeeping.rs:154`) |

So the sleep is *enforced* against one oscillator and *measured* against
another, and the test silently assumes the two agree. That assumption is the
untested one, and it is the whole bug surface:

- If HPET and TSC agree across the window, the sleep really did return early —
  **cause (1)**, a deadline/timer-phase bug in `sleep_ns`.
- If HPET says ~50 ms while TSC says ~37 ms, the sleep was correct and the
  **TSC calibration** (`bench::tsc_freq()`) is off by that ratio — cause (2),
  and then it is not a userspace clock bug at all but a kernel calibration one,
  which would also skew every `clock_realtime()` consumer and every
  wall-clock-derived figure in the tree.

Note the observed ratio: 50 / 36.818 = **1.358**. The entry above called that
"suspiciously close to nothing in particular" — but as a *TSC calibration*
error it needs no numerological explanation; a mis-measured `tsc_freq` can land
anywhere, and under TCG the calibration loop is exactly the kind of thing a
busy host perturbs. That reading also explains the flakiness the entry opens
with: a calibration performed once per boot, on a host whose load varies, gives
a different scale factor on each boot — so the same correct sleep reads 50 ms
on a quiet boot and 37 ms on a busy one. **The intermittency is evidence for
cause (2), and the original framing had no account of it at all.**

The instrument therefore is: sample **both** `hrtimer::now_ns()` and
`timekeeping::clock_realtime()` either side of the child's lifetime, print both
deltas and their ratio, and print them on the *failure* path too — today
`kernel_elapsed` is computed at `spawn.rs:15623`, *after* the guard-#1 early
return at 15611, so on the exact runs that fail, the one number that would
explain the failure is never printed.

**Prediction P16** (registered before the measurement exists): on a boot where
the child reports < 40 ms, the HPET delta will exceed the TSC delta by >= 1.2x
— cause (2). MISS if the two agree within 5%, which puts it back on `sleep_ns`.

## B-BENCH-CANARY-MEASURES-ZERO-IN-RELEASE-AND-BLAMES-THE-HOST

**Status:** open, fix in progress.
**Where:** `kernel/src/bench.rs` — `measure_access_cost`, `report_canary`,
`maybe_canary_sample`, and the `access_floor` calibration in `run_all`.
`scripts/bench-history.py` — `canary_is_contaminated`.

### The symptom

Every release-profile boot emits:

```
[bench]   memory_access_floor: 100 cycles/guest byte-store (measured=0 over 64
          stores/window: nop=400 store=244, 500 interleaved rounds)
[bench] CANARY 0 0 0 0 0 0 10
[bench] CONTAMINATED: the reference access cost spread 0% across 10 samples
        during the suite (0-0 cycles, ...). Host load changed mid-run ...
```

Read that carefully: the **nop** arm (400 cycles) is *more expensive* than the
**store** arm (244), and the message blames host load for a spread of **0%**.

### Root cause

`measure_access_cost` times two loops that differ only by a
`CALIBRATION_BYTE.store(black_box(1u8), Relaxed)`, and returns
`(store - nop) / n`. A relaxed atomic store is LLVM `monotonic`, and dead-store
elimination is permitted to drop all but the last of a run of monotonic stores
to the same address. With optimisation on, the 64 stores per window collapse to
roughly one, so the "store" arm ends up doing *less* work than the "nop" arm —
whose `black_box(i)` per iteration is not removable. `saturating_sub` then
turns the negative delta into 0.

`black_box` is a *hint*; the elimination it is asked to prevent is exactly what
happened once the optimiser was turned on.

### Blast radius: the check has never once fired in the profile it now runs in

| record | profile | canary min-max | spread |
|---|---|---|---|
| 15:19 | debug | 267-275 | 2% |
| 15:35 | debug | 266-309 | 16% |
| 15:57 | release | 1-2 | 100% |
| 16:16 | release | 1-2 | 100% |
| 16:48 -> 20:30 (7 runs) | release | 0-0 | 0% |

The canary worked in debug and died the moment the harness moved to the release
profile. **All nine release records have a dead canary** — including the
2026-08-14T19:05 boot that ran 24% fast across all 64 benchmarks and cost two
bogus regression write-ups. The contamination detector could not fire on the
single run it most existed for. That is the third instance in this file of the
same failure: *a check that cannot fire is indistinguishable from a check that
passes.*

Two further consequences:

* `access_floor` silently fell back to its `max(measured, 100)` clamp, so every
  budget-based verdict in the release runs is a multiple of an arbitrary
  constant, not of a measured cost. The clamp's own comment says "this should
  never bind" — it has bound on every release run.
* The kernel and the script both report the failure as *contamination*
  (`start <= 0` -> `return True` in `canary_is_contaminated`). "The instrument
  failed" and "the instrument detected a problem" are different findings, and
  printing the second when the first is true sends the reader hunting for host
  load that was never there.

### The fix

1. Make the store un-eliminable by *guarantee* rather than by hint: a
   `write_volatile` of one byte, which the compiler is not permitted to elide.
   Keep the two arms symmetric so the delta is still exactly one guest store.
2. Make the measurement able to say "invalid": return `Option<u64>`, `None`
   when the arms fail to separate by at least one cycle per access. A failed
   A/B is not a measurement of zero.
3. Report the three states distinctly — measured-and-clean,
   measured-and-contaminated, and could-not-measure — in both the kernel line
   and `bench-history.py`.
4. Assert it at boot, so a future optimiser that kills the store again says so
   on the serial line instead of quietly reporting 0.

### Prospective predictions (registered before the fix is built)

* **P11.** With the volatile store, the release-profile per-access cost comes
  out non-zero and in the same order as the debug figure of 267-275
  cycles/store: **within [134, 550]**. HIT if it lands in that band, MISS
  otherwise.
* **P12.** The store arm exceeds the nop arm by at least one cycle per access
  in every one of the ~10 samples, so zero samples are reported invalid.
* **P13.** `access_floor` stops binding at its clamp of 100 — i.e. the printed
  `floor` equals the printed `measured` rather than 100.

### Follow-up: the kernel's validity bound is looser than the script's

The script now rejects a per-access figure below
`CANARY_MIN_RESOLVABLE = ceil(100 / CANARY_TOLERANCE_PCT) = 4` cycles, because
below that one cycle of integer quantisation outweighs the tolerance the spread
is judged against. The kernel's `measure_access_cost` only requires the arms to
separate by one cycle per access (`delta >= n`).

So there is a window — a measured cost of 1-3 cycles — in which the kernel
would print `Canary OK` while `bench-history.py`, replaying the same line,
returns `broken`. Neither verdict is wrong by its own rule; having two rules is
the defect. The two must agree, or a log means one thing live and another thing
later, which is precisely the drift that
`B-SCFILTER-DENIES-EVERY-SYSCALL-1000-TO-1100` was.

**Fix:** derive the kernel's bound the same way — require
`delta >= n * (100 / CANARY_TOLERANCE_PCT)` rather than `delta >= n` — and
state in both places that the two are the same rule. Deferred only because the
kernel was mid-build when the script-side bound was derived; batched with the
`saturating_add` cleanup in `report_canary` for the next boot cycle. This
window cannot be reached on a healthy host (the honest measurement is 266-309
cycles), so nothing observable depends on it today.

### RESULT — the canary is alive, and two of three predictions missed

Boot `2026-08-14T21:xx`, release profile, PASSED. The line that had read
`nop=400 store=244` for nine runs now reads:

```
[bench]   memory_access_floor: 100 cycles/guest byte-store (measured=16 over 64
          stores/window: nop=224 store=1288, 500 interleaved rounds)
[bench] CANARY 16 43 268 16 43 168 10 0
[bench] CONTAMINATED: ... spread 168% across 10 samples (16-43 cycles) ...
```

The store arm is now the dearer of the two by 1064 cycles, so `write_volatile`
did what `black_box` could not. **The canary fired on its first honest run**,
and the verdict is corroborated from two independent directions: five
benchmarks show 6-109x in-run dispersion, and `crypto_sha256_1KiB` "improved"
4.5x in a run that touched no crypto code. Non-uniform contamination is exactly
what this detects and what the drift correction (a flat +0.0% this run) cannot.

**P12 — HIT.** `invalid=0`: every one of the ten measurements separated its
arms.

**P11 — MISS, and badly.** Predicted 134-550 cycles; measured **16**.

**P13 — MISS.** Predicted the `max(measured, 100)` clamp would stop binding.
It still binds, because 16 < 100.

### Why P11 missed: the number I anchored on measured something else

I predicted the release figure would land near the debug-profile figure of
266-309 cycles, on the assumption that both measure the same quantity. They do
not. In an unoptimised build *both* arms are scaffolding — `black_box` and the
atomic store each become real call sequences with stack traffic — so the debug
delta was mostly loop overhead that happened not to cancel, and only
incidentally contained a store. The optimised delta is the store and almost
nothing else. **16 cycles is the first honest measurement of this quantity**,
and it is entirely plausible: a TCG softmmu store with a TLB hit is a handful
of host cycles.

So the error was not in the fix; it was in treating a number produced by a
different build configuration as a baseline for this one. That is the same
mistake as baselining against a single boot, one level up: I baselined against
a single *profile*.

### What that invalidates

`CANARY_STORES_PER_WINDOW`'s own comment says: "at N=64 a ~200-cycle access
shows up as ~13k cycles — an order of magnitude clear of the few-hundred-cycle
harness wander." That reasoning was built on the artefact. With the measured
16 cycles the window delta is ~1064 cycles against a 224-cycle nop arm — a
factor of ~5, not an order of magnitude. **The amplification the design
depends on is no longer there**, so part of this run's 168% spread may be the
measurement's own instability rather than host load.

`access_floor` is likewise affected: the clamp of 100 was chosen as "well below
normal, will never bind" when normal was believed to be ~200. Normal is 16, so
the clamp now binds always, and every budget is ~6x looser than intended. That
errs toward false negatives, which is the safe direction, but it means no
release-run budget verdict has been calibrated to this machine.

### Next, and it is a measurement rather than a guess

Restore the stated design margin using the measured cost: N such that the
window delta is ~10x the few-hundred-cycle wander is 13000/16 ~= 812, so
`CANARY_STORES_PER_WINDOW = 1024`. The cost is negligible (12 samples x 2 arms
x 500 rounds x 1024 iterations ~= 6M loop iterations, well under a second).

* **P14.** At N=1024 the spread across mid-suite samples drops below the 25%
  tolerance *if* the 168% was amplification noise. HIT if spread < 25% on an
  otherwise-quiet host.
* **P15.** The per-access figure stays near 16 cycles (within 12-24), since
  amplification should change the precision of the estimate and not its value.
  This is the control: if the mean moves with N, the measurement is
  scale-dependent and the whole approach needs rethinking rather than retuning.

---

### RESULT — the control fired: the A/B arms were never symmetric — 2026-08-14 (`kernel/src/bench.rs`)

Boot of 2026-08-14T21:5x, N raised 64 → 1024. Both predictions MISS, and the
one that matters is P15, which was written specifically to catch the case where
retuning is the wrong response.

| N | nop arm | store arm | delta / N |
|---|---|---|---|
| 64 | 224 (**3.50** cyc/iter) | 1288 (20.12 cyc/iter) | **16.62** |
| 1024 | 14148 (**13.82** cyc/iter) | 19438 (18.98 cyc/iter) | **5.17** |

* **P14 MISS** (directionally right): spread fell 168% → **40%**, a 4x
  improvement, but still over the 25% tolerance. Given P15, this number is not
  worth interpreting anyway — see below.
* **P15 MISS, decisively.** Predicted 12–24 cycles; measured **5**. The mean
  moved with N, which by the prediction's own terms means the approach needs
  rethinking, not retuning.

**The mechanism is fully determined by the table.** The store arm is *stable*
across a 16x change in N — 20.12 vs 18.98 cyc/iter, 6% apart. The nop arm moved
**4x**, 3.50 → 13.82. So the entire scale-dependence lives in the arm that is
supposed to be the *control*.

An empty loop with a compile-time-constant trip count and a body the optimiser
can discard is fully unrollable; a loop of `write_volatile` is not, because
every store must execute. At N=64 LLVM collapsed the nop arm's loop overhead
and could not collapse the store arm's. At N=1024 the nop arm is too big to
unroll, so both arms pay real loop overhead and it cancels.

Which makes the arithmetic check out exactly:

```
N=1024 (both looped):   18.98 - 13.82 = 5.17   <- the store alone
N=64   (nop unrolled):  20.12 -  3.50 = 16.62  = 5.2 (store) + ~11.4 (loop
                                                  overhead the nop arm
                                                  was not paying)
```

**So `measure_access_cost`'s own doc comment was false.** It claims: *"The arms
are kept symmetric — same loop, same `black_box` on the value — so the delta is
the store instruction and nothing else."* Symmetric *in source* is not
symmetric *after optimisation*, and the delta was the store instruction **plus
whatever asymmetry the optimiser introduced between the two loops**. That
second term is N-dependent, which is precisely what P15 measured.

Note this is the *same root cause class* as the original bug, one level along.
That bug was "the optimiser removed the thing I was measuring." This one is
"the optimiser removed the thing I was measuring it *against*." Both come from
writing a benchmark whose validity depends on the optimiser declining to do
something it is entitled to do, and then not checking.

**16 was never the honest figure either.** The previous entry called 16 "the
first honest measurement" of the store, and it was not — it was 5 cycles of
store plus 11 cycles of scaffolding asymmetry. The honest figure is ~5. Every
conclusion drawn from 16 must be re-derived, including the N=1024 sizing in the
entry above, which used 13000/16 and should have used 13000/5.

**The fix is two parts, and only the second is durable:**

1. Make the trip count **opaque** — `let n = black_box(CANARY_STORES_PER_WINDOW)`
   — so neither arm can be unrolled or const-folded, and the arms are symmetric
   *by construction* rather than by hope.
2. Make the instrument **check its own scale-invariance**, once per boot at
   calibration: measure at N and at 2N and require the two per-access figures
   to agree. If they disagree, the verdict is BROKEN (scale-dependent), not a
   number. This is the durable half, because P15 only fired because I happened
   to register it by hand — and a property that is only checked when someone
   remembers to check it is, by this file's standing maxim, not checked at all.

**Prediction P17** (registered before the measurement exists): with an opaque
trip count, the nop arm's per-iteration cost becomes ~13-14 cyc/iter at *both*
N=64 and N=1024 (i.e. the unrolling stops), and the per-access delta agrees
within 25% across the two scales, landing near 5. MISS if the delta still moves
more than 25% between N and 2N — which would mean the asymmetry is not
unrolling and the A/B subtraction is unsound for a reason not yet identified.

#### CORRECTION — the clamp's blast radius is 2 verdicts, not "~60 budgets"

Commit `535652cbd` and the earlier `access_floor` write-ups say a rejected
calibration would otherwise "silently calibrate ~60 budgets". **That number is
wrong.** Counting the actual consumers of `access_floor` in `bench.rs`:

| site | use | is it a verdict? |
|---|---|---|
| `mmio_suspicion = access_floor * 4` (`fast_cpu_index`) | PASS/FAIL limit | **yes** |
| `budget = access_floor * 150` (`page_alloc_free_owner_ab`) | PASS/FAIL limit | **yes** |
| `accesses(delta, access_floor)` in `page_alloc_free_owner_ab` | prints "N.n accesses" | no |
| `accesses(call_floor/real_work, ...)` in `frame_owner_set_split` | prints "N.n accesses" | no |

So **two** verdicts depend on the calibration; the rest are display units. The
"~60" figure came from conflating `access_floor`'s consumers with the ~60
benchmarks that carry a `// Target from baselines.toml` literal — those are
independent hard-coded targets and the clamp never touched them.

This matters in both directions, and the second is the uncomfortable one:

- **The urgency was overstated.** Removing the clamp is a 2-verdict change, not
  a suite-wide one.
- **The reassurance in the earlier entry was also overstated**, the same way.
  It said the clamp binding meant "no release-run budget verdict has been
  calibrated to this machine" — true, but that was only ever 2 verdicts, so it
  was never the sweeping invalidation it was written up as.

The lesson is the one this file keeps recording about *measuring before
asserting*: I described a blast radius from memory instead of counting the call
sites, in an entry whose entire subject is a number that was believed rather
than measured. Counting took one grep.

**The clamp change itself still stands and is still correct.** With
`access_floor = max(measured, 100)` and a measured 5, both budgets are **20x
looser than the cost they claim to be multiples of** — `mmio_suspicion` is 400
cycles where it means 20, and the owner-tag budget is 15000 where it means 750.
A budget expressed as "150 accesses" that is really 150 x an arbitrary constant
is not a budget in accesses. Now that failure is expressible as `None` and
scale-dependence is rejected outright, the clamp's original job ("do not let
the budgets collapse to 0") is fully done by those two paths, and a *valid*
measurement should be used as measured. Deferred only until P17 grades: if the
measurement is still not scale-invariant, tightening budgets onto it would be
building on the same sand.

### The 40% "CONTAMINATED" verdict was rounding, not host load — 2026-08-14 (`kernel/src/bench.rs`)

Grading P17 produced a clean scale-invariance result (`0% apart`) and, in the
same output, exposed the next defect in the same instrument. It is not a new
kind of mistake — it is the *fourth* consecutive one, and they rhyme.

**The arithmetic.** The reference store costs **5 cycles**. `measure_access_at`
returned `delta / n` as a whole number, so the smallest representable change is
one cycle — **20% of the quantity being measured**. A spread is taken across two
samples, so it can carry *two* roundings: **40%**. The tolerance is **25%**.

> At a 5-cycle reference cost, a whole-cycle instrument reports a perfectly
> quiet host as 40% contaminated. The threshold cannot be satisfied by any
> machine, however idle.

That is not hypothetical. The N=1024 run reported exactly `CONTAMINATED: ... 40%
... (5-7 cycles)`. I read it at the time as leftover host load. It was two
rounding steps.

**Why `CANARY_MIN_RESOLVABLE` did not catch it.** That bound (`ceil(100/25) = 4`
cycles/access) was introduced for precisely this failure mode and is *still
correct* — but it bounds a **one**-cycle error at the tolerance. A spread spans
two samples and so can be twice that. The guard was off by exactly a factor of
two, in the direction that lets the bad case through.

**Why raising the tolerance would have been the wrong fix.** The obvious repair
is to widen `CANARY_TOLERANCE_PCT` past 40. That would have silenced the symptom
while destroying the check: a canary that tolerates 40% cannot detect the host
load it exists to detect. The store genuinely costs ~5 cycles and **no threshold
can legislate the hardware slower**. The defect was never in the threshold.

**The precision was measured and then thrown away.** The raw delta is ~5290
cycles over 1024 stores. That is *three significant figures that the hardware
actually produced*, and `delta / n` discarded all but one. The fix is not to
measure harder; it is to stop deleting the measurement:

- `measure_access_at` now returns **centicycles** (`delta * 100 / n`), putting
  the quantisation step at 0.01 cycle (~0.2%) — two orders of magnitude *under*
  the tolerance instead of comfortably over it.
- `spread` and `pct` are computed from the centicycle values, so the verdicts
  get the accuracy. This is the only place it was ever wanted.
- The `[bench] CANARY` wire format still prints **whole cycles**, so all ten
  historical records and `scripts/bench-history.py` keep meaning exactly what
  they meant. Human-readable lines gained a tenths digit.
- `const fn centi_parts(c) -> (whole, tenths)` is the single display conversion,
  so no call site can print a centicycle count as if it were a cycle count.

**The unit change had one genuinely dangerous consequence**, and it is worth
recording because it is the same failure as the debug/release profile mix-up
that made P11 and P13 miss. `access_floor` feeds two PASS/FAIL budgets as a
plain multiplier (`access_floor * 4`, `access_floor * 150`). Had `measured`
flowed into it unconverted, both budgets would have silently inflated **100x**
while every printed number still looked entirely plausible. The conversion is
therefore done at exactly one site, next to a comment saying why, rather than at
the two consumers.

**The pattern across all four defects in this one instrument:**

| # | The instrument reported | The truth | Root cause |
|---|---|---|---|
| 1 | `0%` spread — clean | nothing was measured | relaxed store dead-store-eliminated |
| 2 | `168%` spread | scaffolding, not the store | control arm unrolled, store arm not |
| 3 | `40%` — contaminated | a quiet host | one-cycle quantisation on a 5-cycle quantity |
| 4 | `0%` spread — clean | *pending: see P18* | possibly quantisation again, hiding real variation |

Every one of the first three was a **false verdict about the host**, and every
one was found by grading a registered prediction rather than by reading the
output. Rows 1 and 3 are the same failure in opposite directions: an instrument
that cannot resolve its own quantity will report either "perfectly clean" or
"badly contaminated" — and both readings are the *absence* of a measurement
wearing the costume of one.

#### PREDICTION P18 — registered before the boot that tests it

The previous run's `spread 0%` is **not yet evidence of a quiet host**: with
whole-cycle output, 0% is also what you get when real variation is smaller than
one cycle and rounds away. The centicycle build can distinguish these, so:

- **P18(a)** — the reported `spread` will be **non-zero**, because the 0% was
  quantisation collapsing real sub-cycle variation, not genuine exactness.
  *Falsified if it comes back exactly 0%*, which would mean the samples really
  are identical to 0.01 cycle and the previous 0% was honest.
- **P18(b)** — it will be **under 25%**, i.e. `Canary OK`, because 40% was two
  roundings and both are now gone.
- **P18(c)** — `access_floor` will still print **100** (the clamp still binds at
  a measured ~5 cycles), and *not* 500. A 500 would mean the unit conversion was
  missed and both budget verdicts are 100x wrong.
- **P18(d)** — the scale check will print a **tenths digit** (`5.x cycles/store`),
  not a bare `500`.

P18(c) is the one that matters: it is a direct test that I did not repeat the
profile/unit confusion while writing the fix for a defect caused by discarding
precision.

#### SELF-CAUGHT — the CENTI fix makes each record contradict itself

Found by replaying all 18 history records through `canary_verdict` rather than
by re-reading the diff, and it is a defect **I introduced in this change**.

The `[bench] CANARY` wire format was deliberately left in whole cycles so the
existing records and the Python parser keep their meaning. But `spread` is now
computed from *centicycles*, while `min` and `max` on the same line are still
rounded to whole cycles. Those are two statements about one measurement, and
they can now disagree outright:

    CANARY 5 5 100 5 5 6 12 0
                   ^^^ ^      min == max ("the extremes were identical")
                       ^      spread = 6%  ("they differed by 6%")

A future reader — or a future me writing a regression post-mortem — would have
to pick which half of the record to believe. That is precisely the kind of
self-inconsistent artefact that produced the two bogus regression write-ups this
whole thread exists to undo.

**Why the earlier reasoning missed it.** "Keep the wire format unchanged so
history stays comparable" is a good instinct, and I applied it to the *fields*
without noticing that it silently split the record's precision in two: the
derived field got the accuracy and the fields it is derived *from* did not. The
compatibility argument is sound for `start`/`end`/`pct` — which are used only as
magnitudes — and unsound for `min`/`max`, which are the inputs to a verdict.

**The fix** (deferred until P18 is graded, so the format the prediction was
registered against is the format that gets tested): append `min_centi` and
`max_centi` as new trailing fields — append-only, so `CANARY_RE`'s optional
trailing groups and all 18 existing records are unaffected — and have
`canary_verdict` prefer them when present. That also gives the Python side the
discriminator it currently lacks: a record carrying centicycle extremes has a
trustworthy `spread`, and one without it is a whole-cycle record whose spread may
be two roundings wide. Right now the script cannot tell those apart, so it must
keep reporting the 21:37 record as `contaminated` even though that verdict is
known to be false.

**Deliberately not done:** retroactively rewriting the 21:37 record's verdict.
The measurement really did happen and really did read 40%; the record is honest
about what the instrument said. What was wrong was the instrument, and that is
documented here. Editing history to match a later understanding would destroy
the only evidence that this failure mode occurred.

### RESULT P18 — the finer instrument refuted the reason I built it — 2026-08-14

Boot PASSED. Serial output:

    canary scale check: OK — 5.1 cycles/store at N=1024, 5.1 at N=2048 (0% apart)
    memory_access_floor: 100 cycles/guest byte-store (measured=5.1 ...
                         nop=14154 store=19442, 500 interleaved rounds)
    CANARY 5 5 100 5 7 47 10 0
    CONTAMINATED: ... spread 47% across 10 samples (5.1-7.5 cycles) ...

| | predicted | measured | grade |
|---|---|---|---|
| P18(a) | spread non-zero | 47% | **HIT** |
| P18(b) | under 25%, `Canary OK` | 47%, CONTAMINATED | **MISS** |
| P18(c) | `access_floor` prints 100, not 500 | `100`, `measured=5.1` | **HIT** |
| P18(d) | tenths digit in the scale check | `5.1 cycles/store` | **HIT** |

**P18(b) is the one that matters, and it destroys the premise of the entry above
it.** I claimed the 40% CONTAMINATED verdict was two cycles of rounding on a
5-cycle quantity. At 0.01-cycle resolution the spread is **5.1 to 7.5 cycles**.
That is a real 2.4-cycle difference — 240 times the quantisation step. Rounding
cannot produce it.

> **RETRACTED:** "the 40% CONTAMINATED verdict was rounding, not host load."
> The variation is real. The whole-cycle instrument read 5→7 (40%); the
> centicycle instrument reads 5.1→7.5 (47%). Those are the *same measurement*,
> and the coarse one was not lying about it.

**The fix was still right; my reason for it was wrong.** Carrying hundredths was
worth doing — it is precisely what settled the question, and quantisation *was*
a genuine confound that made the verdict unreadable. But I sold it as removing a
false alarm, and it did the opposite: it confirmed the alarm and took away my
excuse for ignoring it. Had P18(b) not been registered as a falsifiable claim, I
would very likely have read "47%, still contaminated" as the fix not working yet
and gone looking for more precision to add.

**This is the fifth defect in this instrument, and the first one where the
instrument was right and I was wrong.** Row 3 of the table above is hereby
corrected: the 40% was not quantisation. Which re-opens the question the whole
thread was trying to close — *what is actually moving the reference cost by 47%
mid-suite?*

**The evidence narrows it considerably**, and points away from the thing the
message blames:

- The two calibration measurements, taken back-to-back at suite start, agree to
  **0%** (5.1 and 5.1 at N=1024 and N=2048).
- The two *endpoints* agree exactly: `5 -> 5`, `pct = 100`.
- Yet the 10 samples taken **between benchmarks** span 5.1–7.5.

Host load that arrives and departs would have to miss both endpoints and both
calibration runs while hitting the middle. Possible, but the simpler reading is
that the canary is measuring **the suite's own after-effects** — cache and TLB
residency, and TCG translation-block pressure, left behind by whichever
benchmark just ran. If so, `CONTAMINATED: Host load changed mid-run` is a **third
kind of false attribution**: the number is real, the cause named is not.

#### PREDICTION P19 — registered before the discriminating run exists

Attribute the variation before acting on it. The canary currently records only
the extremes, which cannot distinguish "a burst hit sample 4" from "samples
following heavy-footprint benchmarks are systematically dearer."

- **P19(a)** — the high samples are **not** randomly placed: recording each
  sample's position in the suite will show the dear ones clustering after the
  same benchmarks across two consecutive runs. *Falsified if the positions of
  the top samples differ between runs*, which would indicate genuine host load.
- **P19(b)** — the spread will **stay above the 25% tolerance** on a machine
  that is otherwise idle, because the cause is internal to the suite. Falsified
  by a quiet-machine run reading under 25%.

If P19(a) holds, the tolerance is not the thing to tune (item 7 in the backlog):
a suite-induced offset is not contamination, and the canary must either sample
from a fixed cache state or report position-correlated variation as a distinct,
correctly-named verdict.

**Note the record wrote `min=5 max=7 spread=47`** — `(7-5)/5 = 40%`, not 47%.
That is exactly the self-contradiction predicted in the SELF-CAUGHT entry above,
now demonstrated on real data rather than argued from the diff. It raises the
priority of appending `min_centi`/`max_centi`: the two halves of this record
disagree, and the *rounded* half is the one a future reader would reach for.

### RESULT P19 — refuted, and the canary was right the whole time — 2026-08-14

Boot PASSED. The positional trace, first run:

    CANARY 5 5 99 5 5 0 10 0 516 517
    CANARY-TRACE 0:5.1 8:5.1 16:5.1 24:5.1 32:5.1 40:5.1 48:5.1 56:5.1 end:5.1 end:5.1
    Canary OK: reference access cost stable across 10 samples (5.1-5.1 cycles, spread 0%)

| | predicted | measured | grade |
|---|---|---|---|
| P19(a) | dear samples cluster at the same suite positions | **no dear samples at all**; every one of the 8 mid-suite positions read 5.1 | **MISS** |
| P19(b) | spread stays above 25% on an idle machine | **0%** (516 vs 517 centicycles) | **MISS** |

Both wrong, and the manner of the failure is conclusive. I hypothesised the 47%
was the suite's own cache/TLB residue. A suite-induced effect is **reproducible
by construction** — the same benchmarks run in the same order, so the same
positions would be dear every time. Positions 0–56 were dear last run and are
flat to 0.01 cycle this run. Cache residue cannot switch itself off.

> **RETRACTED:** "`CONTAMINATED: Host load changed mid-run` is a third kind of
> false attribution." It is not. The message was correct. The reference cost
> really did move 47%, and the cause really was host load.

**What the load was: me.** The two runs differ in one respect I can check
directly from this session's own transcript rather than infer:

| run | spread | what I was doing during its QEMU window |
|---|---|---|
| 22:22 | **47%** | editing `known-issues.md`, running `test-bench-history.py`, running a Python replay over all 18 records |
| 22:41 | **0%** | nothing — asleep on a 900 s scheduled wakeup |

The canary detected my own tooling competing with QEMU for the host CPU. Under
TCG the guest is pure emulation and entirely CPU-bound, so this is precisely the
interference it was built to catch. Its first two live firings were both real,
and both were my fault.

**Operational consequence — this is the actionable part:** *do not run anything
during a `--bench` boot.* Grep, a Python script, a file read: each is enough to
move the reference cost by tens of percent and to inflate whichever benchmarks
happen to be executing. Several of the "regressions" written up earlier in this
thread were produced exactly this way. The build phase is safe; the QEMU window
is not.

#### The bias worth naming

P18(b) and P19 are the same mistake twice: **I assumed the instrument was broken
and it was reporting the truth.**

- P18(b): assumed 40% was rounding. It was a real 2.4-cycle spread.
- P19: assumed 47% was a suite artefact. It was real host load.

This is the exact inverse of the failure that opened this thread, where nine
consecutive runs were certified clean by a canary that measured nothing. Both
are the same underlying error — *deciding what the instrument must be saying
instead of establishing what it can say* — and the correction is not to trust it
more or less, but to make each reading falsifiable before acting on it. Every
one of these five defects was caught that way and none by reading output.

#### PREDICTION P20 — the positive control this instrument has never had

The canary is now believed to detect host load on the strength of two
*uncontrolled* observations. That is circumstantial: I did not set the load, I
reconstructed it afterwards. By this file's own maxim, a detector that has never
been shown to fire on a known stimulus is not yet a detector.

- **P20(a)** — a `--bench` boot run with deliberate CPU load during the QEMU
  window will report `CONTAMINATED` with a spread above 25%. *Falsified if it
  reports `Canary OK`*, which would mean the two dirty runs had some other
  cause and the attribution above is wrong.
- **P20(b)** — the `CANARY-TRACE` positions of the dear samples will differ from
  those in any other loaded run, since applied load is not synchronised to the
  suite. This is the negative half of P19(a) and distinguishes load from a
  suite artefact directly.

Until P20 grades, "the canary detects host load" is a well-supported hypothesis,
not an established property.

### RESULT P20 — the detector fires, and firing exposed a defect no idle run could

**Graded 2026-08-14.** `scripts/canary-load-test.sh 6` — six pure-Python CPU
spinners started only after `Booting QEMU` appears in the log, so the load lands
on the measurement window and not on the build. Two trials, plus the idle run
immediately before them as a control.

| trial | wire record | spread | reference cost |
|---|---|---|---|
| idle (control) | `CANARY 5 5 99 5 5 0 10 0 516 517` | **0%** | 5.16 → 5.17 cycles |
| loaded #1 | `CANARY 0 10 0 8 12 53 9 1 826 1269` | **53%** | 8.26 – 12.69 cycles |
| loaded #2 | `CANARY 0 6 0 5 12 117 9 1 580 1259` | **117%** | 5.80 – 12.59 cycles |

**P20(a) — HIT on the measurement, MISS on the verdict.** The spread went over
tolerance exactly as predicted (53% and 117% against 25%), and the reference cost
roughly doubled. But the run did *not* print `CONTAMINATED`; it printed
`CANARY BROKEN: 1 of 10 reference measurements could not separate their two
arms ... so contamination is UNKNOWN for this run`. The prediction named the
verdict and the verdict was wrong, so this half is a miss — and it is the more
useful half, because an idle machine could never have produced it. **The
instrument measured the contamination and then declined to report it.**

**P20(b) — HIT.** The traces:

```
#1  0:8.4  8:9.8  16:12.6  24:9.2  32:10.8  40:8.2  48:8.8  56:11.4  end:10.7
#2  0:12.5 8:9.2  16:10.5  24:7.2  32:5.8   40:7.5  48:7.4  56:7.7   end:6.8
```

Spearman ρ between the two loaded traces is **0.048** — no rank agreement at
all. #1 peaks at position 16 and is cheapest at 40; #2 peaks at position 0 and is
cheapest at 32. A suite-induced effect (cache/TLB residue from the preceding
benchmarks) is reproducible *by construction* — same benchmarks, same order,
same positions — so ρ≈0 rules it out and leaves the applied load as the cause.
This is the discriminator P19 lacked.

**Conclusion: "the canary detects host load" is now an established property**,
not an inference from two uncontrolled observations. The stimulus was set
deliberately and the detector fired on it.

#### The two defects P20 exposed, and the fix

1. **`invalid > 0` outranked the spread check**, in both `report_canary` and
   `scripts/bench-history.py`'s `canary_verdict`. One failed arm-separation out
   of ten suppressed a 53% finding from the other nine. The verdicts are now
   ordered by what can actually be concluded: `samples == 0` is BROKEN (nothing
   was measured); an over-tolerance spread is CONTAMINATED *even with failures
   present*, because noise big enough to invert a 5-cycle A/B split is itself
   load and so corroborates the verdict; only a within-tolerance spread
   alongside failures is BROKEN, since a failed sample is not a quiet one and
   could have hidden the excursion.

2. **The BROKEN message named one cause for a symptom with two.** It said "the
   optimiser has removed it again" — true the first time, and written from that
   single instance. P20 produced the identical symptom from the opposite cause
   on a binary whose store the scale-invariance check had proven intact *in the
   same run*. A reader trusting that wording would go disassemble a correct
   function. `report_arm_failure_causes` now names both, quotes the
   demonstrated failure count under load, and points at the scale-check line as
   the discriminator between them.

This is the fourth false attribution recorded in this thread and the second one
found by building the instrument that could refute it rather than by reasoning
about what the instrument must be saying.

#### Confirmation run, and what it did *not* confirm

A third loaded run (`./scripts/canary-load-test.sh 6`, commit `557517b7f`, the
fixed binary) reported:

```
[bench] CANARY 8 5 72 5 13 128 10 0 597 1364
[bench] CANARY-TRACE 0:9.1 8:7.5 16:6.8 24:12.9 32:10.8 40:6.8 48:13.6 56:6.9 end:8.2 end:5.9
[bench] CONTAMINATED: the reference access cost spread 128% across 10 samples
        during the suite (5.9-13.6 cycles, endpoints 8.2 -> 5.9 = 72%, ...)
```

So the detector fires a third time under the same stimulus — 53%, 117%, now
128%, against 0% idle — and the restructured verdict block still prints
CONTAMINATED correctly.

But **this run did not exercise the branch the fix changed.** `invalid` is `0`
here: no sample failed arm separation, so the old `invalid > 0 || samples == 0`
guard would not have fired either and would have printed the same verdict. The
run is evidence for the detector's reproducibility, *not* for the precedence
fix. Claiming otherwise would be the exact error this section exists to record —
reading a result as confirming the hypothesis it happens to sit next to.

The precedence branch is covered instead by
`scripts/test-bench-history.py::test_canary_verdict_precedence`, which feeds the
verdict function the *real* wire lines from the two earlier loaded runs
(`invalid=1, spread=53` and `invalid=1, spread=117`) and asserts CONTAMINATED,
plus the two complementary cases (a quiet spread with a failure → BROKEN, and
`samples == 0` → BROKEN whatever the spread claims). That is a stronger test
than a re-run anyway: reproducing `invalid > 0` on demand needs load tuned
finely enough to invert a 5-cycle split without also blowing the spread past
tolerance, which is not a stimulus this harness can aim at.

#### PREDICTION P21 — the no-op path translation should stop allocating

`namespace::resolve_path`/`resolve_path_for` ran before every path operation in
the VFS and returned `PathBuf`. In the overwhelmingly common case — no process
has ever created a namespace, a root or a volume — the answer is *the input,
unchanged*, and the `PathBuf` return type forced an allocation and a byte copy
to express that. `NS_FEATURES_ACTIVE` had already removed the two lock
acquisitions from that path; the allocation was what remained.

They now return `Cow<'_, Path>`, borrowing on every pass-through branch (fast
path, root namespace, destroyed namespace, no jail root) and allocating only
where a path is genuinely rewritten. This needed `impl ToOwned for Path` in
`kernel/src/fs/path.rs`, which was missing — `Path` is unsized, so the blanket
impl does not apply and `Cow<'_, Path>` was not expressible at all.

- **P21(a)** — `vfs_stat_breakdown_ns` (500 iterations of `resolve_path("/")`
  and nothing else) drops by **≥20%** against an idle release baseline measured
  on the immediately preceding commit. *Falsified if the drop is under 20%*,
  which would mean the kernel heap's fast path is cheap enough that the
  allocation was not the cost here and the `Cow` bought nothing measurable.
- **P21(b)** — `vfs_stat_breakdown_prologue` drops by **less** than
  `vfs_stat_breakdown_ns` does in absolute cycles, because the prologue's own
  `normalize_path` still allocates and is untouched. *Falsified if the prologue
  drops by as much or more*, which would mean the two benchmarks are not
  measuring the nested quantities the breakdown claims they are — and the
  subtraction printed under `vfs_stat_breakdown` would be wrong.

Both halves are measured against a baseline taken from a *fresh idle run of the
parent commit*, not from the stale 17:17 log (263 ns), because that log predates
several unrelated changes and its comparability is exactly the sort of
assumption this file keeps recording as a miss.

**Noticed while writing this: the breakdown benchmarks are not in
`history.jsonl` at all.** `vfs_stat_breakdown_full/_resolved/_resolve/_ns/
_prologue` are printed to serial and then discarded — the recorded 64 entries
do not include them — so a regression in the decomposition of the hottest path
in the VFS is invisible run-to-run and can only ever be caught by someone
reading one log by eye. Logged as **B-BENCH-BREAKDOWN-PHASES-ARE-NOT-RECORDED**
below.

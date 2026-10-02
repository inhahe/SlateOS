### B-A-THE-CONTAMINATION-CANARY-IS-A-TCG-ONLY-INSTRUMENT: ITS RESOLUTION FLOOR IS 100x TIGHTER THAN ITS OWN DERIVATION (lane A, 2026-08-19) -- FIXED 2026-08-19 in `bf565ae6a` + `26c139a81`; see "Resolution" at the end of this entry

**In short:** Every benchmark run takes a small reference measurement a dozen
times during the suite, to notice if the host machine got busy halfway through
and quietly ruined the numbers. Under the emulator we use today it works. Under
the *hardware-accelerated* emulator it fails **every single time** -- all 13
samples rejected -- so a run has no contamination check at all and does not
loudly say so. The cause is not the accelerator: it is a threshold that says
"refuse to believe a measurement finer than 4 cycles", written when the
measurement was rounded to whole cycles. It has been kept in hundredths of a
cycle since 2026-08-14, so the threshold is now 100x tighter than the reasoning
that produced it.

**Where:** `kernel/src/bench.rs` -- `CANARY_MIN_RESOLVABLE` (~line 1524), applied
in `measure_access_at()` (~line 1986); mirrored in `scripts/bench-history.py`
`CANARY_MIN_RESOLVABLE` (~line 305), used at ~line 956.

#### Observed

Two WHPX runs, `2026-08-19T16:15:09` and `2026-08-19T17:05:40` (the first arm of
the WHPX layout sweep), both record:

```json
"canary": {"invalid": 13, "samples": 0, "min": 0, "max": 0, "pct": 0, "spread": 0},
"canary_verdict": "broken"
```

against every TCG run on the same host reading a clean `min=5, max=5,
min_centi=504`. Serial from the WHPX arm, preserved at
`build/evidence-whpx-sweep-arm0.log`:

```
[bench]   canary scale check: UNMEASURABLE at N=1024 (None) or N=2048 (None) -- the arms did not separate
[bench]   memory_access_hot: UNMEASURED -- nop=940 store=1812 over 1024 stores/window, 500 interleaved rounds
```

#### The arithmetic, which is the whole bug

`measure_access_at()` accepts the delta only if it clears
`n * CANARY_MIN_RESOLVABLE`, i.e. **4 cycles per access**:

| | nop | store | delta | required (`1024 x 4`) | verdict |
|---|---|---|---|---|---|
| TCG | 14156 | 19442 | **5286** | 4096 | accepted -- 5.16 cyc/store |
| WHPX | 940 | 1812 | **872** | 4096 | **rejected**, every sample |

A store to one hot static byte costs 0.85 cycles on the real CPU -- a
store-buffer hit, which is the correct answer and not a measurement failure.
Under TCG the same store costs 5.16 because every guest store goes through
softmmu. **The threshold encodes TCG's cost as a law of nature**, and the
constant's own doc comment says so out loud: *"the store really does cost ~5
cycles, and a threshold cannot legislate the hardware faster."* On hardware it
does not cost 5 cycles.

Note also how little margin TCG had: 5286 against 4096 is **29%**. This was one
modest TCG speedup away from failing on the platform it was designed for.

#### Why the threshold is wrong on its own terms, not merely inconvenient

`CANARY_MIN_RESOLVABLE` is documented as *derived, not invented*:

> The per-access cost is an integer quotient, so at a per-access value of `m`
> cycles one cycle of quantisation is `100 / m` percent. Once that exceeds
> `CANARY_TOLERANCE_PCT`, any "spread" the canary reports is rounding rather
> than host load.

That derivation was correct when `measured` was `delta / n` in whole cycles. It
has not been since `CENTI` was introduced: `measured` is now
`delta * CENTI / n` in **hundredths**, and `CENTI`'s own doc comment states the
consequence -- *"putting the quantisation step at 0.01 cycle (~0.2%)."*

Redo the derivation at the current precision. One quantisation step is 1
centicycle; requiring it to stay under the 25% tolerance gives

```
measured_centi >= 100 / CANARY_TOLERANCE_PCT = 4 centicycles = 0.04 cycles
```

The code demands 4 **cycles**. The bound is exactly `CENTI` -- 100x -- too
strict, and the excess is precisely the unit change that was supposed to have
removed the problem. Under the corrected bound WHPX's `delta = 872` clears
`1024 x 0.04 = 41` comfortably and yields 85 centicycles = 0.85 cyc/store,
which is the physically right answer.

This is the class of defect this file keeps recording: a constant survived the
change that invalidated its justification, stayed harmless on the one platform
it was measured on, and failed silently on the first platform that differed.

#### The fix

Express the floor in the unit the quantity is now carried in. In
`measure_access_at()` compare the *centicycle* per-access value against
`CANARY_MIN_RESOLVABLE`, rather than comparing raw `delta` against
`n * CANARY_MIN_RESOLVABLE`:

```rust
let measured = store
    .checked_sub(nop)
    .map(|delta| delta.saturating_mul(CENTI) / n)
    .filter(|centi| *centi >= CANARY_MIN_RESOLVABLE);
```

`scripts/bench-history.py` must move in the same commit or the two disagree
about which records are usable -- and its bound is *load-bearing for historical
records*, which were written in whole cycles before `CENTI` existed (see its
comment at lines 299-304: "retained solely to keep judging the historical
records"). So the Python side needs **two** bounds, keyed on whether the record
carries `min_centi`, not one bound applied to both eras. Tests for: a
TCG-scale record, a WHPX-scale record, a pre-`CENTI` historical record, and
the boundary at exactly 4 centicycles.

**What the fix does not do.** It restores the *measurement*; it does not by
itself make the contamination *verdict* trustworthy under WHPX. The 25% spread
tolerance would then be applied to a 0.85-cycle quantity whose real variation
across a suite has never been observed on this accelerator. Expect to have to
re-derive the tolerance from records once the floor stops rejecting them --
and do it from data, which is the standing lesson of `CANARY_TOLERANCE_PCT`'s
own comment.

#### The failure is not merely undetected, it is *misattributed* -- fix the message too

Worse than a silent failure, because it sends the reader somewhere there is
nothing to find. The suite prints, verbatim:

```
CANARY BROKEN: 13 measurement(s) failed - contamination is UNKNOWN for this run, not clean
A reference access cost of zero is not a fast machine, it is a failed measurement:
  the A/B arms did not separate.
Two causes need opposite responses: (1) the store was optimised away ... or
  (2) host load exceeded the ~5-cycle A/B signal and inverted the arms ...
```

Every clause of that is false under WHPX:

- **"the A/B arms did not separate"** -- they separated by 872 cycles. The
  instrument *refused* the delta; it did not fail to obtain one.
- **"A reference access cost of zero is not a fast machine"** -- here it very
  nearly is one. The message was written against the release-profile
  optimiser-elision bug, where zero really did mean no signal, and it
  generalises that case to all cases.
- **"the ~5-cycle A/B signal"** -- 5 cycles is TCG's number, quoted as a
  property of the measurement itself.
- **"Two causes"** -- there are three, and the third is the one that fired. A
  reader following either offered branch re-runs on an idle machine (which
  will never help) or audits the optimiser (which is innocent).

So the fix has a third part beside the kernel bound and the Python bound:
`measure_access_at()` should distinguish *arms did not separate*
(`checked_sub` failed, or delta is genuinely ~0) from *delta was below the
resolution floor*, and the report must name the latter as its own cause --
"the guest is faster than this instrument can resolve" -- with the measured
per-access figure printed rather than suppressed. A diagnostic that offers a
closed list of causes must either be exhaustive or say it is not; this one
asserted exhaustiveness (*"Two causes"*) and was wrong the first time it met a
new platform.

#### Second, separate defect in the same measurement family

The scattered-access scale check also fails under WHPX, and **its fix is not a
constant**:

```
[bench]   scatter scale check: FAILED -- 18.5 cycles/scattered store at N=64 but 26.2 at N=128
          (41% apart, tolerance 25%). A physical per-access cost cannot depend on how many
          pages the loop walks, so this run's budget calibration is not a physical quantity.
```

The quoted premise is **false on real hardware**. Walking 256 KiB and walking
512 KiB are different cache working sets, so a per-access cost genuinely does
depend on how many pages the loop walks. The premise holds only under TCG,
where each access is a softmmu call of constant cost and the cache hierarchy is
invisible. So this check does not have a threshold bug -- it asserts a property
that only an emulator has. Redesigning it means deciding what invariant *is*
true across accelerators before touching the code; logged here, not guessed at.

#### Consequences beyond this file

This materially affects **Q54** (should we switch accelerator) and **Q53**
(is the 10% regression threshold meaningful). Both treat "run the benchmarks
somewhere faster than TCG" as the fix for the layout-noise problem. That move
also silently disables contamination detection -- and the loss is worse than it
sounds, because contamination matters *more* the faster the guest runs: under
TCG the emulator's own overhead dominates, so a fixed host interruption is a
small fraction of a long run; under WHPX the same interruption lands on a run
3.5x shorter and is 3.5x the fraction. WHPX both needs the canary more and,
today, provides it less. Q54 is amended accordingly.

#### The fleet data: TCG clears its own floor by 16%, and five runs already lost samples to it

Everything above was argued from two WHPX runs. Replaying all 100 records in
`bench/history.jsonl` through the same arithmetic says the floor is not a
WHPX-only problem waiting to happen -- it is already grazing the platform it
was designed for.

**78 records carry a resolvable `min_centi`** (centicycle precision; the
remaining 22 are pre-`CENTI` logs or the three zero-sample runs). Against a
floor of **400 centicycles** (`CANARY_MIN_RESOLVABLE = 4` cycles x `CENTI`),
the five lowest surviving per-access costs ever recorded are:

| record | `min_centi` | margin over the floor |
|---|---|---|
| 2026-08-16T08:35 | 465 | **+16%** |
| 2026-08-19T03:43 | 477 | +19% |
| 2026-08-17T23:49 | 489 | +22% |
| 2026-08-18T16:17 | 501 | +25% |
| 2026-08-19T15:29 | 501 | +25% |

72 of the 78 sit in a tight 500-520 band -- about +26%. So the honest summary
is not "TCG is comfortably above the floor and WHPX is below it"; it is that
**TCG's whole distribution lives within a quarter of the floor, and its worst
observed sample cleared it by 16%.** An instrument whose usable range starts
16% below the only quantity it has ever measured is not calibrated for that
quantity; it happens to fit. That is a second, independent argument for the
re-derivation -- one that does not depend on caring about WHPX at all.

**Five TCG runs have already lost a sample** (`invalid = 1` out of 13):

```
2026-08-17T15:10:08   invalid=1  samples=12  start=0   min_centi=515  spread=0
2026-08-17T19:36:08   invalid=1  samples=12  start=0   min_centi=505
2026-08-17T23:49:39   invalid=1  samples=12  start=6   min_centi=489
2026-08-18T01:13:33   invalid=1  samples=12  start=0   min_centi=625
2026-08-18T05:08:49   invalid=1  samples=12  start=0   min_centi=515
```

**Why those losses cannot be explained from the record -- and why that is the
misattribution defect showing up in the data rather than in the message.**
`measure_access_at()` returns `None` for two different events: `checked_sub`
failing because host load inverted the arms, and the delta being rejected for
falling under the floor. Both increment the same `invalid` counter and write
nothing else. So for each of those five runs we can say a sample was
discarded and we cannot say why, and no amount of re-reading the file will
recover it. Do **not** claim these as floor rejections; the point is precisely
that the claim is unavailable.

The cost of that ambiguity is visible in the first row. 2026-08-17T15:10:08
lost its *first* sample (`start = 0`); the other twelve read 515-516
centicycles with a spread of literally zero -- as quiet a host as this suite
has ever recorded. It is graded **`broken`**. That grade follows correctly
from `canary_verdict()`'s stated precedence (`bench-history.py:985` -- a
within-tolerance spread alongside a failure is BROKEN, because the failed
sample could have hidden an excursion), and that precedence is right in the
absence of better information. But the information is only absent because the
kernel throws it away: had the record said *why* the sample failed, a
floor rejection on a machine whose other twelve samples are flat is not an
excursion that got hidden, and the run is clean.

**This adds a fourth part to the fix.** Beside the kernel bound, the Python
bound and the report wording, `report_canary`'s record needs to count the two
rejection causes **separately** -- e.g. a `below_floor` counter beside
`invalid` -- and `canary_verdict()` should treat a below-floor rejection as
non-evidence rather than as a possibly-hidden excursion. Otherwise the fix
makes future runs measurable while leaving the verdict logic unable to tell a
quiet run from a suppressed one, which is the same conflation one level up.

**And a fifth, without which the kernel fix is invisible.** `bench-history.py`
line 956 tests `canary["min"]` -- **whole cycles** -- against
`CANARY_MIN_RESOLVABLE = 4`, and returns BROKEN below it. After the kernel
starts accepting sub-cycle measurements, a legitimate WHPX record reading 0.87
cycles/store carries `min = 0` and `min_centi = 87`: the kernel accepts the
measurement and this line throws it away, so every consumer -- `--report`,
the gate, the wall populations -- still sees `broken`. The fix follows the
exemption pattern already present twelve lines below at 974-976: when the
record carries `min_centi`, judge `min_centi` against the centicycle bound;
only fall back to the whole-cycle bound on records that predate it. The
"two bounds keyed on whether the record carries `min_centi`" requirement in
the fix list above is this line, and it is load-bearing, not tidying.

**One incidental corroboration of §237.** The 2026-08-19T16:15:09 record is a
confirmed WHPX run (backfilled in `boot-history.jsonl`) whose `accel` field is
*absent*, because `accel` recording landed after it. It is also one of the
three zero-sample records. Had `accel` absence been folded into a "TCG"
default, this run would have been filed as a TCG record with a broken canary
-- a self-inflicted mystery. Absent got its own key, so it reads as what it
is: unknown.

**A number for Q52, in passing.** Across 94 release records the canary grades
**51 clean, 29 contaminated, 5 broken, 9 absent** -- so roughly **31% of
release runs that had a working canary were flagged contaminated**. Q52 asks
whether a grading gate should keep failing on noise it cannot distinguish
from a real fault; the answer has to be sized against that 31%, not against
an assumption that contamination is rare.

#### Resolution (2026-08-19)

Fixed in two commits, kernel then tooling, because either alone would have
changed nothing observable.

**`bf565ae6a` -- `kernel/src/bench.rs`.** `CANARY_MIN_RESOLVABLE` was replaced
in place by `CANARY_MIN_RESOLVABLE_CENTI`, and the scaling now happens *before*
the comparison, so the threshold and the quantity it judges are the same kind
of thing. (Retaining the old constant, as the written plan proposed, would have
left dead code: `bench-history.py` computes its own bound from
`CANARY_TOLERANCE_PCT` and never imported the kernel's.)

A refused measurement now carries *why*, as a new `ArmFailure` enum, because
the causes need opposite responses from the reader and carry opposite
evidential weight for the verdict:

| variant | what happened | can it have hidden an excursion? |
|---|---|---|
| `NotSeparated` | arms inverted, or the store was elided | **yes** -- keeps the verdict UNKNOWN |
| `BelowFloor(n)` | arms separated by a resolvable but sub-floor amount | no -- an excursion is large and would have cleared the floor |
| `ScaleRejected` | the scale-invariance check refused it before it was read | yes |

`ScaleRejected` did not exist in the plan and had to be added: the calibration
block flattened a scale rejection to "no value" and the canary then re-inflated
it into a *separation failure*, which is simply a false statement about what
happened. A delta of exactly zero is `NotSeparated`, not `BelowFloor(0)` -- it
resolves nothing, so noise of any size could sit between the arms, and the safe
direction of error is UNKNOWN (a re-run) over CLEAN (a missed regression).

Boundary cases are `const` assertions on the real `const fn`, following
`layout_pad.rs`: a kernel `#[cfg(test)]` module never compiles here, so a test
written that way could never fail. Mutation-checked -- `>=` to `>` on the floor
fails the build.

**`26c139a81` -- `scripts/bench-history.py`.** The same defect turned out to be
present here in *three* places, not the one this entry predicted, and each is
the same mistake: reading a whole-cycle field as though it were the whole story.

1. `min` is rounded to whole cycles, so 0.87 cycles/access reads back as
   `min == 0` -- bit-for-bit what a dead instrument writes. (Predicted above.)
2. **`start` is rounded the same way**, and the pre-`invalid` heuristic "a zero
   start means one endpoint measurement failed" condemned the identical good
   records. Not predicted; found only because the new WHPX-scale test still
   failed after fixing (1). It now applies only where there is no `invalid`
   field to read, which is the only situation it was ever an inference about.
3. Every failure counted against the verdict equally. `below_floor` -- the new
   11th wire field -- is now subtracted, so the `BelowFloor` rejections that
   remain after the floor fix cannot keep a fast guest ungradeable.

`below_floor`'s absence stays distinguishable from zero on disk: a pre-fix
record's failures are genuinely of unknown kind. The verdict *assumes* zero
when judging such a record -- the conservative direction, which preserves every
existing classification -- but never writes it.

**Not retroactive, deliberately.** The six 2026-08-19 WHPX sweep arms recorded
`samples: 0`, and `samples == 0` outranks the whole split: there is no finding
to grade. They stay `broken` and the six-arm band keeps its `6/6 arms could not
measure host load` caveat (see design-decisions.md §239). The fix stops such a
run happening; it does not manufacture data for one that already did.

**Verification.** 605 assertions in `scripts/test-bench-history.py`, including
two new groups covering the boundary from both sides in both units. Six
mutations checked and all caught: dropping the centi bound (12 failures),
dropping its doubling (1), not subtracting `below_floor` (1), subtracting all
of `invalid` (10), restoring the unconditional zero-start fallback (2), storing
`below_floor` with a default (6).

**One correction to the plan, recorded because the plan was wrong in the
direction this whole entry is about.** `build/canary-fix-plan.md` §3.4 put the
centicycle boundary at `CANARY_MIN_RESOLVABLE_CENTI` itself. That is one
quantum short: a spread is `(hi - lo) / lo` across two independently-quantised
samples, so it carries a quantum from *each* end -- `200/m` percent, not
`100/m`. The whole-cycle branch has reasoned that way since P18 measured it;
the plan simply failed to carry the doubling down with the unit, which is the
same species of oversight as the original bug.

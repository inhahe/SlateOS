### [A] PREDICTION P23 — sensitivity, measured with a window wide enough to carry it — registered 2026-08-19

**Registered before the run exists.** Numbers below are derived from the model's
arithmetic, not guessed; the derivation is written out so a later reader can
check that the threshold was not chosen after seeing the answer.

**In short:** the suite has a feature that guesses which of its measurements were
disturbed by other activity on the machine. The last experiment showed it points
at the *right place*, but it only caught 7 of the 12 disturbed measurements — and
that looked like a weak result. We think it is not weak at all: the feature only
takes a reading every 8th measurement, and the disturbed stretch was only 12 long,
so it got exactly **one** reading inside it. This run makes the disturbed stretch
32 long so it gets **four**, which is the first fair test of how much it catches.

#### The run to make

```
scripts/canary-load-test.sh --at io_ring_nop --until crypto_poly1305_1KiB
```

Verified against the previous run's SCORE lines before registering: this lands
the load on **positions 32–63, 32 of 86 benchmarks** — an interior window with
32 benchmarks before it and 22 after. (`io_ring_nop` is a scorecard name; the
controller triggers on its live line `io_ring_nop_submit`, and says so.)

| | run 3 (graded) | this run |
|---|---|---|
| loaded window | positions 60–71 (12) | positions 32–63 (32) |
| canary samples **inside** the window | **1** (position 64) | **4** (32, 40, 48, 56) |
| provably-clean region | 58 | 38 (positions 0–23, 72–85) |

#### The claims

- **P23(a) — sensitivity ≥ 24 of 32 (75%).** *Falsified below 75%.* With four
  samples inside the window instead of one, the model's flagged set should cover
  most of it rather than a triangle around a single point.
- **P23(b) — false positives remain 0 of 38.** *Falsified by any flag in the
  provably-clean region.* This is the half of run 3's result that was **not**
  resolution-limited, and widening the window is the obvious way to break it: a
  model that quietly flags in proportion to window size would fail here and pass
  (a).
- **P23(c) — localisation stays 100%**: every flagged benchmark inside the window
  widened by the sampling interval (24–71). *Falsified by any flag outside it.*
- **P23(d) — the whole-run verdict will again read `Canary OK`.** *Falsified if
  it reads CONTAMINATED.* Added after discovering that run 3's did (see the
  entry above): spread is a function of the excursion's **depth**, not its
  extent, so tripling the window should not move it. If (a) and (d) both hold,
  the positional model is shown to be the more sensitive of the two instruments
  on windows differing by a factor of nearly three — a stronger claim for it
  than P22(a) alone makes, and an argument against letting `Canary OK` be the
  last word on whether a run is trustworthy.

#### Caveat, registered rather than discovered afterwards

The margin under the blindness cliff is **two samples**, not a comfortable
distance. `trace_reference` is the median of the 11 positioned samples, so at 6
elevated the baseline becomes the disturbance and sensitivity collapses to zero
(table in the entry above). This window elevates 4 by design — 32, 40, 48, 56 —
but samples 24 and 64 sit immediately outside the window's edges, and if the
load's ramp-up or ramp-down elevates *both* of them even partially, the count
reaches 6 and the median moves.

So a **0% sensitivity result must be read as the cliff, not as a refutation of
the model** — and it is checkable rather than a matter of interpretation: the
run's own CANARY-TRACE says how many samples were elevated and which baseline
the model chose. If that happens the correct response is to re-run narrower
(24 wide, 3 samples, predicted 83%), not to reinterpret P23.

#### Where 75% comes from

`interpolate_trace` is a straight line between adjacent samples, and
`report_positional_attribution` flags a factor above **1.10**. Run 3's one
elevated sample read ×1.196 against its baseline, which reproduces its 7 exactly:
the ramp crosses 1.10 at 4.09 positions either side of the peak, so positions
61–67 flag and 60 and 68 do not — 7, in the window, all of them.

Applying the same arithmetic to four contiguous elevated samples: the factor is
**flat** at ×1.196 across 32–56 and ramps at each end, crossing 1.10 at position
≈28.1 on the way up and ≈59.9 on the way down. Flagged: **29–59**. Intersected
with the real window 32–63 that is **28 of 32 = 87.5%**, with 29–31 flagged
outside the window but inside reach, and nothing in the clean region.

So 87.5% is the arithmetic's own answer and **75% is the threshold**, leaving
room for the elevation to be less uniform across a 3×-longer window than a
single sample was — which is the physical thing this run actually measures, and
the reason it is worth running rather than deriving.

#### What each outcome licenses

- **All three hold** → 7-of-12 was the sampling interval, as claimed, and
  sensitivity is a property of the interval rather than of the model. The
  remaining lever on it is `CANARY_SAMPLE_EVERY`, not the arithmetic.
- **(a) fails while (b) and (c) hold** → the model attributes far less than its
  own interpolation implies, and the 7-of-12 explanation in the run 3 write-up is
  wrong and must be retracted there.
- **(b) fails** → the more serious outcome. It would mean run 3's 0-of-58 was an
  artefact of a narrow window, and the whole attribution claim weakens to
  "flags a lot, some of it in the right place".

**Not a licence to correct anything.** P23 is about *how much* of a disturbance
the model finds, not about whether the factor is the right size to divide by —
that is P22(b)/(c), still unmeasured. §229 stands either way.

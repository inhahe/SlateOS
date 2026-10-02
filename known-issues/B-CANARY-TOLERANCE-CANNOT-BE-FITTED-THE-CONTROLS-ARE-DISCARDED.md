### B-CANARY-TOLERANCE-CANNOT-BE-FITTED-THE-CONTROLS-ARE-DISCARDED

**Status:** open (blocks the `CANARY_TOLERANCE_PCT` retuning). **Found:**
2026-08-14, on trying to retune the tolerance from real data.

`CANARY_TOLERANCE_PCT = 25` is a placeholder chosen before there was any data on
what an uncontaminated run's spread looks like, and the standing task is to
retune it from measurements. Attempting that surfaced why it cannot be done yet.

`scripts/canary-spread-survey.py` classifies every release record in
`bench/history.jsonl` by whether the host's load state at measurement time is
*established*. The result:

```
known-loaded:  0 release runs
unknown:      14 release runs   (spreads 0,0,0,0,0,0,0,40,47,100,100,168 ...)
              -> 5 of 14 exceed the 25% tolerance
```

**Zero known-loaded records — and that is not an accident of sampling.** The
three demonstrated positive controls (spreads 53%, 117%, 128%, produced by
`scripts/canary-load-test.sh` with six CPU spinners) are absent because that
harness restores `history.jsonl` from its `EXIT` trap. Which is correct on its
own terms: leaving a knowingly-contaminated run in place would make it the
baseline the *next* real run is diffed against. The consequence, though, is that
the only runs whose provenance is established are precisely the ones excluded
from the machine-readable record, and the 14 that remain are all "unknown".

So the data needed to fit a threshold does not exist:

* The 5 unknown-provenance runs above 25% cannot be used to argue the tolerance
  is too tight. A high spread in a run of unknown provenance is equally
  consistent with the run having been genuinely contaminated — i.e. with the
  detector working. Fitting to them would widen the tolerance until the detector
  stopped firing and then read the silence as a clean bill of health, which is
  the same failure the canary exists to prevent.
* The runs at 0% cannot be used to argue it is too loose either, for the mirror
  reason: nothing records that they were idle.

Retuning honestly requires the record to carry the load state, and it must be
*recorded at measurement time*, not inferred afterwards from the number itself —
inferring it from the number is circular, since the number is what the threshold
is being fitted to.

**Proper fix:** `bench-history.py` gains an explicit `--host-load
<idle|loaded|unknown>` recorded as a field on the record, defaulting to
`unknown` and never to `idle` — an unmarked run must not be silently promoted to
evidence. `canary-load-test.sh` passes `loaded` and writes to a sibling
`bench/history-loaded.jsonl` instead of restoring and discarding, so the positive
controls are preserved as data without entering the baseline chain that the
run-over-run comparator walks. Once both classes have several members, the
threshold can be fitted to the gap between them.

Until then `CANARY_TOLERANCE_PCT` stays at 25 and stays labelled a placeholder.
Changing it now would be a guess dressed as a measurement.

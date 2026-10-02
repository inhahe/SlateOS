## §229 — The positional drift model *attributes* contamination and refuses to *correct* it

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The benchmark suite has a "canary" — a tiny, fixed memory
operation re-timed every 8 benchmarks, whose cost should never change. When it
does change, something outside the OS (usually this agent's own tooling) was
competing for the machine's CPU, and the benchmark numbers from that run cannot
be trusted. Until now the tool could only say *how much* the canary moved, which
condemned the whole run — all sixty-odd benchmarks — on the strength of a
disturbance that may have touched two of them. The canary also records *where in
the run* each reading was taken, and that record has now been made usable. The
decision is what to do with it: this tool will **name the benchmarks that ran
during the disturbed stretch**, and will **not** quietly adjust their numbers to
compensate.

### What was built

Three pieces, in `scripts/bench-history.py`:

- Each benchmark now carries its **suite position** — its ordinal among the
  scorecard lines — stored explicitly on the history record. This is the join
  key between a benchmark and a canary reading, and it cannot be recovered
  later: records are written with `sort_keys=True`, which alphabetises the
  benchmark map on the way to disk, so all 71 existing records have lost their
  suite order permanently.
- A **baseline**: the median of the run's own mid-suite canary readings.
- A **factor per benchmark**: the canary reading interpolated to that
  benchmark's position, divided by the baseline. 1.0 means the machine was
  behaving normally where that benchmark ran; 3.2 means the canary was reading
  3.2x dear there.

### The decision, and the case against it

The factor is *printed*, next to the benchmark's name. It is **not** applied to
the measured value, and it is **not** stored on the record.

Against — and this is a real cost, not a strawman: a contaminated run currently
fails the regression gate wholesale, and a correction would rescue most of it.
The information to do so now exists. Declining to use it means continuing to
throw away runs that are mostly good.

For, and why it wins:

1. **The model's resolution is eight benchmarks, and a corrected number would
   not say so.** The canary samples once per 8 benchmarks, so a one-benchmark
   spike and an eight-benchmark plateau are indistinguishable to it; the model
   renders both as a triangle spanning the interval. Against the real
   contaminated trace in `build/ab-old-2.log` — whose only dear reading is
   position 32, at 3.2x — the model flags positions 25–41, sixteen benchmarks,
   of which perhaps one was genuinely disturbed. Printed as an attribution that
   is honest. Baked into fifteen benchmarks' recorded values it is fifteen
   fabrications that look like measurements and would enter the history
   permanently.
2. **The coupling between canary and benchmark is benchmark-dependent.** The
   canary times a memory access. A memory-bound benchmark tracks it closely; a
   branch-bound one barely tracks it at all. One factor applied to both
   under-corrects the first and over-corrects the second, and nothing in the
   record would afterwards distinguish a corrected value from a measured one.
3. **This project's most expensive bench failures have all been instruments
   asserting more than they measured** — nine runs certified clean by a canary
   that measured zero (`known-issues.md`
   B-BENCH-CANARY-MEASURES-ZERO-IN-RELEASE-AND-BLAMES-THE-HOST), a contamination
   verdict that named the optimiser as the sole cause of a symptom host load
   also produces. A silent correction is the same shape of error: it converts a
   modelled quantity into a measured-looking one at the point where a reader
   stops being able to check it.

### The smaller calls inside it

- **Median, not mean, for the baseline.** This is what makes the positional
  model complementary to the existing whole-suite `global_drift` rather than a
  rival estimate of the same thing. If the host was uniformly twice as busy,
  every reading is 2x, the median is 2x, every factor is 1.0 — so this model
  contributes nothing and leaves the uniform case to `global_drift`, which is
  built for it. A mean would let a burst drag its own baseline toward itself and
  shrink the correction exactly in proportion to how badly it was needed.
- **Three readings minimum.** Two define a line through both of themselves, with
  no residual and no way to be wrong; three is the smallest number at which the
  trace can disagree with a line, and therefore the smallest at which an
  excursion is distinguishable from a trend.
- **Hold flat outside the sampled span, never extrapolate.** Extrapolation
  produces its largest values exactly where the evidence is thinnest — the tail
  of the suite. The one exception is the post-suite canary reading, which is a
  genuine right-hand bracket for the trailing benchmarks; using it required
  giving the two endpoint readings distinct labels in the kernel, since both
  previously printed as `end:` and a failed calibration leaves the *end* sitting
  in the slot a reader would take for the start.
- **The factors are not stored on the record.** Both inputs are (the trace and
  the position map), so the factors are derivable, and storing a derived value
  alongside its inputs creates two things that can disagree the moment the model
  changes. The file's existing convention — store the measurement, not the
  verdict — points the same way.
- **Silence, not an empty list, when the model cannot be built.** The
  attribution line sits directly under the contamination verdict, where a reader
  has just been told the run is untrustworthy and is looking for something to
  narrow it down. Printing "affected benchmarks: none" when there was no trace
  to consult answers that question with an exoneration nobody measured.

### What would reverse this

Evidence that the factor predicts the disturbance well enough to correct with.
That is measurable and not yet measured: `scripts/canary-load-test.sh` can apply
load at a known moment, so a run with load confined to a known stretch of the
suite would show whether the flagged benchmarks' inflation actually matches
their factors. If it does — and if the match holds across memory-bound and
branch-bound benchmarks alike — points 1 and 2 above weaken and a correction
becomes defensible. Until that experiment is run, correcting would be asserting
its result rather than establishing it.

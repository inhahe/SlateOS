## §240 — A check that decides whether evidence counts must be on the only path to that evidence; an analysis that re-selects its own inputs is not covered by it

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The benchmark tooling has a function whose job is to decide which
recorded runs are allowed to be compared against each other. It works. But on
2026-08-19 a set of numbers reached `open-questions.md` — where the operator is
being asked to make a decision — that had been produced by a hand-written script
which picked its own runs and never called that function. It picked a run that
the function would have rejected, and the published figures were wrong by up to
400x, in the direction that supported the conclusion the analysis was hoping for.
The decision is that a guard only guards the paths that call it, so evidence
quoted to the operator must come from a command that the reader can re-run —
and any ad-hoc analysis that groups records must call the real predicate rather
than restate it.

### The situation

`layout_arm_rejection()` in `scripts/bench-history.py` decides whether a given
benchmark record may be used as an arm of a layout band. A layout band is a
range of timings measured across several builds of *identical* source at
different code offsets; its purpose is to say how much a benchmark moves for
reasons that are not a code change, so movements inside it can be dismissed. A
band that is too wide therefore dismisses real regressions, silently.

One of the predicate's checks is `LAYOUT_SWEEP_TAG`: the record's `experiment`
field must say it was submitted as a sweep arm. That check looks redundant
beside six fields of real measurement, and its comment already explains why it
is not — the 16:15 run of 2026-08-19 matches a genuine arm on *every* other
field (same host, same profile, unloaded, `text_pad: 0` truthfully, `accel`
absent truthfully, byte-identical kernel tree) and is separated only by the tag.
See `known-issues.md`
`B-A-AN-ORDINARY-RUN-NEARLY-JOINED-A-LAYOUT-BAND-AS-A-SEVENTH-ARM`.

That run is a WHPX (hardware-virtualised) run. Admitting it to a TCG
(software-emulated) band is not a small error: `hpet_read` costs a VM exit under
WHPX (13,680 ns) and is emulated inline under TCG (446 ns), so one such row
inflates that benchmark's band about thirtyfold.

It was admitted. Not by `bench-history.py`, which rejected it and always would
have, but by a hand-written analysis that enumerated records itself to build the
WHPX-vs-TCG comparison table for Q53. The table was published in
`open-questions.md` as the evidence for that question's option D.

| | as published | actual |
|---|---|---|
| median TCG band | 36.5% | 26.0% |
| mean | 104.9% | 40.6% |
| worst benchmark | 2466% (`hpet_read`) | 182.0% |
| `hpet_read` | 2466% | 6.2% |

### The decision

Two rules, adopted together:

1. **Numbers quoted in `open-questions.md` must be reproducible by a command
   the reader can run.** The corrected Q53 table is entirely printed by
   `python scripts/bench-history.py --layout-bands --profile release`, which
   routes through `layout_arm_rejection` and therefore cannot pick up an
   untagged run.
2. **An ad-hoc analysis that groups records must call the real predicate, not
   restate its conditions.** `build/whpx-band-preview.py` does exactly this —
   it monkeypatches the *grouping key* and reuses `layout_arm_rejection`
   unmodified — and its WHPX column survived re-derivation intact. That is the
   pattern; the TCG column was produced by something that did not, and did not.

### Why, and what the alternatives were

**Alternative A: strengthen the record so the tag is unnecessary.** Make the
data itself distinguish a sweep arm from an ordinary run, so that no
declaration is needed and no analysis can get it wrong. Rejected because it is
impossible, and the impossibility is instructive: `text_pad: 0` is *correct*
for an unpadded kernel and there is no value meaning "not a sweep arm";
`accel: None` is correct-for-its-time on a kernel predating the banner; the
source digest is correct *and equal*, because the source genuinely was equal.
The intent — "this run was submitted as an arm" — is not derivable from any
measurement. It has to be declared, which means there will always be a check
that an analysis can skip.

**Alternative B: treat it as a one-off mistake and fix the numbers.** Rejected
because the shape of the failure is the argument against it. It was silent
(nothing flagged it), plausible (the resulting group looked entirely normal),
and *directionally favourable* to the hypothesis being tested — inflating TCG's
noise makes the case for abandoning TCG look stronger. It was found only
because the table was being re-derived for an unrelated reason (adding a sixth
arm), and could as easily have gone unexamined indefinitely. A failure with
those three properties is not one that "being more careful" addresses.

**Alternative C: forbid ad-hoc analysis; require every number to come from
committed tooling.** Rejected as too strong, and it would have prevented
something valuable: the preview script existed precisely to answer "what will
the band be once the digest fix lands?" *before* implementing the fix, which
de-risked the implementation. The distinction that matters is not ad-hoc versus
committed, it is whether the analysis re-implements a decision the tooling
already owns. Rule 2 targets exactly that and leaves exploratory work alone.

**The cost of rule 1** is that some evidence is harder to quote — a figure that
no command prints must either get a command or be described rather than
tabulated. That is accepted: `open-questions.md`'s entire purpose is to support
an operator decision, and a number the operator cannot re-check is worth less
than one they can, however carefully it was derived.

### Relationship to the existing rules

This is the same defect as `layout_arm_rejection()`'s own docstring warns
about, one level out. That docstring explains that `layout-sweep.py` must call
the predicate rather than restate its conditions, because a restatement drifts
and then passes arms the real predicate drops. Here the restatement was written
in an analysis script instead of in the sweep, and drifted in the other
direction — admitting an arm the real predicate rejects. Same cause, opposite
sign, worse consequence: the sweep failure is loud (no band appears), this one
is silent (a plausible band appears).

It is also another instance of the pattern §238 and §239 circle: an inference
resting on a justification that quietly stopped holding, with no test able to
notice because the justification was never expressed as code. Here the
unexpressed justification was "the arms in this table were selected the way the
tool selects them."

## §241 — A record with no recorded accelerator is its own population, not an assumed TCG one; the resulting blindness is announced rather than papered over

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The benchmark history judges a boot by comparing it against
earlier boots on the same machine. It has just been taught that boots produced
by QEMU's two different virtual-machine accelerators are not comparable — they
differ by multiples, in both directions. But sixty of the sixty-one stored
boots predate the field that says which accelerator was used. The choice was
whether to assume those sixty were the common one (TCG), which would keep the
statistics working immediately, or to treat "not recorded" as its own third
value, which is honest but leaves the tool unable to judge anything for the
next handful of runs. The second was chosen, with the tool now printing exactly
why it cannot judge.

### The decision

`ACCEL_UNRECORDED = None` is a value in its own right and groups only with
itself. `comparable_records()` compares `record_accel(record) == accel`
directly, so an unlabelled record never joins a labelled window and vice versa.

### Why not fold it into TCG

Because it is falsifiable and it is false. The first Hyper-V/WHPX boot on this
host is `2026-08-19T16:15:09`, and it predates the `accel` field. An absent
value is therefore demonstrably not evidence of TCG — at least one unlabelled
record is known to be WHPX. Folding would import that record into the TCG band.

The stronger version of the argument does not depend on that one record. The
assumption "unlabelled means TCG" is unfalsifiable *by the tool*: nothing in a
record without the field can ever contradict it, so if it is wrong the band is
wrong forever and silently. The alternative's cost is visible and expires.

Note that the assumption is not even unreasonable — a structural discriminator
built from labelled data (any benchmark where `min(WHPX) > 4·max(TCG)`, which
selects `hpet_read` and the two ARP lookups on VM-exit cost rather than on a
tuned threshold) flags exactly one of the sixty unlabelled records, and it is
the known 16:15 probe. So "the unlabelled window is TCG-like" is *true*. It was
still rejected as a *rule*, because a rule that happens to be true of today's
data and cannot be checked against tomorrow's is the shape of every
band-poisoning bug this project has had.

### What it costs

The first labelled TCG run finds no history. Both banded axes (wall clock,
dispersion) return `unknown` until `MIN_WINDOW_FOR_BAND` (6) labelled runs
accumulate — roughly five runs of reduced sensitivity. During that window the
tool cannot condemn a genuinely contaminated run on those two axes; the canary
axis is unaffected.

*What changes:* for about five boots, the verdict line reads `unknown` where it
would otherwise read `clean` or `contaminated`.

### Why that cost is acceptable

Because it is *stated*. `accel_thinning_note()` puts a sentence in the verdict's
own notes giving the comparable-run count, how many records were excluded for
predating the field, how many for naming a different accelerator, and the
reason ("an accelerator changes the numbers by a multiple, not a percentage, so
those runs would widen the bands rather than fill them"). A reader who sees
`unknown` is told why, and the note disappears by itself once the window fills.

The rejected option has the opposite profile: it works immediately and fails
invisibly. This file has had to undo that trade three times already
(`CANARY_TOLERANCE_PCT`, `DISPERSION_SUSPECT_RATIO`, the absolute A/B cycle
budget), and the general principle it keeps arriving at is that **loud
abstention beats silent confidence.** A tool that says "I cannot tell" is
usable; a tool that says "clean" because it compared against the wrong
population is worse than no tool.

### A corollary that was implemented at the same time

Neither window selector may have a default. `profile` used to default to
`"debug"`, on the reasoning that a caller passing no `--profile` should keep
finding the legacy records. There is no such caller, and a selector that
silently picks a population when the caller forgets to name one is the same
failure in miniature: on this host the unnamed-accelerator bucket holds sixty
runs, so the wrong answer looks like a full and healthy history rather than an
error. Both `profile` and `accel` are now required positionals, and a test
asserts that omitting either raises `TypeError`.

### Revisit when

The unlabelled bucket stops being useful — i.e. once ≥6 labelled TCG release
records exist and the unlabelled sixty are only of historical interest. At that
point the question becomes whether to retire them from the window entirely
rather than keep a third population alive. That is a cheaper decision than this
one and does not need to be made now.

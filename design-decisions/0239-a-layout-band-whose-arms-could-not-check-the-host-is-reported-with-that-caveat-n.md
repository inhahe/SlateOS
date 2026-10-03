## §239 — A layout band whose arms could not check the host is reported with that caveat, not discarded

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Every benchmark run includes a tiny self-check — a "canary" — whose
only job is to answer "was this machine busy doing something else while the
measurement ran?". On all six runs of the 2026-08-19 layout sweep that self-check
failed outright: it could not produce a usable answer either way. The results of
those six runs are used to set a tolerance band, and anything that moves less
than that band is dismissed as noise rather than reported as a slowdown. The
question was whether to throw the six runs away because their self-check failed,
or keep them and print a warning. The decision is: keep them, and make every
place that prints the band also print who could and could not verify the host.

### The situation

`layout_bands()` takes several boots of byte-identical source, built at different
`.text` offsets, and reports how far each benchmark moves when *only* the code's
address changes. That figure is then used to withdraw movements: a benchmark that
moved less than its band is filed under "explained by code placement" instead of
"regressed".

The six WHPX arms recorded on 2026-08-19 (`943b3f21b..78a921b4f`) each carry
`canary: {samples: 0, invalid: 13, ...}` — the kernel-side canary rejected all
thirteen of its own measurements, so `canary_verdict()` returns `broken`. The
band they produce printed with no indication of this whatsoever; it read exactly
like a band measured on a machine verified to be idle.

That is a genuine defect. The band is a *licence to dismiss findings*, and a
licence issued by an instrument that could not check its own preconditions is not
the same object as one that could. Nothing in the output distinguished them.

### The decision

Report, do not discard.

`layout_bands()` now returns a fifth element per band — `{verdict: arm count}` —
and `describe_layout_band()` / `--layout-bands` state it in words. Silence is
reserved for the all-clean case, so the caveat's *presence* is what carries the
information. `absent` (an arm with no canary at all) is caveated alongside
`broken`, because an unchecked run is not a passing run and the two are only
distinguishable if the output refuses to read silence as a pass.

### Why not void the group, which is the usual direction here

`layout_bands()` already voids a group outright when any arm's host-drift factor
cannot be computed, and the reasoning there is the standing direction-of-error
rule: an uncorrected band is *too wide*, a too-wide band dismisses real
regressions, so failing to "unmeasured" is the safe failure. Applying the same
rule to a broken canary is the obvious move, and it was rejected for one reason.

An uncorrectable drift factor is evidence that *this band is wrong* — the
correction it needed is missing, and its absence has a known sign. A broken
canary is evidence that *nobody knows*: the host may have been perfectly idle.
The two are not the same epistemic object, and voiding treats them as if they
were.

Concretely, voiding would have discarded the only WHPX measurement that exists —
and would go on discarding every future WHPX sweep, silently, until the canary's
resolution floor is fixed (`CANARY_MIN_RESOLVABLE` is applied to raw cycles but
was derived before `CENTI` made the measurement centicycles, so the bound is 100x
too strict and WHPX's ~87 centi is rejected out of hand). That is a *separate*,
already-diagnosed instrument bug. Letting it silently delete an unrelated
measurement — with the deletion presenting as "no sweep has been run" — is the
same class of invisible failure that the whole layout-band mechanism exists to
end.

### The cost, stated plainly

A reader who ignores the warning gets today's behaviour, which is the unsafe one:
a band possibly inflated by host noise, dismissing movements it should not. This
decision does not fix that; it makes it visible. The actual fix is the canary
resolution floor, and until that lands the warning is the only thing standing
between a broken instrument and a silently-widened tolerance.

This is therefore a decision to revisit once the floor is fixed. If broken
canaries persist *after* it, the argument above weakens considerably — a canary
that stays broken with no diagnosed cause is not "nobody knows", it is an
instrument that does not work, and voiding becomes correct.

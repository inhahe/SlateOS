## TD-B-THE-THIRTY-TWO-SUITES-THAT-VERIFY-THE-CHECKERS-ARE-THEMSELVES-RUN-BY-NOTHING — **RETRACTED, THE CLAIM WAS FALSE**

**Filed:** 2026-09-04 by lane B. **Retracted the same day, by the same lane,
on observing the thing it said did not exist.** Kept rather than deleted: the
entry was wrong for a reason worth having written down, and a tracking file
that quietly loses its own false entries teaches nobody.

**In short:** I wrote that the 32 `scripts/test-*.py` suites — the ones that
verify the `check-*.py` gates actually work — were run by nothing, and would
execute only when a person typed the filename. **That was false.**
`scripts/boot-test.sh` has run every one of them on every boot test since
2026-08-29 (`bc98c61cb`), in `check_python_suites()` at `boot-test.sh:5159`.
Observed directly on 2026-09-04: a boot test on `lane-b` printed
`=== Running the tooling's own test suites (scripts/test-*.py) ===` and a
per-suite pass line for each, in alphabetical order, before building anything.

### Why the claim was wrong, which is the part worth keeping

The retracted measurement read: *"Grepped for every suite name across `*.sh`,
`*.yml`, `*.yaml`, `*.toml` and `Makefile`: no runner exists."* The grep was
accurate. The inference from it was not.

`check_python_suites` does not name a single suite. It globs
`"$PROJECT_ROOT"/scripts/test-*.py` and runs whatever it finds — and its own
comment says why, in terms that are precisely the refutation of my method:

> WHY IT DISCOVERS RATHER THAN LISTS. A hand-written list is a second place a
> new suite must be registered, and forgetting is silent: the suite passes by
> not running, which is indistinguishable from passing. So the glob is the
> list […]

So the design decision that makes the wiring *robust* — never writing the names
down — is exactly what made a search for the names come back empty. **A grep for
a list cannot find a discovery**, and "I searched for every name and found no
runner" is not evidence that nothing runs them; it is evidence of nothing at
all when the runner is a glob. The general lesson, and the reason this is
retained: when a search returns "nothing anywhere runs this", the next step is
to *run the thing that would run it* and watch, not to file the finding. One
boot test would have refuted this entry in the eleven minutes it took to reach
that gate.

There is a second-order irony worth noting, since it is the same mistake twice
in one entry: the retracted text proposed, as its step 2, to "wire the fast
ones into `boot-test.sh`'s pre-build gate block" — that is, to replace a
working glob with a hand-written list, which is the arrangement the glob's
author had already rejected in writing at the top of the function I had not
read.

### What survives the retraction

Two smaller claims were true and are re-filed here at their real size, rather
than left inside a retracted entry where nobody would trust them:

1. **`check-gates-are-wired.py` really does glob `scripts/check-*.py` only**, so
   the suites are not in *its* audited population. But they are not unaudited:
   `check_python_suites` guards its own discovery with a floor — it refuses to
   build if fewer than 10 suites are found, on the stated grounds that "a gate
   that discovers nothing reports no failures, which reads exactly like a pass."
   That floor is weak (32 suites could fall to 10 unnoticed) but it is not
   absent, and a sharper ratchet here is a *nice-to-have*, not the hole the
   retracted entry described. Not currently tracked as debt; noted here.
2. **Case 11 of `quote-names.py --selftest` really was written in isolation**
   rather than as a case in `test-checkers-honour-head.py`, which is the suite
   built for exactly that question and which would have caught the `GIT_DIR`
   bug. That remains a genuine miss — but the reason I gave for it ("a suite
   nothing runs is a suite nobody remembers to extend") was a rationalisation
   built on the false premise. The real reason is that I did not look for an
   existing home before writing a new one.

### The one real finding, which belongs elsewhere

This gate is a large part of why the pre-build phase is long. Measured on
2026-09-04 on `lane-b`: **~41 minutes of gates before `check_python_suites`
even started**, and the suite gate was still running 6 minutes later, having
reached `test-check-self-tests-wired.py` — with `test-checkers-honour-head.py`
(~10 minutes standalone, measured 582 s) next in alphabetical order and 19
suites after it. The run was killed at 47 minutes, before the build began.

That cost is real and is worth a deliberate decision, but it belongs to
`TD-B-PRE-BOOT-QUICK-IS-NOT-QUICK` — where the question is "what should a quick
run skip", not "is this wired". Recorded there, not here.

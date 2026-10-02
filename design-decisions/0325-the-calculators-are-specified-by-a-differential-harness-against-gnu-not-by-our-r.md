## §325 — The calculators are specified by a differential harness against GNU, not by our reading of POSIX

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** we had written `bc` and `dc` from POSIX plus memory of how the
traditional tools behave, and tested them with unit tests we wrote at the same
time. Those tests asserted the same beliefs the code did, so they could only
ever confirm them. Once a real GNU `bc`/`dc` was found on the machine and a
harness compared the two byte-for-byte, seven separate behaviours turned out to
be wrong — all of them green under our own suite. The decision is that GNU's
observed output, captured in `scripts/calc-diff.sh`, is now the specification
for anything POSIX does not pin down, and that our unit tests are derived from
it rather than from reasoning.

**The alternatives**

| | Spec from POSIX + our reading | Spec from a running GNU binary |
|---|---|---|
| Cost | none; it is what we were already doing | a harness (~250 lines) and a dependency on WSL to *run* it |
| Catches a misreading of the standard | no — a misreading propagates identically into code and test | yes, and it did, seven times |
| Covers what POSIX leaves undefined | no; those are pure guesses | yes; that is most of what it found |
| Risk | agrees with itself forever while disagreeing with every script | encodes GNU's bugs as well as GNU's behaviour |

**Why the second, despite the last row.** The things at stake are not
mathematics, they are *punctuation*: whether a base-36 digit is written `Z` or
` 35`, whether `x++` on its own line echoes, whether `1 0 /` leaves its operands
on the stack. A script does not care which spelling is more principled; it
cares that the spelling matches the one every other `bc` uses. Where GNU is
arguably wrong — an unrecognised escape like `\v` silently swallowing both its
characters is not a *good* rule — matching it is still correct, because a
program that relied on the other behaviour would already be broken everywhere
else. The one thing this must not do is import GNU's *arithmetic* bugs, and it
cannot: the numeric results are independently checked against hand-computed
values (§324), and the harness compares those too, so a disagreement about a
number is a signal to investigate rather than to copy.

**The narrower call inside it: where the line-length conversion lives.**
`BC_LINE_LENGTH=10` makes GNU `bc` emit nine columns then `\`; `DC_LINE_LENGTH=10`
makes GNU `dc` emit ten. Same source tarball, two arithmetics — `L - 2` against
`L - 1`, and each stops wrapping at a different floor. `bignum::wrap_number`
therefore takes a *chunk width*, not a line length, and each front-end owns the
`wrap_chunk()` that derives one from its own environment variable. The
alternative — a flag on `wrap_number` — would have put a `bool is_dc` in the
numeric library, which is exactly the sort of parameter that later grows a third
value and ends up encoding all of `dc` inside `bignum`. The field on each
interpreter was renamed from `line_length` to `wrap_chunk` for the same reason:
the old name is what made the off-by-one invisible for as long as it was.

**What this obliges.** `scripts/calc-diff.sh` must be run after any change to
how either program prints, and a deliberate divergence must be recorded as an
`xfail_bc`/`xfail_dc` case with a comment saying why — a silent difference is
indistinguishable from a regression. If GNU is ever unavailable the harness
skips rather than fails, so it cannot become a build dependency; the unit tests
it seeded stay behind and keep the measured values under test.

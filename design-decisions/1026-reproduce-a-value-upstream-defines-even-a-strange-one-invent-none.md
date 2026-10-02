## 1026. Reproduce a value upstream defines, even a strange one; invent none

**Date:** 2026-09-16
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `l(0)` — the logarithm of zero — has no answer as a real number.
GNU `bc` returns a very large negative number and says nothing; ours printed
`Runtime error: log of non-positive number` and computed nothing. We now return
GNU's number. This looks like the opposite of §1025, which refused to print a
number we could not produce honestly, and the difference between the two cases
is the point of this entry.

**What GNU actually does**, measured at six scales because one would not have
distinguished a formula from a constant: `-(10^scale - 1)`, rendered at the
current scale. `scale=1` gives `-9.0`, `scale=20` gives twenty nines,
`scale=0` gives `0` — which is the formula agreeing with itself, since
`10^0 - 1` is zero. Every non-positive argument saturates the same way;
`l(-1)`, `l(-100)` and `l(-0.5)` all answer what `l(0)` answers, so GNU has no
error path here at all.

**The tension with §1025, stated plainly.** There I refused to emit `adr=`
because we compile to no dc program and there was no honest value for the
field. Here I emit a number for an operation that has no answer. Both cannot be
"never fabricate" — so what is the actual rule?

The rule is about **who defines the value**:

* `adr=` had no definition outside GNU's own implementation. Any number we put
  there would have been ours, meaning nothing, and unfalsifiable by the reader.
* `-(10^scale - 1)` has a precise, measured, reproducible definition, and
  **scripts can already depend on it.** A `bc` script that tests
  `if (l(x) < -1000000)` as an underflow guard works on GNU and would break on
  a `bc` that errored instead. Refusing to produce it is not honesty; it is a
  different behaviour wearing honesty's clothes.

Put another way: §1025 declined to *invent* a fact. This declines to *withhold*
one. The thing both protect is that what we print means what the reader thinks
it means.

**The alternatives:**

* **(a) Keep erroring.** *What changes:* `l(0)` prints a diagnostic and the
  statement produces nothing. Mathematically defensible, and arguably kinder —
  a huge negative number flowing silently into a later calculation is a real
  hazard. But it is a divergence from the program we are a clone of, in a
  direction no script can detect except by failing.
* **(b) Return GNU's value, silently, as GNU does.** *(chosen)*
* **(c) Return GNU's value AND warn on stderr.** *What changes:* the best of
  both, except that the warning is itself a divergence — the differential
  harness compares stderr in full, so this trades a silent numeric difference
  for a noisy textual one, and a script redirecting stderr still gets (b).

**Why (b) over (c).** (c) is tempting and I nearly took it. It fails on the
same ground that makes (b) right: the value is what callers consume, and adding
output they do not expect breaks the compatibility we are adding the value to
achieve. If the hazard of a silent sentinel is ever judged worse than the
divergence, the place to fix it is a `bc` warning flag that GNU also has, not a
message GNU never prints.

**Consequence:** `RuntimeError::LogOfNonPositive` had exactly one raise site
and is now unreachable, so it and its `Display` arm are deleted rather than
left as a variant nothing constructs.

**Reversing this** is restoring that variant and the one `return Err` in
`builtin_ln`, plus moving three harness rows back to `known_bug`.

## B-`dc`-CLAIMS-ARBITRARY-PRECISION-AND-COMPUTES-IN-`f64` (lane B, 2026-08-16) — **fixed 2026-08-16** (`62bd49957`, `2ccf4029a`; see `design-decisions.md` §324)

**Resolution:** `dc`'s numeric core is `bignum::Decimal`, the same type `bc`
computes on. `2 200 ^ p` prints all sixty-one digits exactly; 30! via the
standard factorial macro is exact; 22 expressions written in both syntaxes were
run through both programs and agree digit for digit. The two "fix during the
lift" items below were both done: `div`/`modulo`/`sqrt` return
`Result<_, DecimalError>` and no longer print-and-return-zero, and the parse and
format paths were hardened along with the rest of the crate under
`[lints] workspace = true` — which `bc` and `dc` had *both* been missing from
their manifests, so 31 defensive-lint warnings had never been reported for `bc`.

Moving `dc` onto the shared type immediately found three defects in it that
`bc`'s tests had not: the formatter trimmed trailing fractional zeros (making
`scale` unobservable — `5k 1 10 /` and `1k 1 10 /` printed the same string), the
base-ten parser scored `F` as 22 instead of 15 (so `FF` read as 242), and a
fraction read in a non-decimal base kept an inflated scale. Those were `bc` bugs
too. A fourth, in `dc` itself and inherited from the `f64` version: the
comparison commands were reversed, which turned every `dc` loop into a single
pass (30! answered 870). See the follow-on entry below for what remains
unverified.

**Original report follows.**



**In short:** `dc` is the desk calculator whose entire reason to exist is exact
arithmetic on numbers too big for a machine word. Its own first line of
documentation says "arbitrary-precision integers". It computes in `f64`, a
64-bit floating-point number that carries about 15–16 decimal digits. So it is
silently wrong above roughly 9,000,000,000,000,000, and wrong in the ordinary
way floating point is wrong below that — `0.1 + 0.2` is not `0.3`.

**Where:** `userspace/dc/src/main.rs` — `Value::Number(f64)` (line 17),
`as_number` (22), `format_number` (37, which casts to `i64` and gives up
entirely past `1e15`), `pop_number` (125), and `mod_pow(base as i64, …)` (259).

**How it shows:** `echo "99999999999999999999 99999999999999999999 * p" | dc`
should print the exact 40-digit product. `dc` is also the traditional back end
for `bc` — historically `bc` was a preprocessor that emitted `dc` — so the two
disagreeing about `1/3` at a given scale is a contradiction inside one tree.

**The proper fix, and it is already most-built:** `userspace/bc` has exactly the
right type — `BcNum`, a fixed-point decimal holding a `bignum::BigInt` mantissa
and a decimal `scale` (bc/src/main.rs:31–345, ~315 lines). It should be lifted
into the `bignum` crate as `Decimal`, the same move `BigInt` itself just made
and for the identical reason: `bc` and `dc` are one calculator with two
syntaxes, and a shared type is what stops them disagreeing. Then `dc`'s numeric
core is rewritten on it.

Two things to fix *during* the lift rather than after, since both are in the
code being moved:

* `div`, `modulo` and `sqrt` handle a bad argument by `eprintln!`-ing and
  **returning zero** — a library type writing to stderr and then answering with
  a plausible number. Division by zero has to reach the interpreter as a value
  it can refuse, not as a `0` the program keeps computing with.
* The parse and format paths index and slice strings directly (`body[..dot_pos]`,
  `abs_s[1..]`, `q.limbs[0]`) and add scales unchecked. `BigInt` was hardened
  against exactly this in `98cb065d0`; the decimal layer on top of it was not.

**Note on verification:** the differential harnesses that settled `grep`, `sed`,
`awk`, `expr` and `cat` are not available here — this host has neither GNU `dc`
nor GNU `bc`, and no package manager to add them. The substitute is a
cross-check between our own `bc` and our own `dc`, which is genuinely
informative *because* they are two independent front ends that must agree, plus
unit tests asserting exact values computed by hand. It is weaker than a
differential run and should be labelled as such wherever the result is claimed.

**Until then** `dc` is quietly wrong rather than loudly broken, which is the bad
kind: a script that uses it for big-integer work gets a rounded answer with no
diagnostic. Nothing in the tree depends on `dc` yet, so it is not blocking.

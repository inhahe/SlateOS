## TD-B-BIGNUM-SQRT-IGNORED-THE-PARITY-OF-ITS-INPUT-SCALE (lane B, 2026-09-16) -- **Status: FIXED** 2026-09-16

**In short:** `sqrt(4)` was `2`, and `sqrt(4.0)` was `.632…`. Writing a
trailing zero — which changes nothing about a number's value — changed the
answer, by a factor of the square root of ten. It affected **`bc` and `dc`
both**, since they share the number type, and any value with an odd count of
decimal places was hit: `sqrt(2.0)` gave `.4472…`, which is the square root of
0.2.

**Where:** `userspace/bignum/src/decimal.rs`, `Decimal::sqrt`.

**Why.** A `Decimal` is `digits / 10^scale`, so its root is
`sqrt(digits) / 10^(scale/2)` — and `scale/2` is only a whole number of decimal
places when `scale` is **even**. The code computed the working scale as
`self.scale + 2*result_scale + 2` and then halved it with `div_ceil(2)`. The
added part is always even, so the parity was the *input's*: an odd one rounded
the halving up and shifted the point half a place too far.

That is what made it depend on how the number was written rather than on what
it was — the defect's whole signature. It is also why it went unnoticed: every
literal anyone tests with (`2`, `4`, `0.25`, `1.0049`) has an even count of
decimal places, and so does every intermediate in the old `bc` math library.
It surfaced only when the new arctangent reduction started feeding `sqrt` a
value carried at the full working scale, which was odd.

**Found by:** the first attempt at
`TD-B-BC-MATHLIB-ARCTANGENT-IS-INACCURATE`, which returned `a(1) = 53.18`. The
trace showed `sqrt(1.0049)` answering `.317…` instead of `1.0024…` — a factor
of exactly `sqrt(10)`, which is what named the cause.

**Fixed** by rounding the working scale *up to even* before the halving, and
halving that same number rather than re-reading it from the rescaled value.
Two tests: one asserting that a trailing zero cannot change a root (six pairs
across six scales), and one pinning the known digits of `sqrt(2)`, `sqrt(4)`
and `sqrt(0.25)` so the pair test cannot pass by both sides being wrong the
same way. `dc` verified at the command line against GNU as well as `bc`.

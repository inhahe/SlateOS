## TD-B-BIGNUM-LONG-DIVISION-DROPS-A-BORROW-BIGGER-THAN-ONE-LIMB (lane B, 2026-09-16) -- **Status: FIXED** 2026-09-16

**In short:** dividing by a large enough number gave the wrong answer. Not the
last digit — a quotient with the wrong number of digits in it.
`(10^90)/144444444444444444444444444` was wrong, and so were the 36-, 40-, 42-,
45- and 54-digit divisors, while 9, 18, 28 and 37 were right. It is the `/`
operator, so it affected **`bc` and `dc` both**, and everything built on them:
`sqrt` is Newton's method, which is repeated division, and the `bc` math
library's arctangent is built on `sqrt`.

**Where:** `userspace/bignum/src/lib.rs`, `BigInt::divmod`, the
multiply-and-subtract step of Knuth's Algorithm D.

**Why.** The step subtracted the running borrow from the next *limb*:

    cur = slot - product_lo - borrow

`product_lo` and `borrow` are each free to approach the base, so `cur` could
reach about `-2*LIMB_BASE`. The code added one base back and cast to `u32`, so
whenever both happened to be large the cast wrapped a still-negative number and
that limb became garbage. Knuth folds the borrow into the next *product*
instead — `p = q_hat*v[i] + borrow` — which bounds `p` by `(B-1)^2 + B < B^2`
and therefore `slot - p_lo` by `-(B-1)`, where one base back is always enough.

**Why it hid for so long.** It needs a divisor of several limbs *and* limb
values that collide in that particular way, so it is invisible for small
divisors and erratic for large ones. Nothing in the suite divided by a number
that big, and the calculators' own tests use human-sized operands. The sizes
that failed and the sizes that passed are both recorded in the new test, so a
future fix that repairs one and breaks the other cannot look like progress.

**Found by:** chasing `sqrt` precision, three layers up. The chain is worth
keeping: a wrong arctangent led to a wrong `sqrt`, which led to a wrong
`isqrt`, which turned out to be a wrong `divmod`. Each layer's diagnosis was
confidently stated and two of the three were wrong about the layer beneath.

**Fixed** by folding the borrow into the product. Verified against GNU bc
1.07.1 for divisors of 9 to 54 digits, and the new test checks the defining
identity `q*d <= n < (q+1)*d` rather than a table of expected digits — it needs
no oracle and cannot be satisfied by a wrong quotient. Confirmed to fail
against the previous commit, reporting `27-digit divisor: quotient too large`.

**The rest of the arithmetic was then audited, and is clean.** A wrong `/` in
a shipped calculator is the kind of finding that should not be trusted to be
alone, so 1197 generated cases — `*`, `/`, `%`, `+`, `-`, `^`, `sqrt` and the
division identity, at operand sizes from 1 to 90 digits chosen to straddle the
9-digit limb boundary, plus fractional work at `scale=60` — were run through
both our `bc` and GNU's and compared. **All 1197 agree.** So the borrow was the
whole of it, and the other operators' limb carries are sound at sizes nothing
had previously reached.

That audit is a one-off and is not checked in: it needs WSL and a GNU
reference, which most runs of this tree do not have. What *is* checked in is
the part that needs neither —
`large_operand_arithmetic_obeys_its_own_definitions` in `decimal.rs`, which
asserts the same operators against their own definitions over the same size
grid with a fixed seed. That is the check whose absence let this bug live:
**nothing in the suite divided by a number that big.** It now does, on every
`cargo test`, with no external oracle to go stale.

---

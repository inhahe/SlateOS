## §323 — A calculator's runtime error abandons the top-level statement, and the session lives on

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** when `bc` is asked to divide by zero, three things could happen:
it could print a complaint and keep going as if the division had produced
something (it used to produce zero); it could quit; or it could throw away the
one command that went wrong and read the next one. The third is what it does
now. Concretely, `x = 1/0 + 5` used to leave `x` set to **5** — the division
announced itself on stderr and then contributed a zero that the addition
happily consumed. Now it leaves `x` untouched, prints `Runtime error: divide by
zero`, and the next line of the script still runs.

### What made the choice necessary

Nothing in the old design *chose* the first behaviour; it fell out of the
number type having no way to say "no answer". `BcNum::div` returned a `BcNum`,
so when the divisor was zero it had to return *some* `BcNum`, and the only
candidate was zero. Lifting the type into `bignum` as `Decimal` (§ the
`known-issues.md` entry that started this work, and `userspace/bignum/src/decimal.rs`)
changed the signature to `Result`, and a `Result` cannot be ignored. So the
question "what should bc do about a runtime error?" had to be answered
explicitly for the first time.

There are three defensible answers and they are genuinely different products.

| | *What changes* | Against it |
|---|---|---|
| **Poison the value** — keep returning a sentinel and let the expression finish | nothing observable changes; `1/0 + 5` is still `5` | the user is told about an error *and* handed a number computed from it. The diagnostic and the result disagree, and the result is the one that gets used |
| **Quit on the first error** | `bc script.bc` stops at the bad line; nothing after it runs | one mistyped expression in the middle of a long script discards every result after it, including the ones already correct. Interactive bc would exit on a typo |
| **Abandon the statement** (chosen) | the failing top-level statement produces nothing; the next one runs normally | the granularity is coarse — a division by zero on the last iteration of a loop discards the loop, not just the iteration |

### Why the statement, and not something finer

The obvious wish is to abandon less: fail the division, keep the loop. It
cannot be done, and the reason is worth writing down because it will be asked
again. An expression that failed has no value, so there is nothing to resume
*with* — every enclosing expression is waiting on a number that does not exist.
Recovery has to happen at a boundary where no value is owed, and in bc the
nearest such boundary above an expression is the top-level statement. Anything
between the two would require inventing a value, which is exactly the poisoned
answer this decision rejects.

That the unwind crosses function frames is a consequence, not a separate
choice, and `call_func` is written so the frame is torn down on the failing
path as well as the succeeding one. A `?` in the middle of that function would
have left the callee's `auto` variables shadowing the caller's for the rest of
the session — the error would have corrupted the state it was supposed to be
protecting.

### Why the math library still uses the fallible operations

Every division in bc's `-l` library — the Taylor series for `sin`, `cos`,
`exp`, `ln`, the `atan` series, the Bessel functions — divides by a loop
counter or a constant that provably cannot be zero. A `div_nonzero` helper
that returned a plain `Decimal` would be honest about that and would remove
about forty `?`s.

It is deliberately not used. "This divisor cannot be zero" is a claim about the
series code, not about arithmetic, and the value of routing it through the
fallible path is precisely what happens when the claim turns out to be false:
the user sees `Runtime error: divide by zero` and the statement is abandoned,
rather than a series quietly converging to the wrong number with no indication
that anything happened. The `?`s are cheap; a silently wrong `sin` is not.

### Where it lives

`userspace/bc/src/main.rs`: `RuntimeError`, `type Eval = Result<Decimal,
RuntimeError>`, and `run`, which is the single place a `RuntimeError` is ever
printed. `userspace/bignum/src/decimal.rs` is where the fallibility originates.
`dc` inherits this decision when its numeric core moves onto `Decimal`, since
it is the same calculator with a different syntax.

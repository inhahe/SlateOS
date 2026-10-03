## TD-B-BC-MATHLIB-LOG-ERRORS-WHERE-GNU-SATURATES (lane B, 2026-08-24) -- **Status: FIXED** 2026-09-16

**In short:** `l(0)` — the natural logarithm of zero — has no answer as a real
number. GNU `bc` returns a very large negative number rather than complaining;
ours prints `Runtime error: log of non-positive number` and computes nothing.
A script that takes the log of a value which happens to be zero gets a number
from GNU and a diagnostic from us.

**Where:** `userspace/coreutils/src/bin/bc.rs`, `RuntimeError::LogOfNonPositive`
and the `l(x)` builtin that raises it.

**Measured**, `scale=10`, `bc -l`:

| Call | GNU stdout | GNU stderr | Ours |
|---|---|---|---|
| `l(0)` | `-9999999999.0000000000` | *(none)* | *(none)*, plus `Runtime error: log of non-positive number` |

`-9999999999` is ten nines — one per digit of `scale`, which is 10 here. That
is probably not a coincidence, and it is the thing to check before implementing:
GNU's `l` looks like it saturates at a magnitude tied to the current scale
rather than returning a fixed constant. **Measure `l(0)` at `scale=5`,
`scale=20` and `scale=50` before writing the fix.** A hard-coded
`-9999999999` that happens to match at `scale=10` would be a new bug wearing
the old one's clothes.

**Not yet measured, and needed:** `l(-1)`. Negative arguments may or may not
saturate the same way; the row above establishes only zero.

**The proper fix:** whatever the scale sweep shows, applied in `l(x)`'s
zero/negative path in place of the `RuntimeError::LogOfNonPositive` return. If
it turns out GNU does error for negatives and only saturates at zero, then
`LogOfNonPositive` stays and only its zero case changes — in which case its
wording also falls under `TD-B-BC-RUNTIME-ERROR-WORDING-DIFFERS-FROM-GNU`.

### Fixed 2026-09-16

The sweep this entry demanded was done first, and it earned its keep: the
value is **`-(10^scale - 1)`** rendered at the current scale, not a constant.

| scale | GNU |
|---|---|
| 0 | `0` |
| 1 | `-9.0` |
| 5 | `-99999.00000` |
| 10 | `-9999999999.0000000000` |
| 20 | twenty nines |
| 50 | fifty nines |

`scale=0` answering `0` rather than a signed zero is the formula agreeing with
itself, `10^0 - 1` being zero. The entry's warning was exactly right: a
hard-coded `-9999999999` matches at `scale=10` and nowhere else, and would have
passed the single harness row that existed. That row is now three, at scales 1,
10 and 20, because one sample cannot tell a formula from a constant.

**The unmeasured question is answered: negatives saturate too.** `l(-1)`,
`l(-100)` and `l(-0.5)` all return what `l(0)` returns, so GNU has no error
path here at all. That settles the entry's conditional — `LogOfNonPositive`
does not stay. It had exactly one raise site, is now unreachable, and is
deleted along with its `Display` arm rather than left as a variant nothing
constructs.

**Why match GNU rather than keep erroring** is `design-decisions.md` §1026,
which also answers the obvious objection that this contradicts §1025's refusal
to invent a value. The distinction is who defines it: `adr=` had no meaning
outside GNU's own internals, while `-(10^scale - 1)` is reproducible and
scripts can already depend on it — an underflow guard like
`if (l(x) < -1000000)` works on GNU and would break against a `bc` that
errored.

**Evidence.** All six scales × four non-positive arguments match GNU exactly.
`bc-diff.sh` 141 -> 147 passed, known bugs 5 -> 3. The unit test carries a
control — `l(1)`, `l(2)`, `l(7)`, `l(0.5)` and the round trip `e(l(7))` — so
that an `l` returning the sentinel for *every* argument could not pass. That
round trip is asserted as `6.9999999996`, which is what GNU answers and what we
answer; the first draft asserted `7.0000000000`, which is a claim about
arithmetic nobody had performed.

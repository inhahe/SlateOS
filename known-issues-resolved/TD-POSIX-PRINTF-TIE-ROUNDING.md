### TD-POSIX-PRINTF-TIE-ROUNDING. The float formatter rounded ties away from zero; glibc/musl round ties to even — 2026-07-30 — RESOLVED 2026-07-30

**Where:** `posix/src/printf.rs::fmt_fixed`, the `if f >= 0.5` test that
triggers the digit carry (~line 1884), and the `crate::math::round` pre-round
in the `%e` path (~line 1990).

**What it is:** when the value sits exactly halfway between two representable
outputs, our engine always rounds away from zero, whereas glibc and musl honour
the current FPU rounding mode — `FE_TONEAREST`, i.e. ties-to-even. Observed
divergences (both exactly representable, so this is a genuine tie, not a
representation artefact):

| format | value | ours | glibc |
|---|---|---|---|
| `%.3e` | `1234.5` | `1.235e+03` | `1.234e+03` |
| `%.1f` | `8.25` | `8.3` | `8.2` |
| `%.0f` | `2.5` | `3` | `2` |

**Impact:** low but real. C11 §7.21.6.1p13 leaves the tie direction
unspecified ("in an implementation-defined manner"), so this is not a
conformance bug — but it *is* an observable behaviour difference that will
show up as a one-digit diff in any test suite whose expected output was
generated on Linux. It was found while writing the `%Lf` regression tests
(two of my first expectations, copied from glibc behaviour, failed).

**Fix (DONE).** The note above was right that the running remainder cannot be
trusted to recognise a half — but the exact test turned out to be cheap,
because "is this a tie?" is a question about the value's *binary*
representation, not its decimal expansion. Writing a finite `val` as `m * 2^e`
with `m` odd,

    val * 10^p * 2  =  m * 5^p * 2^(e + p + 1)

and rounding to a multiple of `10^-p` is a tie exactly when that is an odd
integer — i.e. iff `e + p + 1 == 0`, plus (for negative `p`, where the `5^p`
is a division) `5^-p | m`. One exponent comparison, no dependence on the digit
loop. `printf.rs` gained `decompose`, `is_half_way` and `is_odd_integer`
implementing this, and:

- `fmt_fixed`'s carry test became "if this is an exact half, round towards an
  even last digit; otherwise the old `f >= 0.5`". Non-tie behaviour is
  bit-identical to before, which is why all 20 000+ existing assertions passed
  unchanged.
- `fmt_fixed`'s `precision == 0` pre-round uses a new `round_half_even` rather
  than `math::round`. It needs none of the above: `val - floor(val)` is exact
  for every finite `f64` (Sterbenz for `val >= 1`, trivially below), so
  `== 0.5` is already an exact tie test there.
- `fmt_scientific` asks about the **original value at `precision - exp`
  places** rather than the derived mantissa, which is inexact by the time it
  exists. That is what makes `%.3e` of `1234.5` come out as `1.234e+03`.

All three divergences in the table above now match glibc, covered by
`fixed_ties_round_to_even`, `fixed_ties_to_even_carries_through_nines`,
`scientific_ties_round_to_even` and `fixed_non_ties_are_unaffected` (a guard
that the ordinary path is untouched).

**Superseded 2026-07-30 by BUG-POSIX-PRINTF-INEXACT-DIGITS.** The digit loop
this fix bolted an exact tie test onto has since been replaced outright by an
exact decimal expansion, which makes ties fall out of the digits themselves —
"the first dropped digit is a 5 and nothing follows it". `is_half_way`,
`is_odd_integer`, `round_half_even` and printf's copy of `decompose` are gone;
the behaviour they produced is unchanged and still covered by the tests named
above.

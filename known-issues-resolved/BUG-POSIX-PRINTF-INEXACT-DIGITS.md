### BUG-POSIX-PRINTF-INEXACT-DIGITS. `%f` printed 20 digits of garbage for every value at or above 2^64, and every float conversion drifted after ~17 significant digits — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/printf.rs::fmt_fixed` / `fmt_scientific` /
`format_float_general`; now `posix/src/decfloat.rs`.

**What it was:** the digit generator peeled the integer part off with
`val as u64` and produced fraction digits by repeatedly multiplying the
remainder by ten. Both steps are wrong:

- **The cast saturates.** Every finite value at or above 2^64 produced the
  *same* output, `18446744073709551615`, followed by whatever the fraction
  loop made of the resulting garbage remainder. Measured:

  | format | value | ours (before) | correct |
  |---|---|---|---|
  | `%.2f` | `1e20` | `18446744073709551615./0` | `100000000000000000000.00` |
  | `%.2f` | `1e25` | `18446744073709551615./0` | `10000000000000000905969664.00` |
  | `%.2f` | `DBL_MAX` | `18446744073709551615./0` | the 309-digit expansion |

  That is a silent wrong answer for an ordinary, in-range `double` — the
  worst class of formatting bug, and it applied to `%f` and to the `%f` half
  of `%g`.

- **`remainder *= 10.0` rounds,** so the digits drift after roughly the
  seventeenth significant one. `%.30f` of `0.1` printed
  `0.100000000000000000000000000000`, where the value really is
  `0.100000000000000005551115123126`.

- Separately, `%e` and `%g` derived their exponent from
  `floor(log10(val))`, which is neither exact at powers of ten nor correct at
  a rounding boundary (`%.2g` of `99.9` must switch style, because rounding to
  two significant digits carries it to `1.0e+02`).

**Fix (DONE).** New module `posix/src/decfloat.rs` computes the **exact**
decimal expansion, which is possible because every finite `f64` has a finite
one. Writing the value as `m * 2^e` with `m` odd,

    e >= 0:   val = (m << e) * 10^0
    e <  0:   val = (m * 5^-e) * 10^e

since `2^e == 5^-e * 10^e`. Either way the value is an *integer* times a power
of ten, so converting that integer to decimal yields every digit it has with
nothing rounded. The integer needs at most `53 + 1074*log2(5) = 2548` bits, so
a fixed `[u64; 40]` covers the entire `f64` range with no allocation — which
matters inside a freestanding libc's `printf`. Reducing `m` to odd first keeps
the common case to a handful of limbs.

`fmt_fixed`, `fmt_scientific` and `format_float_general` now round that
expansion once, at the place their format asks for, and copy digits. Because
the expansion is exact, ties are recognisable directly ("first dropped digit is
5, rest is zero"), so the separate tie machinery added earlier the same day
(TD-POSIX-PRINTF-TIE-ROUNDING) is subsumed, and `%g` reads its exponent off the
rounded expansion instead of `log10`.

A `%.*f` precision may exceed the 1074 fraction digits an expansion can have;
the surplus is always a run of zeros, so `FloatText` records where they belong
rather than materialising them. That keeps the stack buffer bounded by the
`f64` range instead of by user-supplied precision, and they are still printed
and still counted for field width — `%.400f` used to truncate.

**Verification:** `decfloat` is checked digit-for-digit against Rust's own
(exact, ties-to-even) formatter across 2000 values × 5 precisions, plus
subnormals, `DBL_MAX` and the smallest positive normal. `printf` itself is
checked the same way end-to-end for `%f` *and* `%e` across 600 values × 5
precisions. All 20 065 posix tests pass — the ~20 000 pre-existing formatting
assertions unchanged, which is the evidence that ordinary output is
byte-identical. Clippy clean; the six ring-3 C fixtures relinked and green in
QEMU.

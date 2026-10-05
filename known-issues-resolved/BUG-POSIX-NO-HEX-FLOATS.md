### BUG-POSIX-NO-HEX-FLOATS. Hexadecimal floating point was missing in both directions: `printf %a` printed nothing and `strtod`/`scanf` could not read `0x1.8p+3` — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/printf.rs` (no `b'a' | b'A'` arm in the conversion
dispatch), `posix/src/decfloat.rs` (the scanner had only a decimal grammar),
`posix/src/scanf.rs::scan_float_digits`.

**What it was:** C99 added `%a`/`%A` and the matching `0x…p±d` input syntax to
`strtod` and `scanf`. We had neither. `printf("%a", 1.5)` fell through the
dispatch and emitted nothing at all — not even the literal `%a` — and
`strtod("0x1.8p+1", &end)` stopped at the `0`, returning 0.0 with `end`
pointing at the `x`.

The pair matters more than either half. Hexadecimal is the one textual form of
a `double` that is *exact*: sixteen is a power of two, so the digits are the
bits and there is no base conversion to round. Every finite `double` has an
exact `%a` form in at most 13 fraction digits, so `%a` → `strtod` is the
identity — which is what makes it the format for serialising floats, for test
vectors, and for reading a value out of a debugger or a standard's text. It is
also what glibc's own test suites and many ported build systems print with.

**Fix (DONE).**

*Output* — `format_float_hex` in `printf.rs`, following glibc: `[-]0xh.hhhhp±d`
with leading digit `1` for normals and `0` for zero and subnormals, a
subnormal's exponent pinned at `p-1022` rather than normalised, an absent
precision meaning the shortest exact form (trailing zero digits dropped), and
an always-signed exponent of at least one digit. `emit_float_padded` grew a
prefixed form so `0x` lands after the sign but before zero padding, which is
where `%016a` has to put it.

One subtlety cost a rewrite: rounding is ties-to-even on the last *retained*
digit, and at precision 0 that digit is the **leading** one, not part of the
fraction. `%.0a` of 3.0 (`0x1.8p+1`) is an exact tie whose leading `1` is odd,
so it rounds up to `0x2p+1` — carried into the leading digit rather than
renormalised. Checked against glibc before the test was written.

*Input* — `scan_hex_body`/`hex_to_binary` in `decfloat.rs`. `DigitCollector`
gained a `hex` flag; in that mode its exponent counts powers of two, each digit
shifts the accumulator by 4, and the result goes through the same
`round_to_binary` as the decimal path, so all four conversions (decimal and hex
× `f64` and `f32`) round exactly once, in their own precision. Only
`MAX_HEX_DIGITS = 20` digits are kept — 80 bits, well past a `double`'s 53 plus
guard — because with no base conversion a more distant digit can only make the
tail nonzero, which is what the sticky bit already records. Saturating
arithmetic in the rounder makes `0x1p1000000000000` → inf and
`0x1p-1000000000000` → 0 fall out without a special case.

**Verification:** `%a` is tested against glibc's output on plain values, the
whole exponent range, subnormals, ties, over-long precisions, `#`/`+`/space
flags and zero padding; then round-tripped over 2000+ random bit patterns
twice — once through an independent reference parser written for the test (so
two matching bugs cannot agree), once through the real `strtod`. Parsing is
tested for `endptr` placement, ties-to-even, the full range, `ERANGE`, `f32`
precision, and backing out of an incomplete `0x` prefix. 20 112 posix tests
pass; clippy clean.

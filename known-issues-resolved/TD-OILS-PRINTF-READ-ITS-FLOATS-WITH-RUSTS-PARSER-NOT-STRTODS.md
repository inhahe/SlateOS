### TD-OILS-PRINTF-READ-ITS-FLOATS-WITH-RUSTS-PARSER-NOT-STRTODS. `printf %f` refused hex floats, named the wrong base when it refused, spelled `nan`/`inf` four different ways and let the `0` flag pad them — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `parse_printf_float_checked`,
`parse_printf_int_checked`, `format_g`, `format_a`, and the `%f`/`%e` arms of
the printf formatter.

**What:** four separate divergences, all from reading and writing a float with
Rust's own spellings rather than C's.

1. **Hex floats.** bash's `printf` reads a float with `strtod`, whose grammar
   includes `[+-]? 0[xX] HEX* ('.' HEX*)? ([pP][+-]?DIGIT+)?` — the binary
   exponent is **optional**. So `0x10` is 16, `0xa.b` is 10.6875, `0X1P-2` is
   0.25 and `0x.8p1` is 1. Rust's `f64::from_str` has no hex form at all, so
   every one of these was a complaint. The partial forms matter too: `strtod`
   backs up to the longest valid prefix, so `0x` is 0, `0xg` is 0, `0x1p` is 1
   (the `p` with no digits is not consumed) and `0x1p4z` is 16 with rc 1.

2. **The base the complaint names.** The radix `strtoimax`/`strtod` reads in is
   chosen *after* stepping over blanks and a sign, and either case of `x` will
   do. The radix the *message* names comes from `sh_invalidnum`, which looks at
   the raw word's **first two bytes only**, tries the octal arm first, and
   accepts only a lowercase `x`. So `0X3z` is read as hex (value 3) but reported
   as a plain `invalid number`, and so are ` 0x3z` and `-012x`, while `012x` is
   an `invalid octal number` and `0x3z` an `invalid hex number`.

3. **Non-finite spelling.** C spells these by the *case of the conversion
   letter*: `nan`/`inf` for `%f %e %g %a`, `NAN`/`INF` for `%F %E %G %A`, with
   the sign bit shown (`-nan` is a thing). osh had three hand-rolled spellings
   and one arm with none.

4. **The `0` flag.** C ignores it for a value with no digits, so
   `printf "[%08f][%08G][%08d]\n" nan -inf 5` is `[     nan][    -INF][00000005]`
   — the integer still pads with zeros, the floats do not.

**Fixed** by a new `parse_hex_float` implementing strtod's hex grammar (u128
mantissa capped at 28 hex digits with a sticky low bit so rounding is correct,
bounded `powi` scaling so intermediates cannot spuriously overflow), by routing
both number readers' messages through the `sh_invalidnum_kind` helper the
previous commit added, by naming the non-finite predicate once in
`format_nonfinite`, and by suppressing `zero` for a rendered float body that
contains no digit — which, since every finite float renders at least one digit,
is exactly the non-finite case. Pinned by
`printf_reads_the_hexadecimal_float_strtod_reads`,
`printf_names_the_base_from_the_word_not_from_the_radix`,
`printf_spells_a_non_finite_value_by_the_letters_case` and the corpus case
`printf-reads-a-float-the-way-strtod-does.sh`.

**Standing lesson:** a conversion is two grammars, not one — what the host
library *reads* and what it *writes* — and neither is Rust's. The four bugs
here are one bug seen four times: `f64::from_str`/`{}` was standing in for
`strtod`/`printf`, and it differs at the edges in both directions.

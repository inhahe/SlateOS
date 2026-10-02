### TD-POSIX-LONG-DOUBLE-PRECISION. `long double` has the right *ABI* but only `double` (53-bit) *precision* — ACCEPTED LIMITATION 2026-07-30

**Status (2026-09-28): FIXED.** Nothing in the C library narrows a `long
double` to a `double` any more, except where a C signature does
(`nexttoward`). The maths functions compute in 80 bits on the x87 unit
(`posix/src/mathl.rs`, `ld80.rs`, `ld_abi.rs`; design-decisions §1134), and
the conversions carry all 64 bits of the significand: `printf`'s `%La %Le %Lf
%Lg` print the value's exact digits, and `strtold`, `strtold_l`, `wcstold` and
`scanf`'s `%Lf` round the text into the 80-bit format -- both in the current
rounding direction, and over the whole range, subnormals and all
(`posix/src/decfloat.rs`, design-decisions §1138). Replayed against glibc
2.39: 15,780 `printf` calls of long doubles, the encodings the unit rejects
among them, and 624 `strtold` calls (`wcstold` the same 624), literals of
11,500 digits among them, in all four rounding modes
(`posix/tools/oracle/conv_harness.py`). What follows is the entry as it
stood.

**Where:** `posix/src/x87.rs` (`to_f64`/`from_f64`), `posix/src/printf.rs`
(`va_arg_long_double`), `posix/src/stdlib.rs` (`strtold`).

**What it is:** the sysroot now moves `long double` values correctly — 16
bytes, 16-byte aligned, MEMORY class, x87 80-bit encoding — but it does not
*compute* in 80-bit. Every long double that enters the sysroot is decoded to
an `f64` at the boundary, processed with 53-bit arithmetic, and (where a long
double is produced) re-encoded. Consequences:

- `printf("%.25Lf", x)` prints correctly-formed output, but only ~17
  significant digits are meaningful; the tail is the `f64` rounding of `x`,
  not `x`.
- A value with an exponent outside f64's range (|x| > ~1.8e308, or below
  ~4.9e-324) saturates to ±inf or 0 rather than being represented exactly,
  even though x87 has the range for it. `to_f64` handles this deterministically
  (round-to-nearest, overflow → ±inf) rather than producing a wrong finite
  number, so it degrades predictably.
- There are no `sqrtl`/`powl`/`fabsl`/… in the sysroot at all. That is the
  *safe* failure mode: a link error, not a silently wrong answer. (No longer
  so: they exist, in 80 bits, since 2026-09-28 -- §1134.)

**Why accepted:** the double-precision core is shared with every other float
path in the sysroot and is well tested; an 80-bit software arithmetic layer
would be a large, subtle, and — for our workloads — currently unused
subsystem. The ABI half was the part that was actively corrupting data
(BUG-POSIX-LONG-DOUBLE-ABI); the precision half only under-delivers.

**Proper fix (when a consumer needs it):** implement arithmetic directly on
`x87::LongDouble` (64-bit significand + 15-bit exponent) — add/sub/mul/div and
comparison first, then the `…l` libm entry points — and change
`va_arg_long_double`/`strtold` to keep the full 64-bit significand instead of
narrowing. Alternatively, use the FPU: the sysroot runs on x86-64 with a real
x87 unit, so `fldt`/`fstpt` plus x87 opcodes would give exact 80-bit semantics
for a fraction of the code, at the cost of having to manage FPU state (control
word, stack depth) across the ABI boundary. Trigger: the first port that
genuinely relies on `long double` precision (many numeric C libraries do).

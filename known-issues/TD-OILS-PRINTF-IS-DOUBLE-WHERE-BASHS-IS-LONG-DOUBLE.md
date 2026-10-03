### TD-OILS-PRINTF-IS-DOUBLE-WHERE-BASHS-IS-LONG-DOUBLE. bash reads printf's floats with `strtold` and writes them with the `%L` conversions, so osh's `f64` differs in range, in precision and in the out-of-range warning — 2026-08-04 — OPEN (structural)

**Where:** `userspace/oils/src/interp.rs` — `parse_printf_float_checked` and
every float arm of the printf formatter.

**What:** on x86 `long double` is the 80-bit extended format: 64 mantissa bits
against a double's 53, and an exponent range reaching ~1e4932 against ~1e308.
Measured against bash 5.2.37:

```sh
printf '%.25f\n' 0.1        # 0.1000000000000000000013553   osh: …0000000000555112
printf '%.0f\n'  9007199254740993   # exact                 osh: 9007199254740992
printf '%e\n'    1e309      # 1.000000e+309                 osh: inf
printf '%f\n'    1e4933     # printf: warning: 1e4933: Numerical result out of range
```

**Why deferred:** this is not a bug in a parse or a format string — it is the
arithmetic type. Fixing it properly means an 80-bit soft-float: an `f80`
mantissa/exponent pair with `strtold`-grade decimal→binary conversion and a
correctly-rounded `printf` back out. That is a self-contained project, and it
touches nothing else in the shell, so it is worth doing on its own rather than
bolted onto a printf fix.

**Proper fix:** a small `f80` module (add/mul/scale by powers of ten, decimal
parse with round-to-nearest-even, `%f`/`%e`/`%g`/`%a` rendering via exact
integer expansion), used only by printf. Everything else in osh is integer
arithmetic and is unaffected. Then extend
`printf-reads-a-float-the-way-strtod-does.sh` with the values above, which it
currently avoids on purpose — its header says every value in it is exact in a
double precisely so the case does not measure this gap.

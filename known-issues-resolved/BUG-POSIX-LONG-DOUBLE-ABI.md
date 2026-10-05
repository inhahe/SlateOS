### BUG-POSIX-LONG-DOUBLE-ABI. `long double` was treated as if it were `double` — `printf("%Lf")` desynchronised every later argument and `strtold` returned in the wrong register — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**What.** The sysroot implements `long double` by pretending it is `double`.
That is a defensible *precision* limitation (see
TD-POSIX-LONG-DOUBLE-PRECISION), but it was also applied to the **ABI**, where
it is not a limitation but silent corruption. Two distinct failures:

1. **`printf`/`vsnprintf` never consumed the `L` length modifier.**
   `posix/src/printf.rs` had two parallel length-modifier parsers — one in
   `va_collect` (the argument-collection pass) and one in `parse_spec` (the
   formatting pass) — and *neither* listed `b'L'`. So in `printf("%Lf", x)`
   the `L` was left in place and read as the **conversion character**. `L`
   matches no conversion, so the specifier consumed **no argument at all**,
   emitted nothing useful, and left the `va_list` cursor pointing at the
   `long double`'s 16 bytes. Every subsequent argument was therefore read
   16 bytes early — an off-by-two-slots shift that turns integers into
   fragments of a mantissa and pointers into wild addresses. This is far
   worse than losing precision: it corrupts unrelated arguments.

2. **`strtold` returns its result in the wrong register.**
   `posix/src/stdlib.rs:582` is `pub unsafe extern "C" fn strtold(...) -> f64`
   delegating to `strtod`. Under the SysV x86-64 ABI a `double` return goes in
   `%xmm0`, but `long double` classifies X87/X87UP and is returned in
   **`%st(0)`**. Any C caller compiled against a real `<stdlib.h>` reads
   `%st(0)`, which at that point holds whatever the x87 stack happened to
   contain — so `strtold` returns garbage regardless of the input string.
   The same argument applies to every `…l` function the sysroot might grow.

**Why it went unnoticed.** `long double` is rare in the C we currently build,
and the ring-3 fixtures (`ctest-libc-float`, `ctest-libm`) exercise only
`double`/`float`. Nothing in the tree calls `%Lf` or `strtold` yet, so the
corruption had no visible victim — it was waiting for the first port that
uses `long double`.

**Repro (once a fixture exists).**
`printf("%Lf %d\n", (long double)1.5, 7)` prints a mangled first field and
the wrong integer; `strtold("2.5", NULL)` returns a value unrelated to 2.5.

**Fix, part 1 — printf (DONE).** Added `posix/src/x87.rs`: a single home for
the 80-bit format, with `LongDouble` (`#[repr(C, align(16))]`, explicit
integer bit) plus `to_f64`/`from_f64` that round **once** directly to the
target precision (the obvious "convert then `ldexp`" shortcut double-rounds
and is an ulp off for some subnormals). Then in `printf.rs`: the two
duplicated length-modifier parsers were replaced by one shared
`skip_length_modifier`, which reports whether the modifier was `L`, and
`va_collect` uses `va_arg_long_double` for `L` conversions — reading from the
overflow area only (X87/X87UP resolves to MEMORY, so it touches neither
`gp_offset` nor `fp_offset`), rounding the cursor up to 16 and advancing it
by 16. Six regression tests in `printf::tests` cover the value, all of
`%Le/%Lg/%LF`, two consecutive long doubles, a long double followed by a
stack-passed integer, re-alignment after an odd 8-byte word, and a
wide-exponent value that proves the x87 decode is real.

**Fix, part 2 — `strtold` (DONE).** A Rust function cannot express an `%st(0)`
return, so the export was renamed via
`#[cfg_attr(target_os = "none", unsafe(export_name = "__strtold_f64"))]` and a
target-only `global_asm!` thunk now exports `strtold`: it calls the Rust
function, spills `%xmm0` to the stack and `fld`s it, leaving the value in
`%st(0)`. Verified by disassembling `libc.a` (`fldl (%rsp)` present, both
symbols defined).

**Fix, part 3 — `scanf` (DONE).** `scan_float` now stores 16 bytes through
`crate::x87::from_f64` for `%Lf` (unaligned, since the pointer comes from C),
and the modifier table gained `L`. Adding `L` to the prescan exposed that
`z`, `j` and `t` were missing from the *main* parser too, so `%zu`/`%jd`/`%td`
were silently broken in exactly the same way; fixed in the same change. Five
regression tests.

**Fix, part 4 — the entry points (DONE).** The `%L` fix in `va_collect` was
initially unreachable from the *direct* printf family: its six assembly
trampolines flattened the varargs into two `[u64; 8]` arrays, a
representation that cannot express a MEMORY-class `long double` at all. Two
argument-delivery paths into one engine is exactly what let a fix to one miss
the other, so all twelve trampolines (six plain, six `__*_chk`) were rewritten
around a shared `va_trampoline!` macro that performs a real `va_start` and
delegates to the `v*`/`__v*_chk` function. (The flat arrays still exist one
level down and carry their own bug — see BUG-POSIX-PRINTF-ARG-ARRAY-OOB.)

**Fix, part 5 — ring-3 fixtures (DONE).** `services/ctest-longdouble` (plain
C, `zig cc`, exit 42 = pass) guards the type's shape, `%Lf`/`%Le`/`%Lg`
including the argument-desync case, `strtold` called 32× to catch an x87
stack leak, `%Lf` in `scanf`, and a format/reparse round trip.
`services/ctest-fortify` does the same through the `__*_chk` family, which
has no host coverage at all because the trampolines exist only on the
bare-metal target. Both run from `kernel/src/proc/spawn.rs` and pass in QEMU.

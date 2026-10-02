## D-POSIX-LIBM-LACKS-GLIBC-EXTENSIONS — glibc's libm exports about 150 functions ours does not: the long double complex and Bessel functions, and C23's newer families (lane D, 2026-09-28) — **Status: FIXED 2026-09-29, the last two `fegetmode` and `fesetmode` (`posix/src/fenv.rs`, glibc's `fegetmode.c` and `fesetmode.c`, with `posix/include/fenv.h`'s `femode_t`). Before them: the 22 `long double` complex functions done 2026-09-28, `posix/src/complexl.rs`: replayed against glibc 2.39 for 27,134 calls, and from C in ring 3, `ctest-longdouble` 92-99; the exact C23 functions -- `nextup` to `fminimum_mag_num`, all three precisions -- and `scalbl` done the same day, `posix/src/c23math.rs`, every value, flag and `errno` of glibc 2.39's for 21,390 calls; the eighteen narrowing functions the same day, `posix/src/narrow.rs`, glibc's round to odd, every value, flag and `errno` of its for 52,876 calls in the four rounding directions; `clog10`, `clog10f` and `clog10l` the same day, glibc's algorithm in `complex.rs` and `complexl.rs`, replayed against glibc for 3,212 calls; the six `long double` Bessel functions 2026-09-29, `posix/src/besl.rs`, written from the mathematics (design-decisions §1140): 13,338 of 13,339 values mpmath's correctly rounded ones in all four directions, glibc's special values, flags and `errno` at 8,136 calls)**

**In short:** a C program that calls one of the functions below does not
link. None is in C99; they are C23 additions, GNU extensions, or the `long
double` versions of functions the library has for `double`. The list is exact:
every name glibc 2.39's `libm.so.6` exports that `libc.a` does not, less the
`_FloatN` aliases and glibc-internal names (`comm` of the two symbol tables).

| Family | Names | Notes |
|---|---|---|
| `long double` complex | `cabsl` `cacosl` `cacoshl` `cargl` `casinl` `casinhl` `catanl` `catanhl` `ccosl` `ccoshl` `cexpl` `cimagl` `clogl` `conjl` `cpowl` `cprojl` `creall` `csinl` `csinhl` `csqrtl` `ctanl` `ctanhl` | **done 2026-09-28** (`complexl.rs`): FreeBSD msun's `ld80` versions where it has them, `complex.rs`'s `double` algorithms carried to 80 bits for `ccoshl`, `csinhl`, `ctanhl` and their circular twins, glibc's `cpowl`; with `__mulxc3` and `__divxc3`, which a C compiler calls for `long double complex` `*` and `/` and which were missing too |
| GNU complex | `clog10` `clog10f` `clog10l` | **done 2026-09-28**: glibc's `s_clog10_template.c`, its exact `x^2 + y^2 - 1` near `|z| = 1` included |
| `long double` Bessel | `j0l` `j1l` `jnl` `y0l` `y1l` `ynl` | **done 2026-09-29** (`besl.rs`): musl and FreeBSD have none and glibc's are LGPL, so derived here -- Miller's recurrence and the Neumann series to 48, Hankel's expansion in phase and amplitude past it, Taylor series about each zero below 48, all in double-long-double arithmetic and rounded once (§1140), 3 to 31 microseconds a call; from order 512 Debye's expansions (D-POSIX-BESSEL-HUGE-ORDERS-ARE-SLOW, fixed) |
| C23, all three precisions | `nextup` `nextdown` `llogb` `canonicalize` `fromfp` `fromfpx` `ufromfp` `ufromfpx` `getpayload` `setpayload` `setpayloadsig` `totalorder` `totalordermag` `fmaxmag` `fminmag` `fmaximum_mag` `fminimum_mag` `fmaximum_mag_num` `fminimum_mag_num` (each with `f` and `l`) | **done 2026-09-28** (`c23math.rs`): glibc's code, bit for bit, down to the x87 encodings it refuses |
| C23, `long double` only | `fmaximuml` `fminimuml` `fmaximum_numl` `fminimum_numl` | **done 2026-09-28**, all twelve (`c23math.rs`, with oracle rows): the `double` and `float` ones are ours now, where they were compiler_builtins' weak exports |
| C23 narrowing | `fadd` `faddl` `fsub` `fsubl` `fmul` `fmull` `fdiv` `fdivl` `fsqrt` `fsqrtl` `ffma` `ffmal` `daddl` `dsubl` `dmull` `ddivl` `dsqrtl` `dfmal` | **done 2026-09-28** (`narrow.rs`): round-to-odd in the wider one, then round, as glibc's `math-narrow.h` |
| XSI, obsolete | `scalbl` | removed from POSIX in 2008; glibc keeps them (`scalb` and `scalbf`, which musl declares, done 2026-09-28; `scalbl` the same day, `c23math.rs`: glibc's x87 `e_scalbl.S`, operation for operation) |
| fenv | `fegetmode` `fesetmode` | **done 2026-09-29** (`fenv.rs`): glibc's -- the x87 control word and `MXCSR`, the flags left alone -- with `femode_t` and `FE_DFL_MODE` from `posix/include/fenv.h`, which musl's headers lack; a caller's `MXCSR` held to the bits the processor implements, where glibc's faults on one no `fegetmode` made |

`matherr` (an SVID hook glibc keeps only for old binaries) is deliberately
absent.

**Where:** `posix/src/math.rs`, `mathl.rs`, `complex.rs`. **Found by**
comparing the symbol tables while adding the `long double` functions
(design-decisions §1134).

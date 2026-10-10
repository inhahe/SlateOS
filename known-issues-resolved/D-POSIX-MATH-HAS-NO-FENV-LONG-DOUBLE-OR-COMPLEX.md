## D-POSIX-MATH-HAS-NO-FENV-LONG-DOUBLE-OR-COMPLEX — `<fenv.h>`, the `long double` functions and `<complex.h>` do not exist (lane D, 2026-09-27) — **Status: FIXED 2026-09-28 -- the 22 `long double` complex functions were the last (`posix/src/complexl.rs`; D-POSIX-LIBM-LACKS-GLIBC-EXTENSIONS). Done in steps before that: `<fenv.h>` done 2026-09-27: posix/src/fenv.rs, glibc's x86-64 fenv; `<complex.h>` done 2026-09-28 but for its `long double` functions: posix/src/complex.rs, FreeBSD's msun; the `long double` functions done 2026-09-28: posix/src/mathl.rs, §1134; and those 22 last. Filed with the unresolved entries until 2026-10-06, because the word that described the steps read as this entry's own state**

**In short:** three parts of C's maths are missing from the C library, so a C
program that uses them does not link: changing or reading the rounding mode
and the exception flags (`fesetround`, `fetestexcept` ...), the `long double`
versions of every function (`sinl`, `sqrtl` ...), and complex numbers
(`cabs`, `cexp` ...). Rust programs are not affected -- none of it is
reached from Rust's standard library.

**What each needs:**

- **`<fenv.h>`** -- `fegetround`, `fesetround`, `feclearexcept`,
  `fetestexcept`, `feraiseexcept`, `fegetenv`, `fesetenv`, `feholdexcept`,
  `feupdateenv`, `fegetexceptflag`, `fesetexceptflag`: musl's
  `src/fenv/x86_64/fenv.s`, i.e. `stmxcsr`/`ldmxcsr` for SSE and
  `fnstcw`/`fldcw`/`fnstsw`/`fnclex` for the x87 unit, as `core::arch::asm!`.
  Until it exists, `rint` and `nearbyint` always round to nearest, which is
  also what they do after any `fesetround` a program could not link.
  **Done 2026-09-27** (`posix/src/fenv.rs`): glibc 2.39's `sysdeps/x86_64/fpu`,
  both units, with `feenableexcept`/`fedisableexcept`/`fegetexcept`,
  `fesetexcept`/`fetestexceptflag` and `__flt_rounds`; `nearbyint` holds the
  flags as glibc's does. C23's `fegetmode`/`fesetmode` followed on
  2026-09-29, with the `femode_t` musl's headers lack declared by
  `posix/include/fenv.h`, the header overlay (design-decisions §1141).
- **`long double`** -- on x86-64 an 80-bit x87 value: musl's
  `src/math/x86_64/*.s` for the functions the x87 unit computes (`sqrtl`,
  `fabsl`, `rintl`, `floorl` ... `expl`, `logl`, `atan2l`), and its generic
  `ld80` C for the rest. Rust has no 80-bit type, so these are `asm!` over
  memory operands.
  **Done 2026-09-28** (`posix/src/mathl.rs`, `ld80.rs`, `ld_abi.rs`,
  design-decisions §1134): all 72 C names, each an assembly thunk in front of
  Rust, computing on the x87 unit; musl's algorithms but for `powl` and
  `exp10l`, whose general case is new (musl's `powl` was off by up to 303
  ulps); replayed against glibc 2.39 for 31,062 calls.
- **`<complex.h>`** -- musl's `src/complex/`: seventy functions, formulas over
  the real ones plus their special cases (Annex G).
  **Done 2026-09-28** (`posix/src/complex.rs`) but for the 22 `long double`
  functions, and from FreeBSD's msun rather than musl: musl's `casin`,
  `cacos`, `casinh`, `cacosh` and `clog` are the schoolbook formulas, which
  lose their digits near the branch points and near `|z| = 1`
  (design-decisions §1133). The 44 `double` and `float` functions replay
  glibc 2.39 for 44,958 calls.

**Where:** `posix/src/math.rs` (and new `fenv.rs`, `complex.rs`).
**Found by** reading the archive's symbols while replacing the maths
(`D-POSIX-MATH-WAS-HAND-WRITTEN`).

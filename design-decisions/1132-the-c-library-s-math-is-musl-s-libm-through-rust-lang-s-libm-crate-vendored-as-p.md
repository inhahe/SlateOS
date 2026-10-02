## 1132. The C library's math is musl's libm, through rust-lang's `libm` crate vendored as published, with glibc's `errno`

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous) -- within the operator's standing rule that battle-tested code is ported rather than written (`design.txt`'s "ext4 first"; §539 for cryptography). Reversible: the wrappers in `posix/src/math.rs` are the only callers.

**In short:** the C library's maths -- `sin`, `exp`, `pow`, `sqrt`, `round` and a
hundred more -- was written here, by hand, and its own documentation said it
was "accurate to roughly 10-15 digits". Some answers were far worse: `sin` of a
large number was noise, `sqrt` was not correctly rounded, `round` got numbers
just below a half wrong, and `fma` rounded twice. On SlateOS every Rust
program's `f64::sin`, `f64::round` and `f64::mul_add` calls these same
functions. They are now musl's libm -- the version of it that Rust itself
uses where a platform has no libm -- copied in unchanged, with glibc's way of
reporting errors added on top.

### The problem

`posix/src/math.rs` implemented 136 functions from scratch: Taylor series,
`fmod(x, 2*pi)` range reduction, Newton's-method `sqrt`, thresholds guessed
at (`exp` gave infinity above 709; it is finite to 709.78). Lane E found
`round` wrong (`requests/e-d-libc-round-is-wrong-just-below-a-half-and-past-2-52.md`);
reading the file for that found the rest. Nothing set `errno`. `llrint`,
`__fpclassify` (which musl's `fpclassify` macro calls), `signgam`, the float
Bessel functions and C23's `roundeven` did not exist, so C programs using them
did not link.

### The decision

- The implementations are rust-lang's `libm` 0.2.16, vendored byte for byte
  from crates.io (`posix/vendor/libm`, checksum in `posix/vendor/README.md`),
  a path dependency of `posix`, excluded from the workspace as `rustcrypto/`
  is (so the workspace's lints and `clippy --workspace` do not judge upstream
  code, and its dev-dependencies are never fetched).
- `posix/src/math.rs` is the C ABI over it: one `extern "C"` function per
  symbol, and glibc 2.39's `errno` rules -- its `math/w_*_template.c`
  wrappers, and for the functions whose double implementation sets `errno`
  itself, the implementation -- on top.
- Tested against glibc itself: 23,113 calls replayed from glibc 2.39 under
  WSL, bit for bit for the functions IEEE 754 defines exactly, within stated
  ulps for the approximations.

### Alternatives

- **Fix the hand-written functions one by one.** What would have been done
  for `round` alone. Rejected: every function had its own approximation to
  audit, and a correct `sin` needs Payne-Hanek range reduction, a correct
  `pow` needs double-double arithmetic -- that is writing libm, which is the
  thing not to do.
- **Port musl's C by hand into Rust.** The same code, done again: the
  `libm` crate *is* that port, maintained by rust-lang, used by
  compiler-builtins on every Rust target without a system libm, and tested
  against MPFR in its own CI. A second port would be ours to keep in step with
  musl forever.
- **Compile musl's C `src/math` into `libc.a` with zig.** The most literal
  port, and C is allowed for ported code. Rejected for now: the sysroot is
  Rust-only today, and this would add a second toolchain to the libc build for
  a result the crate already gives. If the crate ever lags musl on accuracy,
  this is the fallback.
- **glibc's libm.** The most accurate x86-64 libm (several functions are
  correctly rounded), and the one this library's behaviour is modelled on --
  but LGPL, and heavily tied to glibc's internals (IFUNC dispatch, the
  `math_config.h` machinery). Its `errno` rules are what is taken from it.

### What remains

`<fenv.h>` (rounding modes and exception flags), the `long double` functions,
and `<complex.h>` do not exist yet: `known-issues.md` ->
`D-POSIX-MATH-HAS-NO-FENV-LONG-DOUBLE-OR-COMPLEX`, lane D's next pieces of
this.

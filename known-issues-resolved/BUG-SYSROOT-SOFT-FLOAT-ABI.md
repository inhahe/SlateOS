### BUG-SYSROOT-SOFT-FLOAT-ABI. The sysroot `libc.a` was compiled soft-float but linked into hard-float programs — `strtod`/`atof`/`difftime`/`printf("%f")` silently returned garbage — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**What.** `toolchain/build-sysroot.ps1` builds the posix crate for
`x86_64-unknown-none`, whose target spec is:

```
"features": "-mmx,-sse,-sse2,…,+soft-float"
"position-independent-executables": true
```

but the `libc.a` it produces is linked into `x86_64-slateos` programs
(`"features": "+sse,+sse2"`, `"relocation-model": "static"`) and into C objects
built by `zig cc`, for which SSE2 is x86-64 *baseline*. Under the SysV x86-64
ABI a `double` is returned in `%xmm0` and passed in `%xmm0`–`%xmm7`; under
soft-float LLVM uses the general-purpose registers and compiler-rt helpers
instead. So every libc function with a float in its signature had a mismatched
ABI — the callee wrote `%rax`, the caller read `%xmm0`.

Affected: `strtod`, `strtof`, `atof`, `difftime`, and the whole `printf`
family's `%f`/`%e`/`%g` (varargs doubles arrive in `%xmm0`–`%xmm7` with the
count in `%al`, and the *callee's* prologue is what spills them to the register
save area `va_arg_double` reads — a soft-float build never emits that spill).

**Evidence.** `difftime` disassembled out of the old `libc.a` was:

```
subq %rsi, %rdi ; movabsq $0,%rax ; callq *(%rcx,%rax) ; popq %rcx ; retq
```

— an integer subtract, a soft-float helper call, and a return with **no write
to `%xmm0` anywhere**. `strtod` likewise contained not one `xmm` reference.

**Why nothing caught it.** An ABI mismatch has no symbol to complain about:
both sides compile and link perfectly. The float-returning functions also have
no host-side test that crosses the boundary (posix's own unit tests are
Rust-to-Rust within one target), and no ring-3 fixture called them.

**✅ RESOLVED 2026-07-30.** `build-sysroot.ps1` now compiles both `libc.a` and
`libstubs.a` with

```
-C code-model=large -C relocation-model=static -C target-feature=+sse,+sse2,-soft-float
```

`$env:RUSTFLAGS` replaces the `[target.x86_64-unknown-none]` rustflags in
`.cargo/config.toml` wholesale, so this is confined to the sysroot — the kernel
and the bare-metal `services/` binaries, which really *are* soft-float, are
untouched. `difftime` is now `subq %rsi,%rdi ; cvtsi2sd %rdi,%xmm0 ; retq`.

`relocation-model=static` was added in the same change: `x86_64-unknown-none`
is PIE-by-default, so `libc.a` was PIC and linked only because lld happened to
relax the GOT and GD→LE TLS accesses at static-link time. `x86_64-slateos` is
`relocation-model: static` with PIE off; matching it removes the reliance on
that relaxation. (`code-model=large` was already set — unknown-none defaults to
`kernel`, and our programs load high.)

**Regression guard.** New ring-3 fixture `services/ctest-libc-float/` (plain C,
built by `zig cc` — the test *must* be a caller from a different toolchain, or
it proves nothing) plus `self_test_clibc_float` in `kernel/src/proc/spawn.rs`.
It checks both directions: returns through `strtod`/`strtof`/`atof`/`difftime`,
and arguments through `snprintf("%f", …)` including a mixed integer/float
vararg list, since the ABI counts the two argument classes separately.

The guard was verified against a **negative control**, not just observed to
pass: rebuilding `libc.a` with the old `-C code-model=large` alone, relinking
the fixture and re-running the boot test produced

```
FAIL: ctest-libc-float (ring 3) — reached Zombie but exit code was Some(10),
expected 42. Codes: 10/12/13 = strtod returned the wrong value (the signature
of a soft-float sysroot: the callee wrote %rax, we read %xmm0) …
```

so the test genuinely catches the regression rather than passing for an
unrelated reason. The sysroot and fixture were restored afterwards.

**Second guard — named float arguments.** `ctest-libc-float` has a blind spot:
every double it moves is either a *return* value or a *varargs* argument.
`strtod`/`strtof`/`atof` take pointers, `difftime` takes two `time_t`, and
`snprintf`'s floats travel the variadic path (caller fills `%xmm0`-`%xmm7` and
sets `%al`, callee spills them to a register save area). **Named** float
arguments are a separate ABI rule — classified SSE, placed directly in
`%xmm0`-`%xmm7`, no `%al`, no save area — and that is the rule nearly every
real float call uses. `services/ctest-libm/` (plain C, `zig cc`) plus
`self_test_clibm` in `kernel/src/proc/spawn.rs` covers it: 48 math functions
including two- and three-`%xmm` calls (`pow`, `atan2`, `hypot`, `fma`), `f32`
arguments in the low half of an `%xmm`, and mixed SSE/INTEGER classes with
out-parameters (`frexp`, `modf`, `ldexp` — where an `int` or a pointer must
consume from `%rdi`-`%r9` and *not* from the `%xmm` sequence).

Two things would silently defeat that fixture, and both are guarded in its
`build.py`: **`-fno-builtin`** (otherwise clang recognises `sqrt`/`fabs`/
`fmax`/`floor` as builtins, emits `sqrtsd`/`andpd`/`maxsd`/`roundsd` inline,
and the sysroot is never called — the test would pass with `libc.a` absent),
and laundering every input through a `volatile` global (otherwise constant
folding evaluates the call at compile time). Verified after building:
`llvm-nm --undefined-only main.o` lists all 48 as real external references —
and, as a negative control, recompiling the same source *without*
`-fno-builtin` leaves only 38: `sqrt`, `sqrtf`, `fabs`, `fabsf`, `fmax`,
`fmaxf`, `fmin`, `fminf`, `copysign` and `lrint` disappear into inline SSE.
The flag is load-bearing, not decorative.

It doubles as the only check that `posix/src/math.rs` is numerically correct
*as compiled for the sysroot* — its 261 unit tests run on the host, which is a
different target, feature set and optimisation pipeline, so host-test success
is not evidence about the artifact in `libc.a`. Tolerances are 1e-12 relative
(~4 ulp), tight enough to also catch a broken series expansion or range
reduction; the expectations were calibrated against the host build first so a
tolerance that `math.rs` cannot meet can't masquerade as an ABI break.

**Related.** Anything else built against this sysroot before 2026-07-30 and
still cached may hold the old soft-float `libc.a`; a full sysroot + fixture +
rootfs rebuild is required to be sure.

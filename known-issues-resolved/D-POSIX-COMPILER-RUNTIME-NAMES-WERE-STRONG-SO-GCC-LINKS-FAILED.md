## D-POSIX-COMPILER-RUNTIME-NAMES-WERE-STRONG-SO-GCC-LINKS-FAILED — `libc.a` defined 40 names strongly that GCC's own runtime defines too, so GCC's `cc1` and `cc1plus` failed to link for SlateOS (lane D, 2026-10-07) — FIXED 2026-10-07
**Status:** FIXED 2026-10-07 — every one is weak on SlateOS (`weak_c_exports!` in `posix/src/lib.rs`, `weak cl_llll` in `posix/src/ld_abi.rs`), and `scripts/check-libc-shape.py` CHECK 7 holds all 115 names a compiler's runtime shares with the archive to weak

**In short:** a C compiler links files of its own into every program
beside the C library: GCC's `libgcc.a` (helpers for arithmetic the processor
has no instruction for, such as `__popcountdi2`), `libgcc_eh.a` (the unwinder
C++ exceptions need) and `libsupc++` (the C++ runtime). This library defines
some of the same names, for the links that bring none -- zig's, which make
every SlateOS program today. It defined 40 of them strongly. A static link
extracts a whole archive member for any one name it defines, so whenever a
link took a name from GCC's file and the member holding ours for some other
reason, the two definitions clashed and the link failed. Building GCC itself
for SlateOS hit it: `cc1` and `cc1plus`, the C and C++ compilers proper,
failed with "multiple definition of `__popcountdi2'".

**How it was found.** Building GCC 16.2 to run on SlateOS, with the cross
compiler `scripts/gcc-spike/cross.sh` builds (`scripts/gcc-spike/native.sh`),
2026-10-07:

    posix.9c74ae364c9e3028-cgu.102:(.ltext.__popcountdi2+0x0): multiple
    definition of `__popcountdi2'; .../libgcc.a(_popcountsi2.o):
    .../libgcc/libgcc2.c:823: first defined here
    collect2: error: ld returned 1 exit status
    make[1]: *** [../../gcc-16.2.0/gcc/c/Make-lang.in:88: cc1] Error 1

`cgu.102` is `posix/src/compiler_rt.rs`'s member. libgcc's `__popcountdi2`
came first; the member was then extracted for another of its names, and its
own `__popcountdi2` was the second definition.

**The measurement.** `check-libc-shape.py --runtime-overlap`, new with the
fix, lists the names an archive shares with a compiler's runtime files.
Against GCC 16.2.0's (`crtbegin*.o`, `crtend*.o`, `libgcc.a` -- with the
unwinder in it, GCC being built without shared libraries -- `libsupc++.a` and
`libstdc++.a`), `libc.a` shared 107 names. 67 were already weak: Rust's
`compiler_builtins` defines its share weak, and `__dso_handle` was fixed the
same morning (`D-POSIX-DSO-HANDLE-WAS-STRONG-SO-NO-GCC-LINK-COULD-SUCCEED.md`).
40 were strong:

| Where in `posix` | Names | Also defined by |
|---|---|---|
| `compiler_rt.rs` | 27 of its 35: the trapping arithmetic `__addv*`, `__subv*`, `__mulv*`, `__negv*`, `__absv*`; `__cmpti2`, `__ucmpti2`, `__negti2`, `__ffsti2`; `__parity*`, `__popcount*`; `__muldc3`, `__mulsc3`, `__divdc3`, `__divsc3` | `libgcc.a` |
| `crt.rs`, `mod cxx_abi` | the ten `__cxa_*` and `__gxx_personality_v0` stand-ins | `libsupc++.a` |
| `crt.rs`, `mod cxx_abi` | `_Unwind_Resume` | `libgcc_eh.a` |
| `complexl.rs` | `__mulxc3`, `__divxc3` | `libgcc.a` |

`compiler_rt.rs`'s other eight (`__cmpdi2`, `__ucmpdi2`, `__negdi2`,
`__paritysi2`, `__popcountsi2`, `__negsf2`, `__negdf2`,
`__compilerrt_abort_impl`) are not in x86-64's libgcc but are in LLVM's
compiler-rt, so a link with clang's runtime would clash on them the same way.

**The fix.** All 48 are weak on SlateOS: where the compiler brings its own
definition, the link takes it; where it brings none, it takes ours. Stable
Rust cannot make a function weak (`#[linkage]` is nightly-only, and the
library is type-checked on stable), so `weak_c_exports!` (`posix/src/lib.rs`)
emits, for each name, a weak one-instruction thunk that `jmp`s to the Rust
function, which no longer has a C name of its own. A `jmp` leaves every
argument register and the stack as the caller set them, so the thunk is
right for any signature. `__mulxc3` and `__divxc3` were already assembly
thunks (they take `long double`s, which Rust cannot), and `ld_c!` gained a
`weak cl_llll` form for them. The C++ stand-ins keep their own module, and
so their own member, which a program bringing a real runtime still never
extracts; the thunks are in the same module, so they land in that member.

`check-libc-shape.py` CHECK 7, which had held `__dso_handle` alone to weak,
now holds 115 names: the 107 measured, each with the file that defines it,
which a violation names, and the eight compiler-rt-only ones. Six new
mutants, all killed. `scripts/gcc-spike/cross-check.sh` runs
`--runtime-overlap` against the GCC it builds on every check and fails on a
strong name or one CHECK 7 does not list, so a new GCC or a new name in this
library is found there rather than at a link.

**What it could have cost.** Every GCC link that extracted one of these
members for another reason -- GCC's own compilers, the first time, and in
general any C++ program (the unwinder and the C++ runtime) or any program
using complex arithmetic or `__builtin_popcount`, depending on what else it
called.

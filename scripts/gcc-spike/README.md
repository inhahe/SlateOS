# gcc-spike — do GCC's compilers link against SlateOS's libc?

**Yes: all six programs, with nothing missing and nothing duplicated
(2026-10-07, the second run).** Each program's link, measured on its own:

| Program | Built as | Missing | Duplicated | Without its DWARF |
|---|---|---|---|---|
| `gcc`, the C driver | `gcc/xgcc` | 0 | 0 | 4.8 MB |
| `g++`, the C++ driver | `gcc/xg++` | 0 | 0 | 4.8 MB |
| `cpp`, the preprocessor | `gcc/cpp` | 0 | 0 | 4.8 MB |
| `cc1`, the C compiler | `gcc/cc1` | 0 | 0 | 47.7 MB |
| `cc1plus`, the C++ compiler | `gcc/cc1plus` | 0 | 0 | 50.6 MB |
| `collect2`, the link step | `gcc/collect2` | 0 | 0 | 3.7 MB |

Each is ours and not musl's (our libc's `rust_begin_unwind`, none of musl's
`__syscall_cp`) and carries the SlateOS ABI note. GMP, MPFR and MPC, built in
GCC's tree through the same wrappers, needed nothing more either.

The first run stopped earlier, in `all-lto-plugin`: link-time optimisation
builds `liblto_plugin`, a shared object the linker loads, which a
static-only link cannot make. The second turned LTO off (`--disable-lto`),
which takes `lto1` and `lto-wrapper` with it.

**`make all-gcc` still exits 2, for two reasons in the build, not the
library** (the policy's fourth category, build friction):

- `gcov-tool`'s three objects (`libgcov-util.o`, `libgcov-driver-tool.o`,
  `libgcov-merge-tool.o`) are compiled from `.c` sources by `$(CXX)`, which
  GCC's build expects to compile them as C++, as `g++` does. zig's `c++`
  compiles a `.c` file as C, so they fail with "unknown type name 'class'".
  Only `gcov-tool` needs them.
- `specs`: the build runs the target compiler to write out its specs
  (`-dumpspecs`). In this arrangement -- built on Linux, to run on SlateOS --
  that has to be a compiler that runs on Linux and makes SlateOS code, and
  there was none -- the same cross compiler the target libraries need
  (below).

Both are this spike's, which builds with zig's compilers to measure the
link. `native.sh` (below) builds GCC with the cross GCC instead, and there
`make all-gcc` exits 0.

Run `./run.sh` from WSL to reproduce. It is the "try the port before you write
a line" step from `roadmap-detailed.md`'s *Porting vs. Reimplementing* policy,
applied to the last quarter of `roadmap.md`'s "gcc, cmake, make, pkg-config via
the POSIX layer". make, pkgconf and cmake are on the image and run on every
boot. gcc was "the one unmeasured quarter", waiting on its mathematical
libraries -- GMP and MPFR, which `scripts/gdb-spike/` already builds against
our libc, and MPC.

## What is measured

GCC **16.2.0**, unmodified, with GMP 6.3.0, MPFR 4.2.2 and MPC 1.4.1 in its
tree -- `contrib/download_prerequisites`' arrangement, with the pinned
tarballs in place of its downloads, so GCC builds them for the host itself.
Cross-configured with `--build`, `--host` and `--target`, so configure runs
nothing it builds. Every host compile is zig's `cc` or `c++` against musl's
headers with `posix/include` in front, and every host link -- configure's tests
included -- is zig's `ld.lld` against `toolchain/sysroot/lib/libc.a`
(`slate_make_link_wrappers` in `scripts/lib/worktree.sh`). The generator
programs GCC's build runs on the build machine (`genattrtab`, `gengtype` ...)
are made with the host's own `gcc` and `g++`.

`make all-gcc`, and each program's link measured on its own: the drivers
`gcc` and `g++`, `cpp`, the compilers proper `cc1` and `cc1plus`, and
`collect2`. C and C++ only; no plugins; no isl; no link-time optimisation,
whose `liblto_plugin` is a shared object the linker loads, which a static-only
link cannot make (the first run stopped there).

## What this does not answer

Whether GCC can build a program on SlateOS. That needs:
- an assembler and a linker there. GNU binutils 2.47 links whole against our
  `libc.a` and is on the image (`scripts/binutils-spike/`): `/bin/as` and
  `/bin/ld.bfd`, with LLVM's `/bin/ld.lld` beside them.
- GCC's target libraries, `libgcc` and `libstdc++`, built for SlateOS.
  `all-gcc` builds none of them, because building them needs a compiler that
  runs where the build runs and makes code for SlateOS: GCC again, built as
  a cross compiler, which is the next step.
- the C library's headers and `libc.a` installed where `gcc` looks.

## A cross compiler for SlateOS: `cross.sh`

GCC 16.2 built to run on Linux and make SlateOS programs, with binutils 2.47
for the same target, and with GCC's target libraries -- libgcc and
libstdc++ -- built for SlateOS by it. The target libraries are what a
compiler running on SlateOS also needs; and the cross compiler is itself a
second compiler for SlateOS beside zig's: `x86_64-linux-musl-gcc` and `-g++`
in `$SLATE_WORK/gcc-cross/prefix/bin`.

Its sysroot is what every C program here is compiled against today: musl's
and Linux's headers as zig orders them for `x86_64-linux-musl`, our
`libc.a`, and empty `crt1.o`, `crti.o`, `crtn.o`, `libm.a` and their sisters
(the script's comment says why each is empty). Our header overlay goes in
GCC's tool include directory, which for a musl target GCC searches after
C++'s headers and immediately before musl's -- where the overlay's
`#include_next` needs them. `-static` is the default. design-decisions.md
1185 records each choice and its alternatives.

**Result (2026-10-07): it builds whole -- binutils, GCC, libgcc and
libstdc++ -- and `cross-check.sh` compiles and links two programs with it,
nothing missing and nothing duplicated:** a C one (stdio, `strdup`), 1.5 MB,
and a C++ one (an exception thrown and caught, `std::cout`, strings in a
vector, a joined `std::thread`), 10.5 MB. Each is a static executable, ours
(our libc's `rust_begin_unwind`, none of musl's `__syscall_cp`), carrying the
SlateOS ABI note. They are SlateOS programs, so they cannot run on Linux.

It needed one fix in the C library first: `libc.a` defined `__dso_handle`
strongly, and GCC's `crtbeginT.o` defines it too, so every GCC link failed
-- libstdc++'s configure, whose tests link, the first among them
(`known-issues-resolved/D-POSIX-DSO-HANDLE-WAS-STRONG-SO-NO-GCC-LINK-COULD-SUCCEED.md`).

And a second, found by GCC built to run on SlateOS (`native.sh`, below):
`libc.a` defined 40 more names strongly that GCC's runtime defines too --
compiler helpers such as `__popcountdi2`, the C++ runtime's stand-ins, the
unwinder's `_Unwind_Resume` -- and `cc1` and `cc1plus` failed on the first
(`known-issues-resolved/D-POSIX-COMPILER-RUNTIME-NAMES-WERE-STRONG-SO-GCC-LINKS-FAILED.md`).
All are weak now. `cross-check.sh` lists every name `libc.a` shares with
this GCC's runtime -- 107 -- and fails unless each is weak in `libc.a` and
listed in `check-libc-shape.py`'s CHECK 7, which holds them to weak on every
sysroot build, where no GCC is at hand.

## GCC for SlateOS, on the image: `native.sh` and `slatelink.sh`

GCC 16.2 built to run on SlateOS: configured and compiled by the cross GCC
above (a "canadian cross": built on Linux, to run on SlateOS, making SlateOS
programs), with libgcc and libstdc++ for it, then `slatelink.sh` relinks its
programs against the current `libc.a`, installs it and makes the image's
tree, `build/spike/gcc`, rooted at `/` with `--prefix=/usr`. The rootfs
recipe stages that and adds what changes with the tree rather than with
GCC: `libc.a` in `/usr/lib`, our header overlay in GCC's tool include
directory, and binutils' `as` and `ld` where GCC looks for them first
(design-decisions 1185). Built by the cross GCC rather than zig's
compilers because GCC's build compiles some `.c` files with `$(CXX)` as
C++, which zig's `c++` does not, and runs the target compiler as it goes.

**Result (2026-10-07): it builds whole and links whole.** `make all-gcc`,
libgcc and libstdc++ all exit 0 -- `gcov-tool` included, the program zig's
compilers could not build -- and each of the 15 programs in the tree is
ours (our libc's `rust_begin_unwind`, none of musl's `__syscall_cp`) and
carries the SlateOS ABI note. The tree is 168 MB with the debugging
information stripped from its programs and archives; the symbols stay.
`gcc-ar`, `gcc-nm` and `gcc-ranlib` are left out: they only run binutils'
tools with the LTO plugin, which this GCC has not got. The
first attempt failed at `cc1` and `cc1plus` on `__popcountdi2`, which
`libc.a` and libgcc both defined strongly -- the second C library fix above.

**The C++23 `std` module was empty until the second build.** The build
compiles `bits/std.cc` -- the source of `import std;`, every header at once
-- and when that fails it says "Cannot compile std module", installs empty
module sources and goes on, exit 0. It failed here: the cross compiler that
builds libstdc++ searches its *own* installed libstdc++ headers too, and the
two copies share include guards, so libstdc++'s `<fenv.h>` wrapper, reaching
for musl's `<fenv.h>` with `#include_next`, found the cross compiler's copy
of itself, already guarded and so empty, and `fenv_t` was never declared. A
program on SlateOS saying `import std;` would have got nothing. `native.sh`
now gives the target libraries' compiler `-nostdinc++`, as GCC's configure
gives an in-tree build's for the same reason, and refuses a build that still
says "Cannot compile std module"; `slatelink.sh` refuses a tree whose
`std.cc` or `std.compat.cc` is empty.

Whether it compiles a program **on SlateOS** is `services/ctest-gcc-runs`'
question: `gcc --version`, `cpp` expanding a macro, and a C program and a C++
one (an exception, `std::cout`, strings in a vector, a joined thread)
compiled, linked and run there. It runs once lane A's generic rung does
(`requests/d-a-one-rung-for-every-c-fixture.md`).

## Sources

Pinned in `scripts/lib/worktree.sh` by sha256: GCC by Gentoo, OpenEmbedded
and Homebrew; MPC by Gentoo, Void and Arch; GMP and MPFR as for the GDB
spike.

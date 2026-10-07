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

## Sources

Pinned in `scripts/lib/worktree.sh` by sha256: GCC by Gentoo, OpenEmbedded
and Homebrew; MPC by Gentoo, Void and Arch; GMP and MPFR as for the GDB
spike.

# gcc-spike — do GCC's compilers link against SlateOS's libc?

**RESULTS_PENDING**

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
`gcc` and `g++`, `cpp`, the compilers proper `cc1` and `cc1plus`, `collect2`,
`lto-wrapper` and `lto1`. C and C++ only; no plugins; no isl.

## What this does not answer

Whether GCC can build a program on SlateOS. That needs:
- an assembler and a linker there. LLVM's `ld.lld` is on the image already.
  GNU binutils is a spike of its own; its 2.47 tarball is attested by only
  one packager so far, and the pin rule wants two.
- GCC's target libraries, `libgcc` and `libstdc++`, built by this compiler
  for SlateOS. `all-gcc` builds none of them, because building them needs a
  compiler that runs here and makes code for SlateOS.
- the C library's headers and `libc.a` installed where `gcc` looks.

## Sources

Pinned in `scripts/lib/worktree.sh` by sha256: GCC by Gentoo, OpenEmbedded
and Homebrew; MPC by Gentoo, Void and Arch; GMP and MPFR as for the GDB
spike.

# binutils-spike — do GNU binutils' programs link against SlateOS's libc?

**Yes: all fourteen, with nothing missing and nothing duplicated, on the
first run (2026-10-07).** Each program's link, measured on its own:

| Program | Built as | Missing | Duplicated | Stripped of DWARF |
|---|---|---|---|---|
| `as` | `gas/as-new` | 0 | 0 | 3.7 MB |
| `ld.bfd` | `ld/ld-new` | 0 | 0 | 4.4 MB |
| `ar`, `ranlib` | `binutils/ar`, `ranlib` | 0 | 0 | 2.9 MB, 2.8 MB |
| `nm` | `binutils/nm-new` | 0 | 0 | 2.8 MB |
| `objdump`, `objcopy` | `binutils/objdump`, `objcopy` | 0 | 0 | 4.2 MB, 3.0 MB |
| `readelf` | `binutils/readelf` | 0 | 0 | 2.6 MB |
| `strip` | `binutils/strip-new` | 0 | 0 | 2.9 MB |
| `size`, `strings`, `addr2line` | `binutils/size`, `strings`, `addr2line` | 0 | 0 | 2.8 MB each |
| `c++filt` | `binutils/cxxfilt` | 0 | 0 | 2.8 MB |
| `elfedit` | `binutils/elfedit` | 0 | 0 | 1.6 MB |

42 MB for the fourteen. Each is ours and not musl's (it carries our libc's
`rust_begin_unwind` and none of musl's `__syscall_cp`) and carries the
SlateOS ABI note. That GDB's tree -- the same bfd, opcodes and libiberty --
had linked before it made this the expected answer, not a surprise.

GNU ld itself links against our `libc.a`: tried on the host with Ubuntu's ld
2.42, a `main` returning 42 linked with `ld -static -o t t.o -L<dir> -lc`
takes `_start` from the archive as its default entry, as lld does, and the
program carries the SlateOS note. Our `libc.a` is built with the large code
model, so its sections are `.ltext.*`, `.ldata.*` and so on; ld 2.42's
default script places them as orphans after their small-model siblings,
which is untidy and correct.

**On the image** (`scripts/create-ext4-rootfs.sh`): `/bin/as`, `/bin/ld.bfd`,
`/bin/nm`, `/bin/objcopy`, `/bin/size`, `/bin/addr2line`, `/bin/c++filt` and
`/bin/elfedit`; and as `/bin/gnu-ar`, `/bin/gnu-ranlib`, `/bin/gnu-strip`,
`/bin/gnu-strings`, `/bin/gnu-objdump` and `/bin/gnu-readelf`, because the
plain names are lane B's own programs
(`requests/d-b-gnu-binutils-is-on-the-image-beside-lane-bs-tools.md`).
`services/ctest-binutils-runs/` runs them there: `as` and `ld.bfd` make a
program from assembly source with the image's `libc.a`, the program runs,
and the other twelve read or write what those two made. It passes on Linux
against the host's binutils; on SlateOS it waits on lane A's generic rung.

Run `./run.sh` from WSL to reproduce; `./slatelink.sh` alone relinks the
built objects against the current `libc.a`. It is the "try the port before
you write a line" step from `roadmap-detailed.md`'s *Porting vs.
Reimplementing* policy. A compiler on SlateOS needs an assembler there: GCC
writes assembly and hands it to `as` (`scripts/gcc-spike/`). LLVM's linker,
`ld.lld`, is on the image already (`scripts/llvm-spike/`); no assembler is.

## What is measured

binutils **2.47**, unmodified. Cross-configured with `--build`, `--host` and
`--target` (`x86_64-linux-musl`, the ABI SlateOS's programs are built for),
so configure runs nothing it builds. Every host compile is zig's `cc` against
musl's headers with `posix/include` in front, and every host link --
configure's tests included -- is zig's `ld.lld` against
`toolchain/sysroot/lib/libc.a` (`slate_make_link_wrappers` in
`scripts/lib/worktree.sh`).

`make all-binutils all-gas all-ld`, and each program's link measured on its
own: `as`, GNU `ld` (`ld.bfd`), `ar`, `ranlib`, `nm`, `objdump`, `objcopy`,
`readelf`, `strip`, `size`, `strings`, `addr2line`, `c++filt` and `elfedit`.
Left out: plugins (the LTO plugin is a shared object `ld`, `ar` and `nm`
load with `dlopen`), `gprofng` (a profiler that preloads a collector library
into the program it profiles), and the optional libraries debuginfod, zstd
and msgpack. zlib is the tree's own copy.

## What this does not answer

Whether the programs work on SlateOS: that is a boot's question, once they
are on the image.

## Sources

Pinned in `scripts/lib/worktree.sh` by sha256, the `.tar.xz`, attested by
Gentoo (SHA512 and BLAKE2B) and Buildroot (SHA512).

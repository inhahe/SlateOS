## 1185. GCC on SlateOS finds our header overlay in its tool include directory, links static by default, and brings its own runtime

**Date:** 2026-10-07
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** GCC 16.2 is being ported to run on SlateOS
(`scripts/gcc-spike/`): its programs link against our C library with
nothing missing. To compile a program there it also needs to find the C
library's headers, link the result the way SlateOS programs are linked, and
bring the small libraries every GCC-built program uses. Three choices settle
that. Our additions to musl's headers (`posix/include`) go in a directory
GCC searches just before the system's headers, so they work exactly as they
do under zig today. Programs are linked static unless asked otherwise,
because there is no dynamic loader for native programs. And GCC-built
programs use GCC's own runtime libraries, built for SlateOS by GCC, rather
than the zig-built ones our other programs use.

### 1. Where the header overlay goes

`posix/include` is not a set of complete headers: each file adds to musl's
and then reaches musl's with `#include_next`. So it must be searched *after*
anything that must come first and *immediately before* musl's. Under zig,
that is the last `-I` (`slate_make_link_wrappers`). Under GCC for a musl
target the built-in order is C++'s headers, `/usr/local/include`, the tool
include directory, `/usr/include`, and GCC's own headers *last*, so that
musl's `stddef.h` and the like win (`gcc/config/linux.h`, its musl
`INCLUDE_DEFAULTS`; measured with the cross compiler's `-v`). The choice:

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. GCC's tool include directory**, `<prefix>/x86_64-linux-musl/include` (chosen) | the overlay is found after C++'s headers and immediately before `/usr/include` | exactly the order the overlay needs; a directory GCC already has for "the target system's headers" (`TOOL_INCLUDE_DIR`); unpatched GCC; relocates with the compiler, so it works with the image at `/mnt` | a GCC-specific location: another compiler on SlateOS (clang) would need its own way to find it |
| B. `/usr/local/include` | the same order | every compiler searches it | it is the place for the user's own installs; the C library's headers would mix with them |
| C. `-isystem` in the compiler's built-in specs | the overlay is searched before C++'s headers | simple | libstdc++'s `<cstdlib>`, `<cstdio>` and the rest reach the C library's headers with `#include_next`, which skips every directory searched before theirs -- so C++ code that includes `<cstdlib>` would get musl's `stdlib.h` without the overlay's additions |
| D. GCC's multiarch directory, `/usr/include/x86_64-linux-musl` | the same order as A | clang searches it too | needs `--enable-multiarch`, which also moves where GCC looks for libraries (`/usr/lib/x86_64-linux-musl`): a layout change for the whole image, for a search order A gives without one |
| E. Merge the overlay into musl's files | one header tree | any compiler | the overlay is `#include_next` wrappers by design (§1141); merging means generating patched copies of musl's headers on every change |

**Why A.** It reproduces the order every SlateOS C program is compiled with
today, with GCC unmodified, and nothing else's place is borrowed. If clang
ever runs on SlateOS, its configuration file can name the same directory.

### 2. fixincludes is off

GCC's build normally copies the system headers it thinks need fixing into
`include-fixed/`. For a musl target that directory is not on the search
path at all (the musl `INCLUDE_DEFAULTS` leave it out), so the copies would
be made and never read. The compiler is configured `--disable-fixincludes`.
The builds are still shown the target's headers (`--with-sysroot` for the
cross compiler, `--with-build-sysroot` for the one that runs on SlateOS), so
that GCC's configure checks of the C library are answered by the C library.

### 3. Static by default

There is no dynamic loader for native programs, and the C library is
`libc.a` alone. GCC for musl links dynamically unless told otherwise, so the
compiler is configured `--with-specs=%{!shared:%{!static-pie:%{!static:-static}}}`:
`-static` is added unless the command already chose. `crt1.o`, `crti.o`
and `crtn.o` are empty objects. `libc.a` carries `_start`, which GNU ld and
lld both take from the archive as the default entry (measured with ld 2.42
before it was assumed). It runs `.init_array` itself, so nothing needs
`.init`. GCC's driver names the three files in every link, so they must
exist. `libm.a`, `libpthread.a` and the other libraries musl ships empty
are empty archives here too, since all of it is in `libc.a`.

### 4. GCC's own runtime for GCC's programs

A GCC-built program links `libgcc` -- `crtbegin`/`crtend`, helpers for what
x86-64 has no instruction for, and the unwinder -- and, for C++,
`libstdc++`. zig-built programs use zig's `compiler_rt`, `libunwind` and
`libc++` instead. The two are not mixed within a program. GCC's libraries
are built for SlateOS by GCC itself, as a cross compiler on Linux
(`scripts/gcc-spike/cross.sh`), against the same headers and `libc.a`. The
alternative was GCC linking zig's runtime. It was rejected because the C++
halves are different libraries: g++ compiles against `libstdc++`'s headers,
and its objects call `libstdc++`'s symbols, which `libc++` does not define.
And GCC's driver names `-lgcc` and `-lgcc_eh` in every link. Supplying
`compiler_rt` under those names would be a stand-in posing as GCC's library,
where GCC's own builds for SlateOS unmodified.

### 5. Built by GCC, not by zig

The GCC that runs on SlateOS is compiled by the cross GCC (a "canadian
cross"), not by zig's compilers, as the link spike was. GCC's build compiles
some `.c` files with `$(CXX)` expecting C++, which zig's `c++` does not
honour; and it runs the target compiler during the build, which must run on
Linux. Both point to the cross GCC, which links against the same `libc.a`.

### 6. What GCC's runtime also defines, the C library defines weakly

GCC's `crtbegin*.o` defines `__dso_handle`, the word C++ static destructors
are registered against; its `libgcc` defines helpers such as
`__popcountdi2` and `__mulxc3`; `libgcc_eh` the unwinder's `_Unwind_Resume`;
`libsupc++` the C++ runtime's `__cxa_throw` and the rest. Neither glibc nor
musl defines any of them. Ours defines 48 of them, for zig's links, which
bring none of GCC's files, and it defined them strongly. Then any GCC link
that took a name from GCC's file and extracted our member for another reason
had two definitions and failed: every link, for `__dso_handle`, which sits
in the member every program extracts
(`known-issues-resolved/D-POSIX-DSO-HANDLE-WAS-STRONG-SO-NO-GCC-LINK-COULD-SUCCEED.md`),
and GCC's own `cc1` and `cc1plus` for `__popcountdi2`
(`known-issues-resolved/D-POSIX-COMPILER-RUNTIME-NAMES-WERE-STRONG-SO-GCC-LINKS-FAILED.md`).
They are weak now: ours fills in where GCC's is absent and yields where it
is present. Stable Rust cannot make a function weak, so each is a weak
one-instruction assembly thunk that jumps to the Rust function
(`weak_c_exports!` in `posix/src/lib.rs`).

The alternative was to stop defining them in the C library and give zig's
links what they lack some other way -- a crtbegin of their own for
`__dso_handle`, zig's `compiler_rt` for the helpers. That would change the
link line of every program here, and the helpers in our `compiler_rt.rs`
are there because links lacked them, CMake's among them. `scripts/check-libc-shape.py`
CHECK 7 holds the archive to weak definitions of all 115 names it shares,
or could share, with GCC's runtime and LLVM's compiler-rt, and
`scripts/gcc-spike/cross-check.sh` measures the list afresh against the GCC
it builds (`--runtime-overlap`).

**Where it bites:** `scripts/gcc-spike/` (cross.sh, native.sh),
`scripts/create-ext4-rootfs.sh` (staging the compiler, the headers and the
libraries). **To revisit** if a second compiler (clang) runs on SlateOS: §1
would then want a location both compilers search.

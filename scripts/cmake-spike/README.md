# cmake-spike — does upstream CMake link against SlateOS's libc?

**Yes, as of 2026-09-11.** It was twenty symbols short when first run; the
twenty are implemented and the link is now clean.

| | first run | after the twenty |
|---|---|---|
| undefined symbols | 20 | **0** |
| duplicate symbols | 0 | **0** |
| link exit | 1 | **0** |
| binary | none | 22,519,240-byte static `ET_EXEC` |

Closing the first twenty revealed **no second layer** — worth stating, because a
linker stops reporting a symbol once it resolves, so a gap behind a gap is
invisible until the one in front of it closes. Here there was nothing behind.

Run `./run.sh` from WSL to reproduce. It is the "try the port before you write a
line" step from `roadmap-detailed.md`'s *Porting vs. Reimplementing* policy,
applied to the `cmake` quarter of the roadmap's
`gcc, cmake, make, pkg-config (via POSIX layer)` item — the last quarter of it
nobody had measured. It is deliberately shaped like `scripts/make-spike/run.sh`;
the two answer the same question.

## What was measured

CMake **4.4.3**, unmodified, cross-configured with the host's cmake 3.28.3 and
compiled with `zig c++` against zig's musl headers, then linked `-nostdlib`
against `toolchain/sysroot/lib/libc.a` plus zig's `libc++`, `libc++abi`,
`libunwind` and `libcompiler_rt`.

Configure and the full build both succeed on a cross toolchain with no source
changes — 19 static archives, on the first attempt. The link did not, and the
twenty names it was missing were the finding:

| | symbols |
|---|---|
| **locale variants (14)** | `iswalpha_l` `iswblank_l` `iswcntrl_l` `iswdigit_l` `iswlower_l` `iswprint_l` `iswpunct_l` `iswspace_l` `iswupper_l` `iswxdigit_l` `towlower_l` `towupper_l` `strftime_l` `strtof_l` |
| **sockets (3)** | `in6addr_any` `recvmmsg` `sendmmsg` |
| **threads (2)** | `pthread_getschedparam` `pthread_setschedparam` |
| **files (1)** | `lutimes` |

Nothing about C++ is missing: zero of the twenty come from `libc++` or the
C++ ABI, which is the result `design-decisions.md` §73 predicted at link stage
and this is the first program of real size to test it.

**Fourteen of the twenty are one job.** The `_l` family is POSIX's
locale-parameterised spelling of functions we already have. musl implements
every one of them as a wrapper that ignores the locale argument, because musl
supports only the C locale — which is also the only locale we have. They are
fourteen one-line functions, not fourteen features.

**`grep` will tell you we already have two of them, and it is wrong.**
`lutimes` appears in `posix/src/file.rs` and `recvmmsg`/`sendmmsg` in
`posix/src/linux_net.rs` — all three inside doc comments describing something
else. A name search says "present"; a link says "absent", and the link is the
one that compiles.

## What this does not answer

Whether cmake **runs**. It forks and waits, walks directory trees, stats and
globs, opens pipes to compilers, and its own test suite compiles and executes
programs. A clean link tells you nothing is *missing*; it tells you nothing
about behaviour. Treat a future `MISSING_COUNT=0` as permission to build a
rootfs rung, not as a port.

Nor does it say anything about `gcc`, the remaining unmeasured quarter of that
roadmap item.

## The size, which is the one number that nearly stopped this being a port

Unstripped the binary is **274,724,488 bytes** — mostly `debug_info`, against a
384 MB image. `--strip-debug` brings it to **22,519,240** and `--strip-all` to
15,337,672. `run.sh` stages the `--strip-debug` form, matching how CPython is
staged (design-decisions.md §344): the 7 MB difference is the symbol table, and
that is what turns a fault address on the serial console into a function name.

An artifact that cannot be staged is not a port, so this number is reported by
`run.sh` on every run rather than being discovered once.

## Two findings from getting configure to run at all

Both produced **the same error message for opposite reasons**, which is why
they are written down rather than merely fixed.

**1. The host's headers leaked into the cross build.** CMake's bundled
libarchive ran `find_path(iconv.h)`, found the host's `/usr/include`, and put
`-I/usr/include` on the cross compile line. clang then read glibc's `stdlib.h`
instead of zig's musl one and failed on `bits/libc-header-start.h`, a
glibc-internal header. Configure reported:

```
CMake Error at Utilities/cmlibarchive/CMakeLists.txt:1206 (message):
  iconv is required, but was not found.
```

which reads like a missing libc feature and is nothing of the kind. The fix is
the `CMAKE_FIND_ROOT_PATH_MODE_*` block.

**2. Pointing the find-root at an empty directory produced the identical
sentence.** With nowhere legitimate to look, `find_path` failed, the iconv probe
was **skipped rather than run**, and configure printed the same line again. The
only way to tell the two apart is that the `Performing Test HAVE_ICONV` lines
*disappear* from the log rather than changing to a failure — an absence, not a
message. A cross build needs somewhere legitimate to look, not only somewhere
illegitimate to be kept away from.

So the find-root is a real sysroot: zig's musl headers plus our `libc.a`. zig
splits musl's headers into an architecture-specific directory and a generic
one, searched in that order, and `run.sh` reproduces that precedence by copy
order plus `cp -n`. Flattening them the other way round would substitute
`generic-musl`'s definitions of exactly the types whose size is
architecture-dependent, which is a mistake that compiles.

`run.sh` refuses outright if the assembled sysroot comes out without `iconv.h`,
rather than proceeding to a run in which every probe fails for a reason that has
nothing to do with our libc.

## Why both counts are printed even when they are zero

`MISSING_COUNT` and `DUPLICATE_COUNT` measure opposite failures — nothing is
absent, versus something is present twice — and a libc can fail either way. The
make spike's first run printed `SLATE_LINK_EXIT=1` beside `MISSING_COUNT=0` and
was briefly read as a fluke, because the only number on screen was the one
saying everything was fine: all eleven errors were *duplicate* definitions,
which the missing-symbol grep does not match. A spike whose headline metric can
read zero on a failed link is a spike that reports success when the answer is
no.

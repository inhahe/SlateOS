# coreutils-spike — does upstream GNU coreutils link against SlateOS's libc?

**No. 106 of 107 links fail, and 106 of them fail for one reason: nineteen
functions our libc does not have.** The granularity problem that broke the make
port is *almost* gone — it accounts for exactly one duplicate symbol.

Run `./run.sh` from WSL to reproduce, or `SLATE_RELINK_ONLY=1 ./run.sh` to redo
just the relink against an existing build. It is the "try the port before you
write a line" step from `roadmap-detailed.md`'s *Porting vs. Reimplementing*
policy, applied to the `coreutils` third of the roadmap's `Enough of POSIX libc
for gcc/coreutils/bash/CPython` item. It is deliberately shaped like
`scripts/make-spike/run.sh`.

## What was measured

GNU coreutils **9.5**, unmodified, `./configure --host=x86_64-linux-musl
--disable-shared --disable-nls`, compiled with `zig cc` against zig's musl
headers, then each binary relinked `-nostdlib` against
`toolchain/sysroot/lib/libc.a` (commit `a1b26843b`, 12,564,980 bytes, shape
check green).

configure and the build both succeeded on the first attempt, producing 286
gnulib objects and 107 link lines.

| | count |
|---|---|
| link lines harvested | 107 |
| **linked OK** | **1** (`make-prime-list`, a build-time host tool) |
| **failed** | **106** |
| distinct undefined symbols | **19** |
| distinct duplicate symbols | **1** |
| links that pulled in zig's musl | **0** |

Note the shape of that table against the make spike's. make failed with
`MISSING_COUNT=0` and 11 duplicates — a pure granularity problem. coreutils is
the reverse. Reading either count alone gives the wrong diagnosis, which is why
`run.sh` prints both unconditionally.

## §340's prediction was right, and §340's fix works

`design-decisions.md` §340 split seventeen `libc.a` archive members so that each
held one function, because gnulib supplies its own replacements for exactly
those names. Its text predicted this case: *"make missed them only because its
./configure did not compile in those particular gnulib modules; **coreutils and
tar** would have hit them."*

That had never been tested. It has now. coreutils vendored **six** of the
seventeen:

    lib/asprintf.o                 lib/vasprintf.o
    lib/libcoreutils_a-rawmemchr.o lib/libcoreutils_a-getopt.o
    lib/fnmatch.o                  lib/libcoreutils_a-error.o

`getopt`, `fnmatch` and `error` are three of the four families that broke the
make link in the first place. **None of the six collided.** The prediction was
correct — coreutils does compile in modules make did not — and the fix holds
against the program it was written for. That is what this spike existed to find
out, and it is the one unambiguously good news here.

## The one duplicate that is ours: `wmempcpy`

Five binaries (`ls`, `dir`, `vdir`, `du`, `dircolors`) fail on a single
duplicate. They need `mbrtowc` from our libc; that extracts the member holding
`posix::wchar`, which defines **78 external symbols**, one of which is
`wmempcpy` — and gnulib supplies its own `wmempcpy` because musl lacks it. The
collision is unavoidable from the caller's side, exactly as §339 describes.

`-C codegen-units=4096` did not prevent this because **`codegen-units` is a
ceiling, not a splitter**: rustc partitions at *module* granularity and will not
divide a single module, so `wchar.rs` (156 KB, one module, no `gnu_*` blocks at
all) is one codegen unit however high the ceiling goes. §340's actual mechanism
was not the flag — it was wrapping each of seventeen functions in its own
`mod gnu_<name> { … }`, which is why only those seventeen got their own members.

So the remaining fix is one more `mod gnu_wmempcpy` block, not a rebuild
strategy.

### Why `check-libc-shape.py` passed on this archive

The guard's CHECK 2 fires only when a member defines a `REPLACEABLE` name
*alongside* an `UNAVOIDABLE` one. `posix::wchar`'s member defines `wmempcpy`,
which is in neither set. The two sets are drawn as if they were disjoint
categories — replaceable things versus things every program needs — and the
wide-character family is simply absent from both.

## The nineteen missing symbols

These block 106 of the 106 failures and are the actual work.

| group | symbols |
|---|---|
| stdio internals gnulib reaches for | `__fpending`, `__freadahead`, `__freadptr`, `__freadptrinc`, `__fseterr` |
| `_unlocked` stdio variants | `clearerr_unlocked`, `feof_unlocked`, `ferror_unlocked`, `fflush_unlocked`, `fputc_unlocked`, `fputs_unlocked`, `fread_unlocked`, `fwrite_unlocked` |
| variadic exec wrappers | `execl`, `execlp` |
| misc | `qsort_r`, `strtod_l`, `strtold_l`, `timespec_get` |

Two details worth keeping. We already define `putc_unlocked` but not
`fputc_unlocked`, so the `_unlocked` family is half-present, which is how it
escaped notice. And all nineteen are *declared* by zig's musl headers, so
`./configure` concluded they existed and compiled calls to them; the gap appears
only at link time against our archive.

## A defect in this script, found and fixed — worth reading before trusting a spike

The first run reported **12** duplicate symbols, not 1. Eleven of them were an
artifact of this script.

`sort` is the only utility whose link line `configure` gave `-lpthread`. Because
`zig cc --target=x86_64-linux-musl` resolves `-lpthread` against its **own
bundled musl**, and musl folds pthread into `libc.a`, that single flag put a
complete second libc on a `-nostdlib` link line. musl's stdio members were then
extracted to satisfy the `X_unlocked` names our libc lacks, and each dragged its
`X` in behind it — producing `fflush`, `fread`, `fwrite`, `feof`, `ferror`,
`clearerr`, `fputs`, `__fpurge`, `putc_unlocked`, `__progname`,
`__progname_full` as duplicates against ours.

Every one of those was a fact about musl's packaging, not about our libc.
`run.sh` now strips `-lpthread`/`-lrt`/`-ldl`/`-lm`/`-lcrypt`/`-lc` before
appending our archives, and asserts `LINKS_THAT_PULLED_ZIG_MUSL=0` afterwards.

The assertion matters more than the fix. A foreign libc on the link line fails
*silently* in the direction that flatters us: it quietly satisfies symbols we do
not have, so `MISSING_COUNT` becomes a lower bound of unknown size. These eleven
only became visible because a few musl members happened to *also* collide. Had
`sort` needed no colliding symbol, musl would have supplied the gaps and the
spike would have reported a cleaner result than the truth.

Checked: the make and pkgconf spikes are not affected. Both construct their link
command from an object list with no `-l` flags at all, so zig's musl was never
reachable. Their green results stand.

## Status

Nothing staged on the image. Unlike pkgconf and make there is no binary to stage
— the one that linked is coreutils' own build-time helper, not a utility.

A clean link would not be a port, either. The same caveat as pkgconf and make
applies and bites harder here: `ls` is a VFS exerciser, `dd` is an I/O
exerciser, and `stat` reads out the exact struct we are least sure about. Treat
`MISSING_COUNT=0` as permission to build a rootfs rung, not as a port.

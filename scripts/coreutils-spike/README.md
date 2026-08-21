# coreutils-spike — does upstream GNU coreutils link against SlateOS's libc?

**No. 106 of 107 links fail.** But the interesting part is *which* failures
happened, because this run was built to test one specific prediction and that
prediction came out **confirmed** — the failures are two problems it never
covered.

Run `./run.sh` from WSL to reproduce. It is the "try the port before you write a
line" step from `roadmap-detailed.md`'s *Porting vs. Reimplementing* policy,
applied to the `coreutils` third of the roadmap's `Enough of POSIX libc for
gcc/coreutils/bash/CPython` item. It is deliberately shaped like
`scripts/make-spike/run.sh`.

## What was measured

GNU coreutils **9.5**, unmodified, `./configure --host=x86_64-linux-musl
--disable-shared --disable-nls`, compiled with `zig cc` against zig's musl
headers, then each of its binaries relinked `-nostdlib` against
`toolchain/sysroot/lib/libc.a` (commit `a1b26843b`, 12,564,980 bytes, shape
check green).

configure and the build both succeeded on the first attempt, producing 286
gnulib objects and 107 link lines. Then:

| | count |
|---|---|
| link lines harvested | 107 |
| **linked OK** | **1** (`make-prime-list`, a build-time host tool) |
| **failed** | **106** |
| failed on undefined symbols only | **100** |
| failed on duplicate symbols only | **6** |
| failed on both | 0 |
| distinct undefined symbols | **19** |
| distinct duplicate symbols | **12** |

Note the shape of that table against the make spike's. make failed with
`MISSING_COUNT=0` and 11 duplicates — a pure granularity problem. coreutils is
the reverse: overwhelmingly a *missing functions* problem, with a granularity
problem behind it. Reading either count alone would have given the wrong
diagnosis, which is why `run.sh` prints both unconditionally.

## §340's prediction was right, and §340's fix works

`design-decisions.md` §340 split seventeen `libc.a` archive members so that each
held one function, because gnulib supplies its own replacements for exactly
those names. Its text predicted the case: *"make missed them only because its
./configure did not compile in those particular gnulib modules; **coreutils and
tar** would have hit them."*

That had never been tested against coreutils. It has now. coreutils vendored
**six** of the seventeen:

    lib/asprintf.o   lib/vasprintf.o   lib/libcoreutils_a-rawmemchr.o
    lib/getopt.o     lib/fnmatch.o     lib/libcoreutils_a-error.o

`getopt`, `fnmatch` and `error` are three of the four families that broke the
make link in the first place. **None of the six appears in the duplicate list.**
So the prediction was correct — coreutils does compile in those modules where
make did not — and the fix holds against the program it was written for. That is
the one thing this spike existed to find out.

## What it also found: §340 fixed the symptoms, not the mechanism

All 12 duplicates come from just **three** archive members, none of which §340
touched:

| member | module | external symbols | duplicates it caused |
|---|---|---|---|
| `…-cgu.005.rcgu.o` | `posix::wchar` | **79** | `wmempcpy` |
| `…-cgu.013.rcgu.o` | `posix::stdio` | **76** | `__fpurge`, `clearerr`, `feof`, `ferror`, `fflush`, `fputs`, `fread`, `fwrite`, `putc_unlocked` |
| `…-cgu.031.rcgu.o` | startup/crt | **46** | `__progname`, `__progname_full` |

A libc member holding 79 externally visible symbols is the defect §339
described, still present. The reason `-C codegen-units=4096` did not prevent it
is that **`codegen-units` is a ceiling, not a splitter**: rustc partitions at
*module* granularity and will not divide a single module, so `wchar.rs` (156 KB,
one module, no `gnu_*` blocks at all) is one codegen unit no matter how high the
ceiling goes. §340's actual mechanism was not the flag — it was wrapping each of
seventeen functions in its own `mod gnu_<name> { … }`, which is why only those
seventeen got their own members.

`ls` is the clearest example: it needs `mbrtowc` from our libc, which extracts
the 79-symbol `wchar` member, which drags in `wmempcpy`, which gnulib also
defines. Unavoidable from the caller's side, exactly as §339 says.

### `scripts/check-libc-shape.py` passed on this archive, and was right to

The guard has two checks. CHECK 1 asserts four curated families own their member
outright — `getopt`, `glob`, `fnmatch`, `error`. All four still do. CHECK 2 is
the generalising one: no member may define a `REPLACEABLE` name alongside an
`UNAVOIDABLE` one.

`posix::stdio`'s member defines `fopen`, `fclose`, `fread`, `fwrite`, `fflush` —
five `UNAVOIDABLE` names. It defines nothing in `REPLACEABLE`, because
`REPLACEABLE` lists no stdio names at all. So the intersection is empty and the
check passes.

That is not a coding error; it is the set being drawn from the wrong premise.
`REPLACEABLE` and `UNAVOIDABLE` are written as if they were disjoint categories
— replaceable things versus things every program needs. On a musl target they
overlap heavily: gnulib replaces `fflush`, `fclose`, `fseek`, `fread`, `fwrite`,
`feof`, `ferror` and friends precisely *because* they are unavoidable and musl's
differ from glibc's. Any member holding the stdio family is therefore both at
once, and a check requiring one of each can never see it.

## The 19 missing symbols

These are genuine libc gaps, independent of the packing problem, and they are
what actually blocks 100 of the 106 binaries.

| group | symbols |
|---|---|
| stdio internals gnulib reaches for | `__fpending`, `__freadahead`, `__freadptr`, `__freadptrinc`, `__fseterr` |
| `_unlocked` stdio variants | `clearerr_unlocked`, `feof_unlocked`, `ferror_unlocked`, `fflush_unlocked`, `fputc_unlocked`, `fputs_unlocked`, `fread_unlocked`, `fwrite_unlocked` |
| variadic exec wrappers | `execl`, `execlp` |
| misc | `qsort_r`, `strtod_l`, `strtold_l`, `timespec_get` |

Two details worth keeping. First, we already define `putc_unlocked` (it is in
the duplicate list) but not `fputc_unlocked` — so the `_unlocked` family is
half-present, which is how it escaped notice. Second, these were *declared* by
zig's musl headers, so `./configure` concluded they existed and compiled code
calling them; the gap only appears at link time against our archive. That is the
same header/library split the spike itself relies on, working against us.

## Status

Not staged on the image. Unlike pkgconf and make there is no binary to stage —
the one that linked is coreutils' own build-time helper, not a utility.

`MISSING_COUNT=0` is not the bar for calling this done, and neither is a clean
link: the same caveat as pkgconf and make applies and bites harder here, because
`ls` is a VFS exerciser, `dd` is an I/O exerciser, and `stat` reads out the exact
struct we are least sure about. A clean link would be permission to build a
rootfs rung, not a port.

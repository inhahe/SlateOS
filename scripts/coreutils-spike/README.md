# coreutils-spike — does upstream GNU coreutils link against SlateOS's libc?

**Yes, now. All 107 links succeed, with zero missing symbols and zero
duplicates.** It did not start that way: the first run failed 106 of 107, on
nineteen functions our libc did not have and one duplicate it should not have
exported. Both are fixed; this file records what was measured and what the
measurement taught, because the *reasons* are more durable than the counts.

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
`toolchain/sysroot/lib/libc.a`.

configure and the build both succeeded on the first attempt, producing 286
gnulib objects and 107 link lines.

| | first run (`a1b26843b`) | after the fixes (`693f35f09`) |
|---|---|---|
| link lines harvested | 107 | 107 |
| **linked OK** | **1** (`make-prime-list`, a build-time host tool) | **107** |
| **failed** | **106** | **0** |
| distinct undefined symbols | **19** | **0** |
| distinct duplicate symbols | **1** | **0** |
| links that pulled in zig's musl | 0 | 0 |
| `libc.a` size | 12,564,980 | 12,603,348 |

Note the shape of the first column against the make spike's. make failed with
`MISSING_COUNT=0` and 11 duplicates — a pure granularity problem. coreutils was
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
make link in the first place. **None of the six collided**, on either run. The
prediction was correct — coreutils does compile in modules make did not — and
the fix holds against the program it was written for.

## The one duplicate that was ours: `wmempcpy`

Five binaries (`ls`, `dir`, `vdir`, `du`, `dircolors`) failed on a single
duplicate. They need `mbrtowc` from our libc; that extracted the member holding
`posix::wchar`, which defined **78 external symbols**, one of which was
`wmempcpy` — and gnulib supplies its own `wmempcpy` because musl lacks it. The
collision was unavoidable from the caller's side, exactly as §339 describes.

`-C codegen-units=4096` did not prevent this because **`codegen-units` is a
ceiling, not a splitter**: rustc partitions at *module* granularity and will not
divide a single module, so `wchar.rs` (156 KB, one module, no `gnu_*` blocks at
all) is one codegen unit however high the ceiling goes. §340's actual mechanism
was not the flag — it was wrapping each of seventeen functions in its own
`mod gnu_<name> { … }`, which is why only those seventeen got their own members.

The fix was therefore one more `mod gnu_wmempcpy` block, not a rebuild strategy.

### Why `check-libc-shape.py` passed on the broken archive

The guard's CHECK 2 fired only when a member defined a `REPLACEABLE` name
*alongside* an `UNAVOIDABLE` one. `posix::wchar`'s member defined `wmempcpy`,
which was in neither set. The two sets were drawn as if they were disjoint
categories — replaceable things versus things every program needs — and the
wide-character family was simply absent from both.

Adding `wmempcpy` to `REPLACEABLE` would **not** have fixed this. The member's
other 77 symbols — `mbrtowc`, `wcslen`, … — are in neither set either, so the
`repl and unav` condition still would not have held. The defect was structural:
`UNAVOIDABLE` means "referenced by a hello-world", which is a weak proxy for
"referenced by the program under port", and widening it has no honest stopping
point short of *every name in libc*.

**CHECK 3** (§348) states the property without the proxy: a replaceable name
must own its member outright, or share it only with other replaceable names. On
its first run against the real archive it found two more latent hazards of the
same shape that nothing had yet tripped — `mkstemp`/`mkostemp`/`mkstemps`/
`mkostemps`/`mkdtemp`/`getsubopt` riding with `atoi` and `bsearch`, and
`timegm`/`strptime` riding with `clock_gettime` and `ctime`. gnulib vendors all
eight. Each now has its own member.

## The nineteen missing symbols

These blocked 106 of the 106 failures and were the actual work. All are now
implemented, with tests.

| group | symbols | where |
|---|---|---|
| stdio internals gnulib reaches for | `__fpending`, `__freadahead`, `__freadptr`, `__freadptrinc`, `__fseterr` | `posix/src/stdio.rs` |
| `_unlocked` stdio variants | `clearerr_unlocked`, `feof_unlocked`, `ferror_unlocked`, `fflush_unlocked`, `fputc_unlocked`, `fputs_unlocked`, `fread_unlocked`, `fwrite_unlocked` | `posix/src/stdio.rs` |
| variadic exec wrappers | `execl`, `execlp` (and `execle`, for family completeness) | `posix/src/spawn.rs` |
| misc | `qsort_r`, `strtod_l`, `strtold_l`, `timespec_get` | `posix/src/stdlib.rs`, `posix/src/time.rs` |

Two details worth keeping. We already defined `putc_unlocked` but not
`fputc_unlocked`, so the `_unlocked` family was half-present, which is how it
escaped notice. And all nineteen are *declared* by zig's musl headers, so
`./configure` concluded they existed and compiled calls to them; the gap
appeared only at link time against our archive.

Implementing `qsort_r` turned up a separate, unrelated defect it is worth
recording here because the spike is what surfaced it: our `qsort` was an
insertion sort — O(n²) — which `mmap`'d a scratch buffer for elements over 256
bytes and, **if that `mmap` failed, returned leaving the array unsorted**, which
a caller cannot detect through a `void` return. `ls` sorts a directory and
`sort` sorts a file, so the quadratic term was never theoretical: 100 000
entries is ~10¹⁰ comparisons on the old code against ~1.7×10⁶ on the introsort
that replaced it, which allocates nothing.

## A defect in this script, found and fixed — worth reading before trusting a spike

The first run reported **12** duplicate symbols, not 1. Eleven of them were an
artifact of this script.

`sort` is the only utility whose link line `configure` gave `-lpthread`. Because
`zig cc --target=x86_64-linux-musl` resolves `-lpthread` against its **own
bundled musl**, and musl folds pthread into `libc.a`, that single flag put a
complete second libc on a `-nostdlib` link line. musl's stdio members were then
extracted to satisfy the `X_unlocked` names our libc lacked, and each dragged its
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

Nothing staged on the image. **A clean link is not a port**, and the same caveat
as pkgconf and make applies, biting harder here: `ls` is a VFS exerciser, `dd` is
an I/O exerciser, and `stat` reads out the exact struct we are least sure about.
`MISSING_COUNT=0` means every symbol coreutils names now *exists* and is
*callable*; it says nothing about whether the syscalls beneath them return the
right answers on our kernel. Treat this as permission to build a rootfs rung —
stage a handful of these binaries and run them on the image under ring 3 — not
as a completed port. That rung is where the behavioural gaps will show up, and
it is the next step for this item.

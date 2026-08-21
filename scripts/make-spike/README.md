# make-spike — does upstream GNU make link against SlateOS's libc?

**Yes, after two fixes, and the second one matters far more than make does.**

Run `./run.sh` from WSL to reproduce. It is the "try the port before you write
a line" step from `roadmap-detailed.md`'s *Porting vs. Reimplementing* policy,
applied to the `make` third of the roadmap's
`gcc, cmake, make, pkg-config (via POSIX layer)` item. It is deliberately shaped
like `scripts/pkgconf-spike/run.sh`; the two answer the same question.

## What was measured

GNU make **4.4.1**, unmodified, `./configure --host=x86_64-linux-musl
--disable-shared --without-guile`, compiled with `zig cc` against zig's musl
headers, then linked `-nostdlib` against `toolchain/sysroot/lib/libc.a`.

configure and the build both succeeded on the first attempt, producing 30
objects in `src/` plus `lib/libgnu.a`. The link did not.

| | first attempt | after the two fixes |
|---|---|---|
| undefined symbols | 0 | 0 |
| **duplicate** symbols | **11** | **0** |
| link exit | 1 | **0** |
| binary | none | 3,114,912-byte static `ET_EXEC` |

Both fixes are applied and the link now completes: `LINK_EXIT=0`,
`UNDEFINED=0`, `DUPLICATES=0`. The binary is staged at
`build/spike/make-slateos.elf` and put on the image as `/bin/make` by
`scripts/create-ext4-rootfs.sh`, under the same mtime staleness gate as bash and
pkgconf — absent is a warning, present-but-older-than `libc.a` is fatal. It is
`ET_EXEC`, x86-64, entry `0x10488c0`, five `LOAD` segments at `0x1000`
alignment. That last number is the same unverified detail flagged for pkgconf:
our pages are 16 KiB and these segments are aligned to 4 KiB.

Zero undefined symbols on the very first attempt is the headline the pkgconf
spike also produced: our libc already covers everything make asks of a libc.
The failure was the opposite problem.

That combination — a failed link reporting `MISSING_COUNT=0` — is also why
`run.sh` now prints `DUPLICATE_COUNT` beside it, unconditionally, even when both
are zero. The first run of this spike showed `SLATE_LINK_EXIT=1` next to a
`MISSING_COUNT` of 0 and nothing else, because the grep that produces that
number matches only `undefined symbol:`. A spike whose one headline number can
read zero on a failed link reports success when the answer is no. The two counts
measure opposite failures — nothing is absent, versus something is present twice
— and a libc can fail either way. (The same blind spot has been fixed in
`scripts/pkgconf-spike/run.sh`, which never tripped it only because pkgconf does
not vendor gnulib.)

## Finding 1 — one genuinely missing symbol: `bsd_signal`

Once the duplicates were out of the way, exactly one name was unresolved:

```
ld.lld: error: undefined symbol: bsd_signal
>>> referenced by main.c:1270 (src/main.c:1270)
```

`bsd_signal` is `signal()` with the BSD half of that function's famous
ambiguity nailed down: the handler stays installed across deliveries, the signal
is blocked inside its own handler, and interrupted syscalls restart. glibc and
musl both export it, so configure finds it and make uses it for `SIGHUP`,
`SIGINT`, `SIGQUIT` and `SIGTERM` rather than trust a bare `signal()`.

Fixed in `posix/src/signal.rs`, together with its System V counterpart
`sysv_signal` — the two exist precisely to be *distinguishable*, so shipping
one without the other would invite the next port to reach for `signal()` and get
whichever semantics we happened to pick.

## Finding 2 — `libc.a`'s archive granularity was not libc-like

The other ten were duplicates, in four families:

```
fnmatch
glob globfree
getopt getopt_long getopt_long_only optarg opterr optind optopt
error
```

Every one of those is a name that **gnulib supplies a replacement for**, which
means every GNU package that vendors gnulib — coreutils, grep, sed, tar,
findutils, diffutils, gawk, gcc, binutils, and make — defines it itself. A
real libc also defines all four, and this is not normally a problem: in glibc
each function is its own object file inside the archive, so if the program has
already defined `getopt`, the linker simply never extracts libc's copy.

Ours could not do that. `libc.a` is built by `cargo build --release`, whose
default `codegen-units = 16` packs the whole crate into 16 objects, grouping
unrelated modules together. The result:

| symbol | shared its object file with |
|---|---|
| `fnmatch` | `stdout`, `fopen`, `fwrite`, `fileno`, `isalpha`, `tolower` |
| `glob` | `printf`, `snprintf`, `vfprintf`, `uname`, `err`, `warn` |
| `getopt` | `sem_wait`, `sched_getaffinity`, `__fprintf_chk` |
| `error` | `getenv`, `environ`, `setenv`, `regcomp`, `statfs` |

So the collision was not bad luck — it was **unavoidable**. Each of the four
names shared a member with something every C program on earth needs, so the
member was always going to be extracted, and its `getopt` was always going to
collide with the program's own. No link order, no object subsetting and no
`--start-group` can avoid that.

The fix is to give the archive the granularity a libc archive is expected to
have. Raising `codegen-units` lets rustc's partitioner stop merging and emit
roughly one object per *module*, which is exactly glibc's one-object-per-`.c`
layout. Rebuilt that way:

```
cgu.034 -> fnmatch
cgu.038 -> glob globfree
cgu.050 -> getopt getopt_long getopt_long_only optarg opterr optind optopt
cgu.065 -> error error_at_line error_message_count error_one_per_line
           error_print_progname verror verror_at_line
```

— one clean module each, nothing else riding along (the archive goes from 419
members to 577). Relinking make against that archive dropped the duplicate count
from 11 to 0 and left `bsd_signal` as the sole remaining error, which is how
finding 1 was found at all.

It also made every binary on the system smaller, which was not the goal.
Extraction is all-or-nothing per member, so a coarse member drags in whatever
else it contains. Relinking **pkgconf** — which had linked cleanly all along and
needed no fix — against the finer archive took it from 2,926,720 to 2,551,080
bytes, 12.8% off a program whose source did not change. The dead weight was
`glob`, `regcomp` and `statfs` riding in behind `printf` and `getenv`.

**This was never a make problem.** It would have blocked every gnulib-using C
port, and it is the reason this spike was worth running even though make itself
is small. See `design-decisions.md` §339.

## What this does NOT establish

A link is a complete, mechanical enumeration of a program's demands on its libc,
and nothing more. `MISSING_COUNT=0` means nothing is *absent*; it does not mean
anything *works*.

make leans on the operating system far harder than pkgconf does — it forks,
execs, waits, opens pipes, installs `SIGCHLD` handlers, and on a `-j` build runs
a jobserver over either a pipe or a POSIX named semaphore. `run.sh` records what
configure decided about all of that
(`MAKE_JOBSERVER_AND_WAIT_DECISIONS_FROM_CONFIG_H`) precisely because they are
the likeliest reasons a make that links will not run. As of 2026-08-20:

```
#define HAVE_FORK 1              #define HAVE_WAIT3 1
#define HAVE_VFORK 1             #define HAVE_WAITPID 1
#define HAVE_MKFIFO 1            #define MAKE_JOBSERVER 1
#define HAVE_POSIX_SPAWN 1
#define HAVE_POSIX_SPAWNATTR_SETSIGMASK 1
/* #undef HAVE_SYS_LOADAVG_H */  (HAVE_NAMED_SEMAPHORES: absent)
```

Read that as a to-do list, because it is one. Two lines matter most:

- **`HAVE_POSIX_SPAWN 1`** — make will launch children through `posix_spawn`,
  *not* `fork`+`exec`. Whatever is true of our `fork` is therefore not the
  question; `posix_spawn` with `posix_spawnattr_setsigmask` is.
- **`MAKE_JOBSERVER 1` with `HAVE_MKFIFO 1` and no `HAVE_NAMED_SEMAPHORES`** —
  a `-j` build will coordinate over a **FIFO**, so `mkfifo` and blocking
  reads/writes on it are on the critical path. The named-semaphore route, which
  would have been the other possibility, is not taken.

Every one of those symbols is present in `libc.a` — checked with `nm` rather
than by reading the source, since the archive is what the program links:
`mkfifo`, `mkfifoat`, `posix_spawn`, `posix_spawnp`, `wait3`, `waitpid`,
`fork`, `vfork`, `pipe`, `pipe2`, `sem_open`, `sem_wait`, all defined. That is
still only presence, not behaviour.

Treat a green run as permission to build a rootfs rung, not as a port. The same
mistake has already been made twice in this tree: pkgconf was called "proven" for
two days while its only binary sat in `/tmp`, and it is still `[-]` rather than
`[x]` because *shipped is not run*.

### The recording itself was broken until 2026-08-20

Worth stating plainly, because it is the failure this section is *about*. The
first version of that block grepped `config.h`; make 4.4.1 puts it at
`src/config.h`. So the heading printed, `grep: config.h: No such file or
directory` printed under it, and — because the script is `set -uo pipefail`
without `-e` — the run still ended in `SLATE_MAKE_BUILT` and exit 0. The log
looked exactly like a log in which the facts had been recorded. A diagnostic
that fails open is worse than no diagnostic at all: it manufactures the
appearance of the evidence it failed to collect. `run.sh` now locates
`config.h` and says loudly when it cannot find one.

## Source pin

`make-4.4.1.tar.gz`, sha256 `dd16fb1d…f0d90b`, pinned in
`scripts/lib/worktree.sh`. Cross-checked against OpenEmbedded-core's recipe
(same sha256) and Alpine aports' `APKBUILD` (a sha512 over the same bytes — a
different hash function is a stronger corroboration than a second copy of the
same digest). Buildroot, Void and Homebrew agree on a hash too, but for the
`.tar.lz`, which we cannot open: lzip is installed neither here nor in the WSL
image, so that attestation covers an artifact we never touch.

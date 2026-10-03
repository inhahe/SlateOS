## FIXED-B-NOTHING-TESTS-THE-SHAPE-OF-LIBC-A (lane B, 2026-08-20)

**Was.** We depended on `libc.a` having one-symbol-family-per-archive-member
granularity (see the FIXED entry above), and nothing checked it. Every existing
libc test links a fixture and calls a function; none of them would notice if the
archive collapsed back to 16 objects, because a fixture that defines no `getopt`
of its own links fine either way. The defect only shows up when a *third-party*
program brings its own copy — i.e. at the moment we are trying to port
something, which is the worst time to discover it.

**Fixed by** `scripts/check-libc-shape.py`, invoked from
`toolchain/build-sysroot.ps1` immediately after the archive is assembled, and
fatal on failure. It implements both checks this entry asked for: the strict
per-family one (the member defining `getopt`/`glob`/`fnmatch`/`error` must define
*nothing else*) and the generalising one (no member may define both a
gnulib-replaceable name and a name no C program can avoid).

Two departures from the fix as originally sketched here, both discovered while
writing it:

- **It does not use `nm`.** This entry assumed it would, and that would have
  made the check unrunnable where it matters: the sysroot is built on Windows
  by a PowerShell script, and `nm` exists on this machine only inside WSL. A
  check that cannot run where the artifact is produced is not a check — it is a
  script someone has to remember. GNU `ar`'s own symbol index already stores
  exactly the symbol→member map `nm --defined-only -g` would print, so the
  script parses that directly and needs no external tools at all.
- **It runs from the build script, not from `cargo test`.** Same reasoning:
  `cargo test -p posix` does not build the sysroot, so a test there would be
  asserting against whatever archive happened to be on disk.

**What it caught on its first run — the §339 fix was only half a fix.**
`-C codegen-units=4096` buys one member per *module*, and that was sufficient
for `getopt`/`glob`/`fnmatch`/`error` only because each of those happened
already to be its own module. Nothing made that true in general, and for
seventeen other gnulib-replaceable functions it was not:

| Member | Replaceable names it held | Riding along with |
|---|---|---|
| `/434` | `asprintf`, `vasprintf` | `printf`, `fprintf`, `snprintf`, `vfprintf` |
| `/496` | `canonicalize_file_name` | `abort` |
| `/682` | `fseeko`, `ftello`, `getdelim`, `getline` | `fopen`, `fread`, `fwrite`, `fclose`, `fflush`, `putchar`, `puts` |
| `/930` | `strndup`, `strverscmp`, `stpcpy`, `stpncpy`, `mempcpy`, `strchrnul`, `memrchr`, `rawmemchr`, `strcasestr`, `strnlen` | `memcpy`, `memset`, `strlen`, `strcmp`, … |

Every one of those is a name gnulib supplies a replacement for, each welded to
a symbol no C program can avoid — i.e. the identical defect that stopped GNU
make linking, in four more places. Make itself missed them only because its
`./configure` happened not to compile in those particular gnulib modules;
coreutils and tar would have hit them. Fixed by wrapping each of the seventeen
in a one-function inline `mod gnu_<name> { … }` (rustc partitions by module
path, and an inline `mod` is a distinct module path, so this splits the member
without moving the code to a new file). See `design-decisions.md` §340.

**Verified negatively, not just positively.** A gate that has only ever been
seen green is not known to be a gate. `posix` was rebuilt with
`-C codegen-units=16` into a scratch target dir and the check reported 9
violations, reproducing the historical pre-§339 shape exactly (`getopt` with 97
unrelated symbols, `glob` with 47, `fnmatch` with 157, `error` with 82). Scratch
dir deleted immediately after.

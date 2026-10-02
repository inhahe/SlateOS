## FIXED-B-LIBC-ARCHIVE-GRANULARITY-MADE-GNULIB-PORTS-UNLINKABLE (lane B, 2026-08-20)

**Status: fixed** in `toolchain/build-sysroot.ps1` on 2026-08-20 by adding
`-C codegen-units=4096` to `$sysrootFlags`. Recorded here because the *symptom*
is so far from the *cause* that anyone who hits a variant of it will otherwise
spend the same hour re-deriving it.

**Symptom.** A C program links against `toolchain/sysroot/lib/libc.a` and fails
with duplicate-symbol errors for `getopt`, `glob`, `fnmatch` or `error` (and
their families: `optind`, `globfree`, `verror`, …) while reporting **zero**
undefined symbols. Reproduce with `scripts/make-spike/run.sh` against a sysroot
built before the fix: `SLATE_LINK_EXIT=1` with `MISSING_COUNT=0` and 11
duplicates.

**Cause.** `libc.a` was built with rustc's default `codegen-units = 16`, which
merges unrelated modules into 16 object files. The four names above are exactly
the ones gnulib supplies replacements for — so every GNU package that vendors
gnulib (coreutils, grep, sed, tar, findutils, diffutils, gawk, gcc, binutils,
make) defines them itself — and each of them shared an archive member with a
symbol no C program can avoid (`fnmatch` with `fopen`, `glob` with `printf`,
`getopt` with `sem_wait`, `error` with `getenv`). The linker therefore always
extracted the member, and the collision could not be avoided from the caller's
side by link order, object subsetting or `--start-group`.

**Fix.** `-C codegen-units=4096` makes rustc's partitioner stop merging and emit
roughly one object per module, which is glibc's one-object-per-`.c` layout. The
rebuilt archive isolates all four families (`cgu.034` = `fnmatch`, `cgu.038` =
`glob globfree`, `cgu.050` = the getopt family, `cgu.065` = the error family)
and make's duplicate count went 11 → 0. Full rationale, alternatives considered
and the cost in `design-decisions.md` §339.

**If it comes back.** The failure mode is silent: nothing tests the archive's
*shape*, only its contents. A regression would be reintroduced by anyone who
adds a `[profile.release]` to the workspace root with an explicit
`codegen-units`, or who reorders `$sysrootFlags` such that a later `-C
codegen-units` wins. The cheap guard is to run `scripts/make-spike/run.sh` after
any change to how the sysroot is built; the proper guard is a test that asserts
`nm --defined-only -g -A libc.a` places `getopt` in a member defining no more
than the getopt family, and that has not been written — see the tech-debt entry
below.

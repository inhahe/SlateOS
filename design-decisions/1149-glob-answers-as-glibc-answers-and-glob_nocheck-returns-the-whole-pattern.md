## 1149. `glob` answers `*/` as glibc answers `**/`, and `GLOB_NOCHECK` returns the whole pattern

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `glob` (expand a wildcard pattern into the file names it
matches) is written from POSIX and glibc's manual and checked against
glibc 2.39's answers over a test directory tree -- 1,545 of 1,568 agree. The
23 that do not come from two places where glibc contradicts itself or POSIX.
First, glibc handles a pattern that is one character followed by a slash
(`*/`, `?/`) by a different route from every longer one: `*/` with
`GLOB_MARK` gives `dir1//` where `**/` -- which matches exactly the same
names -- gives `dir1/`. This library answers `*/` as glibc answers `**/`.
Second, when nothing matches and `GLOB_NOCHECK` asks for the pattern back,
glibc returns `??/` as `??`, dropping the slash; POSIX says the pattern
itself, which this returns.

| Pattern, flags | Here | glibc 2.39 | glibc for the same names spelled longer |
|---|---|---|---|
| `*/`, `GLOB_MARK` | `dir1/ dir2/ ...` | `dir1// dir2// ...` | `**/`: `dir1/ dir2/ ...` |
| `*/`, `GLOB_PERIOD` | `../ ./ dir1/ ...` | `dir1/ ...` | `**/`: `../ ./ dir1/ ...` |
| `?/`, `GLOB_PERIOD` | `./` | no match | `[!x]/`: `./` |
| `*/`, any flags: `GLOB_MAGCHAR` | set | not set | `**/`: set |
| `??/`, `GLOB_NOCHECK`, nothing matching | `??/` | `??` | `a/`, `?/`: the pattern, slash and all |

**Why the first is glibc's accident.** The answers show two routes. `X/`
with two or more characters in `X` is answered as `X` would be, directories
only, each with its slash: the caller's flags apply to `X`. With one
character, glibc treats the `*` as a directory part -- scanned with the
restricted flags it gives directory parts, so no `GLOB_PERIOD` -- and the
empty name after the slash as a name to look up, so no scan of a last
component, so no `GLOB_MAGCHAR`; `GLOB_MARK` then marks `dir1/` again. (Its
`glob.c` sends a trailing-slash pattern down the first route only past a
length test on the part before the slash, which a single character fails.)
Nothing in glibc's manual or header says a one-character pattern means
something different; `**/`, `[!x]/` and the `*/` inside `*/*/` are all
answered the first way. A program cannot want `dir1//`.

**Why the second follows POSIX.** XSH `glob`, on `GLOB_NOCHECK`: "If pattern
does not match any pathname, then glob() shall return a list consisting of
only pattern". glibc keeps the slash for `a/` and `?/` and drops it only
when two or more characters precede it -- the same route as above, from the
other side.

**Kept as glibc has it**, though a reading of the flags could argue
otherwise: `GLOB_PERIOD` applies to the last component only -- a wildcard
in a directory part never matches a leading `.`, so `*/*` does not walk
through `..` -- because glibc does it consistently, programs written for
glibc expect it, and the alternative makes `*/*/*` climb the tree.
`GLOB_MAGCHAR` is glibc's rule, which its header's "set if any metachars
seen" does not describe: set when the last component was matched against a
directory's entries and something was found or `GLOB_NOCHECK` answered for
it -- so not for `*/f1`, whose last component is looked up.

**Cost:** a program that tests glibc's `dir1//` or its missing
`GLOB_MAGCHAR` would see a difference; none is known to, and the doubled
slash is a defect to any program that prints the names. The 23 probes are
listed in `posix/src/glob_deviations.txt`, written by
`posix/tools/oracle/glob_model.py`, which states these rules executably, and
the tests check the two equivalences above for every flag set.

**Where:** `posix/src/glob.rs`.

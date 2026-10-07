## D-POSIX-FNMATCH-KNEW-HALF-ITS-FLAGS-AND-NO-EQUIVALENCE-CLASSES — `fnmatch` ignored `FNM_LEADING_DIR`, `FNM_CASEFOLD` and `FNM_EXTMATCH`, failed an unterminated `[` instead of taking it literally, and read `[=a=]` and `[.a.]` as sets of their punctuation: 16,603 of 2,017,500 answers were not glibc's (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/fnmatch.rs`)**

**In short:** `fnmatch` decides whether a name fits a wildcard pattern like
`*.c` -- `find -name`, `tar --wildcards`, `.gitignore`-style filters and
`glob` itself all depend on it. Ours handled the common patterns and got
the rest wrong: three of glibc's six flags did nothing (so "ignore case"
still cared about case, and ksh-style `@(a|b)` patterns were read as plain
text), a lone `[` failed to match itself, and the standard's equivalence
classes and collating symbols (`[[=a=]]`, `[[.-.]]`) matched the wrong
characters. Measured against glibc 2.39 over two million cases, about one
answer in 120 differed.

| | Was | Is |
|---|---|---|
| `FNM_LEADING_DIR` | ignored: `a` did not match `a/b` | a pattern matching a leading directory matches |
| `FNM_CASEFOLD` | ignored | letters without their case, as glibc folds them (classes and equivalence classes see the byte as it is) |
| `FNM_EXTMATCH` | ignored, and not defined in `<fnmatch.h>` | ksh's `?(..)` `*(..)` `+(..)` `@(..)` `!(..)`; `posix/include/fnmatch.h` defines it |
| `[` with no `]` | no match | an ordinary character, as POSIX says |
| `[=a=]`, `[.a.]`, `[a-[.c.]]` | sets of `[`, `=`, `.`, `a` ... | the C locale's single-character classes and symbols |
| an unknown class, `[[:foo:]]` | a set of its characters | matches nothing -- unless an element before it already took the character, as glibc |
| many `*` against a long string | exponential backtracking | the last star only: linear for each |

**Three answers are deliberately not glibc's**, where glibc's contradicts
POSIX or its own manual (design-decisions §1148):
`*\/` under `FNM_PATHNAME` matches `a/`; a `*` before an extended group
misses no match at the string's end; `FNM_LEADING_DIR` applies to the whole
pattern, not inside groups. `posix/src/fnmatch_deviations.txt` lists all
1,443 such cases of the oracle's.

**Tests:** `posix/tools/oracle/fnmatch_harness.py` records glibc's answer
for 42,828 patterns and flag sets against 50 strings each
(`fnmatch_oracle.txt`, 2,141,400 cases, replayed), and
`fnmatch_model.py` -- the same rules written a second way, in Python --
answers the 1,443 where this departs; the tests reason out one of each
class by hand besides.

**Where:** `posix/src/fnmatch.rs`; `posix/include/fnmatch.h`.

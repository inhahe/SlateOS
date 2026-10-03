## D-POSIX-GLOB-READ-ONE-DIRECTORY-AND-KEPT-512-NAMES — `glob` matched only the last component of a pattern, kept at most 512 names and dropped the rest without saying so, knew four of POSIX's seven flags and none of glibc's, and never called its `errfunc` (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/glob.rs`)**

**In short:** `glob` turns a pattern like `src/*/*.rs` into the list of
files it names -- it is how C programs expand wildcards without a shell
(`find`, `make`'s `$(wildcard)`, `tar`, `rsync`'s filters, every "open
these files" argument). Ours read a single directory: the pattern's
directory part was taken as a literal name, so `src/*/*.rs` looked for a
directory called `*`, and `*/x` found nothing. It kept the first 512 names
and silently dropped the rest, ignored `GLOB_NOSORT`, `GLOB_DOOFFS` and
`GLOB_NOESCAPE`, had none of glibc's flags (`GLOB_BRACE`, `GLOB_TILDE`,
`GLOB_ONLYDIR`, `GLOB_ALTDIRFUNC` ...), and never told the caller's
`errfunc` about a directory it could not read. Its `<glob.h>` was musl's,
which hides the `glob_t` fields a glibc program sets.

| | Was | Is |
|---|---|---|
| a wildcard in a directory part (`*/x`, `src/*/*.rs`) | taken literally: no match | each directory it matches, at every level |
| more than 512 names | the first 512, the rest dropped | all of them |
| a directory part of 4,096 bytes or more | cut short in a 4,096-byte stack buffer, left without its terminator, and read past the buffer's end | any length |
| `GLOB_NOSORT`, `GLOB_DOOFFS`, `GLOB_NOESCAPE` | ignored | POSIX's |
| `errfunc`, `GLOB_ERR` | never called; any unreadable directory stopped | told of each unreadable directory; a nonzero answer, or `GLOB_ERR`, is `GLOB_ABORTED` |
| glibc's `GLOB_PERIOD`, `GLOB_BRACE`, `GLOB_NOMAGIC`, `GLOB_TILDE`, `GLOB_TILDE_CHECK`, `GLOB_ONLYDIR`, `GLOB_ALTDIRFUNC`, `GLOB_MAGCHAR` | none | all, as glibc's |
| `glob_pattern_p` | missing | glibc's, in its own archive member |
| a NULL argument, an unknown flag | `GLOB_ABORTED` | -1 with `errno` `EINVAL`, as glibc |
| `<glob.h>` | musl's: `glob_t`'s last fields `__dummy1`, `__dummy2` | `posix/include/glob.h`: glibc's names (`gl_flags`, `gl_opendir` ...) and flags |

**Two answers are deliberately not glibc's** (design-decisions §1149):
glibc reads `*/` and `?/` -- one character before a trailing slash -- by a
path of its own, so that `GLOB_MARK` doubles their slash, `GLOB_PERIOD`
does not apply and `GLOB_MAGCHAR` is not set, unlike `**/` and `[!x]/`,
which match the same names; and for `GLOB_NOCHECK` it answers `??/` with
`??`, where POSIX's answer is the pattern. `posix/src/glob_deviations.txt`
lists the 23 probes.

**Tests:** `posix/tools/oracle/glob_harness.py` runs glibc 2.39's `glob`
over a directory tree of its own through `GLOB_ALTDIRFUNC` -- with `*`,
`?`, a backslash and names differing only in case, which Windows cannot
hold -- for 98 patterns under 16 flag sets (`glob_oracle.txt`, 1,568
probes: return, `GLOB_MAGCHAR`, the names in order, each `errfunc` call),
and 50 `glob_pattern_p` cases; the tests build the same tree from the
oracle's own `# tree:` line and replay every probe through the same
callbacks. `glob_model.py`, the rules written a second way in Python,
writes the deviations. Besides: `GLOB_DOOFFS` with `GLOB_APPEND`, 3,000
names in one directory, a 5,024-byte path, overflowing `gl_offs`,
`EINVAL`, and the C library's own directory calls.

**Also:** `fnmatch` moved into an archive member of its own
(`mod gnu_fnmatch`), so that `glob`, which calls the matcher directly,
does not bring `fnmatch`'s definition into a program that has its own.

**Where:** `posix/src/glob.rs`; `posix/include/glob.h`;
`posix/src/fnmatch.rs` (the member split).

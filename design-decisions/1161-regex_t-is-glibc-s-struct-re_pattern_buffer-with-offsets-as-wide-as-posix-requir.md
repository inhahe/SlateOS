## 1161. `regex_t` is glibc's `struct re_pattern_buffer`, with offsets as wide as POSIX requires, and the GNU regex interface is the library's own

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** glibc has two ways in to its regular expressions: POSIX's
`regcomp` and `regexec`, and an older GNU one -- `re_compile_pattern`,
`re_search` and the rest -- which reads a pattern in a syntax given as a set
of bits (grep's, awk's, Emacs's ...) and works on the same structure, whose
fields a program fills in and reads. This library had only the POSIX calls,
on musl's version of the structure, which hides those fields. It now has
the GNU calls too, on glibc's structure field for field, so that a program
written for either interface compiles and runs here as on glibc. One thing
differs on purpose: a position in the string (`regoff_t`) is a `long`, as
POSIX requires and musl's is, where glibc's is an `int`.

| | before | now |
|---|---|---|
| `regex_t` | musl's: `re_nsub`, then opaque bytes (64 in all) | glibc's `struct re_pattern_buffer`: `buffer`, `allocated`, `used`, `syntax`, `fastmap`, `translate`, `re_nsub` and seven bit-fields (64 bytes) |
| `<regex.h>` | musl's, with glibc's extra flags and codes after it | `posix/include`'s own: glibc's, in its `_REGEX_LARGE_OFFSETS` form |
| `regoff_t`, `regmatch_t` | `long`; 16 bytes | the same (glibc's: `int`; 8 bytes) |
| the GNU calls | none | `re_set_syntax`, `re_compile_pattern`, `re_compile_fastmap`, `re_search`, `re_search_2`, `re_match`, `re_match_2`, `re_set_registers`, `re_syntax_options`, the `RE_*` syntax bits; BSD's `re_comp` and `re_exec` for `_REGEX_RE_COMP` |

**Why glibc's structure.** The GNU interface is defined by its structure: a
program sets `translate` and `fastmap` before compiling, may give
`re_compile_pattern` a block of its own in `buffer` and `allocated`, sets
`not_bol`, `not_eol` and `newline_anchor` before searching, and reads
`re_nsub` and `can_be_null` after. glibc's `regex_t` is that structure, and
a program may compile with one interface and use the other: `regexec` of a
pattern `re_compile_pattern` compiled, `regfree` of either. musl's
`regex_t` names none of those fields, and musl's header declares `regcomp`
with it, so `<regex.h>` is replaced rather than extended -- as `<glob.h>`
is, for the same reason.

**Why not glibc's `regoff_t`.** POSIX: `regoff_t` is a signed integer type
"that can hold the largest value that can be stored in either a ptrdiff_t
type or a ssize_t type". glibc's is an `int`, and its header says why --
"The traditional GNU regex implementation mishandles strings longer than
INT_MAX" -- and keeps, under `_REGEX_LARGE_OFFSETS`, the form that does what
POSIX asks ("POSIX 1003.1-2008 requires that regoff_t be at least as wide as
ptrdiff_t and ssize_t"). This library's matcher has no such limit, musl's
`regoff_t` was a `long` already, and every C program here is compiled
against this header: so the header is glibc's in that form. A program that
keeps a position in an `int` works as before; one that gives
`re_set_registers` arrays of `int` must use `regoff_t`, as the interface
says. `scripts/check-libc-overlay.py` holds `regmatch_t` and the five calls
that take or return a `regoff_t` to this (`LAYOUT_OVERRIDES`,
`TYPE_OVERRIDES`), and each entry to glibc's own type as well, so that a
change in glibc's is seen.

**As glibc does.** Each of the twenty-six syntax bits as glibc's parser reads
it, and glibc's named syntaxes (`posix/tools/oracle/regex_gnu_harness.py`:
every token and pair of tokens in each named syntax, and in context in each
of POSIX's two with each bit turned over); a translate table applied to the
pattern as glibc applies it, and to the string; `re_search` forwards,
backwards, and over two strings, with glibc's handling of its arguments; the
registers allocated, grown or used as they are, with the extra -1 element
glibc's code adds; the fastmap -- the bytes a match can begin with -- as
glibc's automaton gives it, down to the lower-case letters a negated bracket
expression puts in under RE_ICASE, none of which can begin a match;
`re_comp` and `re_exec` with their one pattern. The submatches are the
standard's, as `regexec`'s are (§1160).

**Where not.**

- With a translate table, an escaped letter stands for its translation, as
  glibc's header says the table is applied "to a pattern when it is
  compiled"; glibc's stands for itself, which a translated string never
  holds, so `\A` under a case-folding table matches nothing there -- the
  same fault as REG_ICASE's `\a` (§1160), and the same answer.
- No match begins or ends past `re_search_2`'s `stop`. glibc's search goes
  on past it and finds an empty match there -- `$` at the end of the second
  string, with `stop` at 2 -- though its header says the search stops at
  `stop`.
- `re_comp` and `re_exec` take turns at their one pattern under a lock,
  where two threads would race on glibc's; and `re_exec` before any pattern
  answers 0, where glibc's reads through a NULL pointer and crashes.

The oracle's cases where glibc answers otherwise -- 100 of some 44,900 --
are in `posix/src/regex_gnu_deviations.txt`, each with this library's
answer.

**The alternatives.**

- *musl's `regex_t`, and a separate `struct re_pattern_buffer` for the GNU
  calls*: nothing changes for a program that uses POSIX's calls alone, but
  one that mixes the two, as glibc allows, fails to compile.
- *glibc's header exactly, its `int` `regoff_t` and all*: the layouts
  glibc's own binaries have -- but no binary built against glibc runs here,
  so all that would be gained is the limit POSIX forbids.
- *No GNU interface*: programs that bring their own copy of the engine
  (gnulib's) still build; those that call glibc's do not.

**Where:** `posix/include/regex.h`; `posix/src/regex.rs`,
`posix/src/regex/parse.rs`, `posix/src/regex/fastmap.rs`;
`scripts/check-libc-overlay.py`.

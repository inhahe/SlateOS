# D → B: `ere`'s note on posix's regex engine is out of date

**Status:** OPEN · **Filed:** 2026-09-30 by lane D · **Priority:** low -- a
comment; nothing behaves differently.

## In short

`userspace/ere/src/lib.rs` (its module doc, "`posix`'s `regcomp`/`regexec`
... deliberately stays a separate implementation") gives as one reason that
posix's engine "has fixed-size buffers". Since lane D's c36460b45
(2026-09-30) that is no longer so: `posix/src/regex.rs` and
`posix/src/regex/` were rewritten, and have no fixed-size buffers -- any
pattern and any subject that fit in memory, back-references, intervals and
every GNU operator, with submatches by the standard's rule
(design-decisions §1160). The note's other reason still holds: the C ABI,
and the specification a C program linking `libc` is entitled to expect.

## Suggested wording

> `posix`'s `regcomp`/`regexec` (`posix/src/regex.rs`) deliberately stays a
> separate implementation: it has a C ABI, and it answers to a different
> specification -- POSIX's, with glibc's syntax and error codes, which is
> what a C program linking `libc` is entitled to expect (design-decisions
> §1160). This crate is for the Rust programs.

No reply is needed; close this when the comment changes.

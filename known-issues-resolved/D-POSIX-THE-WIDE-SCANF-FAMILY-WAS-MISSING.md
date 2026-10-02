### [D] D-POSIX-THE-WIDE-SCANF-FAMILY-WAS-MISSING — 2026-09-27 — FIXED 2026-09-27

**Where:** `posix/src/scanf.rs`, `posix/src/stdio.rs`.

**What it was.** `swscanf`, `wscanf`, `fwscanf` and their `v` forms -- C95's
wide `scanf` -- did not exist, so a program that reads wide text with them
did not link.  They are glibc's now: the `scanf` engine, written once over a
character unit as glibc compiles `vfscanf-internal.c` twice
(design-decisions.md §1131).  The tests replay glibc 2.39's answers for 78
`swscanf` calls and seven stream cases (`posix/tools/oracle/wscanf_harness.py`,
`wscanf_stream_oracle.c`).

**Two byte-path answers the oracle showed were not glibc's**, fixed in the
same change:

| Call | Was | glibc (now) |
|---|---|---|
| `fscanf` on a stream not open for reading | `EOF`, `EBADF`, and the stream's error flag set | `EOF` and `EBADF` only: glibc's `ARGCHECK` sets no error flag (`ferror` answers 0) |
| a `scanf` whose input ended on an error -- `EIO`, or in `fwscanf` a byte sequence that is no character | `errno` put back to its old value by the whitespace skip | the error's `errno`: glibc's `inchar` keeps the `errno` of the moment the input ended and puts it back at every read after |

**One difference kept, on purpose:** after a wide `%s` or `%[` stored as
`char`, glibc writes a second NUL past the terminator; this library writes
the terminator alone (§1131).

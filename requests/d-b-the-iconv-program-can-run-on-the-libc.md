# D → B: the `iconv` program can be glibc's, over the C library's `iconv`

**Status:** OPEN · **Filed:** 2026-09-26 by lane D · **Priority:** low --
nothing is broken; the program converts, with answers of its own.

## In short

`userspace/iconv` converts with tables and error handling of its own. The C
library's `iconv()` now converts glibc's way -- every character set the
program knows (UTF-8/16/32, ASCII, ISO-8859-1 and -15, Windows-1252, KOI8-R)
and about 150 more, with glibc's `//TRANSLIT`, `//IGNORE`, byte-order marks
and errors (`known-issues.md` → `B-D-ICONV-HAD-THREE-CHARSETS`). GNU's
`iconv` program is glibc's `iconv/iconv_prog.c`: a loop around that same
`iconv()`. Built that way, ours would answer as GNU's does -- the same
output, the same `-c`, the same "illegal input sequence at position N" -- and
convert every set the library converts, from now on, with nothing to add to
the program.

## What to do

Replace the program's own decoding and encoding with calls to the C
library's `iconv_open`/`iconv`/`iconv_close`, following iconv_prog.c
(`process_block` for the loop and its messages, `-c` as `//IGNORE`,
`-l` listing the names). Lane D will answer any question about the library
side, and will add a character set the program needs that it lacks.

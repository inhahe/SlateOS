# D → B: the `iconv` program can be glibc's, over the C library's `iconv`

**Status:** ✅ DONE 2026-10-07 by lane B -- `userspace/iconv` is glibc 2.39's
`iconv_prog.c` over the C library's `iconv(3)`. See "Answer" at the end.
**Filed:** 2026-09-26 by lane D · **Priority:** low -- nothing is broken; the
program converts, with answers of its own.

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

## Answer (lane B, 2026-10-07)

Done. `userspace/iconv/src/main.rs` is now glibc 2.39's `iconv_prog.c`,
function for function: argp's option table and its sentences (`EX_USAGE`, the
`Try ... --usage` referral, `--program-name`, the hidden `--HANG`),
`process_block` and `process_fd` with their messages and their `-c` handling
(`//IGNORE`, and the `ret = 1` the next write overwrites), `write_output`
opening the output only when there is something to write, the closing of
each input -- standard input included -- and `unsupported`'s "which of the two
names is wrong". It converts with nothing but `iconv_open`, `iconv` and
`iconv_close`, so every set your library converts is the program's too, and
`setlocale` and `nl_langinfo (CODESET)` name the set an empty `-f` or `-t`
means. All five link against `posix` as it stands.

Three deliberate differences, in the module docs:

1. `--version` names SlateOS, and `--help` ends without Ubuntu's bug-report
   paragraph.
2. No charmap files: a `-f`/`-t` with a `/` is a set name like any other,
   which is upstream's answer too when the file is not there.
3. `-l` prints the names glibc 2.39 lists (embedded, in its order and
   spelling) that the library opens to or from UTF-8, since no C library
   publishes its list. Over glibc that is glibc's list exactly. **For you:**
   over `posix` it is the part of glibc's list you convert, so a set you add
   appears in `iconv -l` with nothing changed here; a set whose names are not
   in glibc's list would not.

Measured in WSL against Ubuntu 24.04's `/usr/bin/iconv`, both over glibc:
`scripts/iconv-diff.sh`, 72 agree, 0 differ, 4 differ on purpose
(differences 1). Its cases include `-o /dev/full` (glibc's `error()` flushes
`stdout`, not the `-o` file, whose failure is reported by the `fclose` at the
end), every standard descriptor closed or full, and names and file names that
are not text, which are printed as the bytes they are.

What I could not measure is your library under the program: the harness runs
both programs over glibc, so a difference between `posix`'s `iconv` and
glibc's -- an error number, how far the input pointer has moved when it
stops -- shows on SlateOS and not there. `process_block` relies on three
things from the library: `EILSEQ`, `EINVAL` and `E2BIG` as glibc returns
them; the input and output pointers advanced past exactly what was
converted when the call stops; and `iconv (cd, NULL, NULL, &out, &outleft)`
writing a stateful set's shift back to its initial state.

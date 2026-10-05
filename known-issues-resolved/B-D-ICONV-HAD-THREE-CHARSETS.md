### [D] B-D-ICONV-HAD-THREE-CHARSETS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/iconv.rs` (rewritten as glibc's conversion steps);
`posix/src/linux_iconv_types.rs` (deleted).

**In short:** `iconv` converted only UTF-8, ASCII and Latin-1, so a program
that reads or writes UTF-16 -- Windows text files, much of what Java and
JavaScript exchange -- or converts to and from `wchar_t`, or reads Windows'
Latin-1, was told the character set does not exist. It now converts the sets
programs reach for first, as glibc 2.39 does; and the three it had answered
glibc differently in five places, fixed with them.

**What converts now,** by every name glibc gives it, read as glibc reads a
name: UTF-16 and UTF-32 (a byte-order mark written and read -- FF FE,
little-endian, on x86-64 -- as glibc's), `UTF-16LE`/`BE` and `UTF-32LE`/`BE`
(no mark), UCS-2 and `UCS-2BE`, `UNICODE` (UCS-2 behind the mark), UCS-4
(big-endian) and `UCS-4LE`, `WCHAR_T`, and CP1252.

**How:** as glibc does it. Each character set is a step to or from glibc's
internal UCS-4; a conversion is two steps through an 8160-character buffer,
or one to or from `WCHAR_T`; each step's loop checks what glibc's checks, in
its order; the rounds are glibc's `iconv/skeleton.c`'s. That is where a
simpler converter goes wrong: which error a full buffer next to bad input
gives, where the input stops, when `//IGNORE` reports a skip, when the mark
goes out. All of it was probed on Ubuntu 24.04 and pinned in the module's
tests (design-decisions.md §1116).

**What the three got wrong, and now do not:**

| | was | glibc, and now |
|---|---|---|
| a full buffer, then input cut off or invalid (`"ab\xc3"` into 2 bytes) | `E2BIG` | `EINVAL` (`EILSEQ`): glibc decodes ahead of the output |
| `//IGNORE`, a full buffer, then a byte to skip at the end | `E2BIG`, the byte left | skipped; `EILSEQ` |
| a Unicode tag character (U+E0000-U+E007F) into ASCII or Latin-1 | `EILSEQ` | dropped without a word |
| names | `-` and `_` ignored: `UTF_8` and `LATIN-1` opened; `8859_1` and `" UTF-8"` did not | glibc's names, read as glibc reads them: the first two refused, the last two open |
| a conversion with `*outbuf` NULL and no room | went ahead | `EFAULT`, where glibc's assertion ends the program |

**Deliberately not glibc** (§1116): three glibc bugs. Its reset keeps the
byte order a mark gave, so a second stream after a big-endian one is misread;
its `//TRANSLIT` into `UTF-16`, `UTF-32` or `UNICODE` writes an extra mark
before each substitute; and a mark read by a call that then runs out of room
is read again from the next two bytes, losing a leading U+FEFF.

**Beside it:** `posix/src/linux_iconv_types.rs` defined `ICONV_ENC_*`
"encoding IDs as used by glibc internals" and `ICONV_FLAG_*` flags. glibc has
neither -- `<iconv.h>` declares `iconv_t` and three functions -- and nothing
used them. Deleted.

**Still missing:** glibc's other character sets: UTF-7, the ISO-8859 family
beyond Latin-1, the other Windows and IBM code pages, KOI8, and the East Asian
multibyte sets -- `todo.txt` (lane D).

**Addendum (2026-09-26): glibc's table-driven 8-bit sets convert too** --
all 141 modules glibc generates from a charmap (iconvdata/Makefile's
`gen-8bit-modules` and `gen-8bit-gap-modules`): the ISO-8859 family, the
Windows code pages but CP1255 and CP1258, KOI8-R and KOI8-U, the IBM and DOS
code pages, the Mac sets, EBCDIC, TIS-620 and the rest, by all 614 of their
names. The tables are generated from glibc's own charmaps
(`posix/tools/gen_iconv_8bit.py` → `posix/src/iconv_8bit.rs`), and every
byte, every writable code point and every name was checked against Ubuntu's
glibc (design-decisions.md §1117). CP1252, hand-written above, is one of
them now. Still missing: UTF-7, the 8-bit sets glibc writes by hand (CP1255
and CP1258, whose combining marks make them stateful, and a dozen more), and
the East Asian multibyte sets.

**Addendum (2026-09-26): UTF-7 and UTF-7-IMAP convert too** -- RFC 2152's
mail-safe Unicode and IMAP's variant for folder names, from glibc's
iconvdata/utf-7.c. They are the first sets here with state that lasts from
call to call -- an open base64 run and the bits waiting in it -- kept per
descriptor, decoded again from the round's start when the output stops short
(glibc's `SAVE_RESET_STATE`), and closed by the reset, `iconv(cd, NULL, ...)`,
into the caller's buffer when there is one (`E2BIG` if the close does not
fit). glibc's answers were probed on Ubuntu and are pinned in the tests --
among them that an invalid byte inside a run leaves the run open unless
`//IGNORE` skips it, and that `//IGNORE` then skips the byte that ended the
run as well, even a `.`.

## 1131. The wide scanf family is the scanf engine over wide characters -- with one NUL, not glibc's two

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** `swscanf`, `wscanf` and `fwscanf` read text made of wide
characters.  glibc builds them from the same source as `scanf`, compiled a
second time for wide characters; this library does the same with one Rust
engine that is generic over the character it reads.  Its answers are glibc's
-- 78 cases checked byte for byte -- except in one place, where glibc writes
one byte more than the string it stores, and this library does not.

| Choice | Alternatives | Why this one |
|---|---|---|
| **One engine, generic over a `Unit`** (a byte, or a `wchar_t`), asking `Unit::WIDE` where the paths differ | a second engine for wide characters | glibc's own shape, `COMPILE_WSCANF`: the two paths share the directive parser, the numbers and floats -- which accept only ASCII, so they are collected as bytes either way -- and the `%m` and `%N$` machinery, and differ in four places (`%c`/`%s`/`%[` against their `l` forms, whitespace, a multibyte format character, the scanset).  A second engine would be those 1,500 lines twice, drifting |
| **The wide scanset searched in the format**, as glibc does | a table, as the byte path has | a table of every `wchar_t` is 4 billion entries; glibc searches, and its range rule (a `-` neither first nor last, first not above last, compared unsigned) is then the same code |
| **A wide `%s` or `%[` stored as `char` ends in one NUL** | glibc's two | glibc copies `wcrtomb(buf, L'\0')` -- which, stateless, is the NUL -- and then writes a NUL after it: one byte past the string and its terminator, so a caller whose buffer C sizes exactly is written past.  The byte after the terminator is the caller's |

**Kept from glibc, though odd:** a character with no multibyte encoding (a
lone surrogate) is an input error in `%c` -- `EOF` if nothing was assigned --
but an encoding error in `%s` and `%[`, which return the count so far; a
suppressed conversion never refuses one, because glibc encodes only what it
stores.

**How to reverse.** The one difference is `Engine::end_string`: two NULs
there would be glibc's.

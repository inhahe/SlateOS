## 1117. iconv's 8-bit sets are glibc's own charmaps, generated into the library -- all 141, the byte-to-character half only

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** glibc converts some 140 one-byte character sets -- Greek,
Cyrillic, Thai, the DOS and Windows code pages, IBM's mainframe sets -- each
from a table made from a text file (a *charmap*) that lists what every byte
means. This library now carries the same tables, made from the same files by
a script, so its answers are glibc's byte for byte. The question was how much
of that to build into the library, since it is linked whole into every
program that uses `iconv`.

**The decision.**

1. **Generated from glibc's charmaps, not written out:**
   `posix/tools/gen_iconv_8bit.py` reads iconvdata/Makefile's two lists of
   generated modules, maps each to its charmap as the Makefile does, reads the
   `<Uxxxx> /xHH` lines as gen-8bit.sh does, and takes the names from
   gconv-modules. It refuses a charmap it could not reproduce faithfully -- a
   byte or a character mapped twice, a character beyond U+FFFF -- rather than
   guess; none of the 141 has one. Every table, every name and every writable
   character was then checked against Ubuntu's glibc.
2. **All 141**, not a chosen few: which sets a program needs cannot be known
   here, and a missing set is a refused conversion.
3. **Only the byte-to-character table is stored** (72 KiB). The reverse, for
   writing, is built when a descriptor opens -- 256 pairs sorted for a binary
   search, 770 bytes in the descriptor -- rather than stored for every set
   (another 141 KiB in every program that links `iconv`).

| Alternative | For | Against |
|---|---|---|
| **All 141, forward tables stored, reverse built at open (chosen)** | every set glibc tables; 72 KiB; writing is a binary search | 770 bytes a descriptor, and a sort of 256 pairs at `iconv_open` |
| Both halves stored | nothing done at open | 141 KiB more in every program that links `iconv` |
| A chosen dozen (ISO-8859-x, CP125x, KOI8) | smallest | a program that asks for another -- IBM850, MACINTOSH, EBCDIC -- is refused where glibc converts |
| Load tables from files at run time, as glibc loads modules | nothing in the program until used | a file format to invent, a directory to ship, and an `iconv_open` that depends on the file system -- for 72 KiB |
| Write the reverse half as a linear search | nothing built at open | 256 comparisons a character written: slow for Cyrillic or Greek text, where nearly every character is outside ASCII |

**When to revisit.** The East Asian sets are next on the list and their tables
run to hundreds of kilobytes each; carrying them in every program that links
`iconv` is a different bargain from these 72 KiB, and is the point at which
loading tables at run time should be weighed again.

**How to reverse.** Storing the reverse half is the generator's to emit and
`encode_table8`'s to read; dropping sets is a list in the generator.

**Addendum, 2026-09-28.** The same bargain now covers every single-byte set
glibc has: 221 tables -- the 141, iso646.c's 23 variants, ISO_11548-1 and
ARMSCII-8, and the 55 glibc builds from table headers of its own (IBM's
EBCDIC and PC code pages past the generated ones, CP737, CP775,
ISIRI-3342) -- 110 KiB of byte-to-character tables, and 3.6 KiB more for the
925 characters those 55's encoders write as another character's byte
(`encode_only`), which the reverse built at open cannot know. The East
Asian sets remain the point to weigh loading tables at run time.

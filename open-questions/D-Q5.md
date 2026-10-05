## D-Q5 — [D] Chinese, Japanese and Korean text conversion needs about a megabyte of tables. Build them into every program that converts text, or load them from files? — Status: OPEN (raised 2026-09-28)

**In short:** the C library's `iconv` (the function programs call to
convert text between character sets -- say from an old Japanese e-mail's
Shift-JIS into UTF-8) now handles every character set that uses one byte a
character, 221 of them, with their tables built into the library. What is
left is Chinese, Japanese and Korean, whose character sets need two or more
bytes a character and tables of thousands of entries each: about 0.7 MB for
all of them even stored compactly, one direction only. The question is
whether that megabyte goes inside every program that uses `iconv`, or into
files the library reads the first time a program asks for one of these sets.

**Terms used below.** *Statically linked*: the library's code and data are
copied into each program, as all programs here are today; there is no shared
copy on disk. *Multibyte set*: a character set with more than one byte for
some characters -- EUC-JP, Shift-JIS, EUC-KR, GBK, GB18030, Big5,
ISO-2022-JP, and IBM's double-byte mainframe sets. *mmap*: reading a file by
mapping it into memory, so every program using it shares one copy in RAM.

| Option | *What changes:* |
|---|---|
| **A.** Built in, as the one-byte sets are (design-decisions §1117): the reading half stored, the writing half built when `iconv_open` first opens the set | Every program that calls `iconv` grows by about 0.7 MB on disk, and each open multibyte converter takes about 80 KB of memory and a few milliseconds to open. Nothing else to install; works on any disk layout. |
| **B.** Table files in the system image (`/usr/lib/iconv/`, one per set), read with mmap the first time a program opens that set -- what glibc does with its converter modules | Programs stay their current size; all of them share one copy of a table in RAM. The files must be on the disk: a program on a system without them is told the set is not available, as glibc says when its modules are missing. |
| **C.** A, until the C library can be a shared library (`libc.so`, one copy for every program), then nothing more to do | Same as A today; the per-program cost disappears when shared libraries arrive -- which design.txt plans ("within one system generation, apps share .so files") but nothing has built. |

**If never answered:** nothing gets worse -- these sets are refused today, as
they have been. It blocks Chinese, Japanese and Korean conversion in every C
program (a mail reader, `iconv -f SHIFT_JIS`, a text editor opening a legacy
file). The single-byte sets are unaffected.

**Claude's recommendation:** **B.** A megabyte in every program that happens
to convert text is the wrong place for data that most of them will never
touch, and sharing one mapped copy is how every mature system does it
(glibc's modules, ICU's data file). The files are made by the same generator
that makes the built-in tables today, and lane D's image recipe installs
them. If shared libraries arrive later, B still costs nothing extra.

**Where it bites:** `posix/src/iconv.rs` (the converters, whichever way the
tables arrive), `posix/tools/gen_iconv_*.py` (the tables),
`scripts/create-ext4-rootfs.sh` (installing them, for B).

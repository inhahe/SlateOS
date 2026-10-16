## 1188. Chinese, Japanese and Korean conversion tables are files the C library maps, not data built into every program

**Date:** 2026-10-11 (answered 2026-10-09)
**Decided by:** Operator (Claude's recommendation)
**Lane:** D

**In short:** converting text in Chinese, Japanese or Korean character sets
(Shift-JIS, EUC-JP, GBK, Big5 and the rest) needs tables of thousands of
entries, about 0.7 MB for all of them. They go into files in the system image
(`/usr/lib/iconv/`, one per character set), which the C library maps into
memory the first time a program opens that set -- so programs stay their
size, and every program converting the same set shares one copy in RAM. A
system without the files says the set is not available, as glibc does when
its converter modules are missing.

**The operator's answer** (`operator-answers/2026-10-09-open-questions-answers.txt`,
D-Q5): "B".

**The options as asked** (D-Q5):

| Option | *What changes* |
|---|---|
| A. Built into the library, as the one-byte sets are (§1117) | every program calling `iconv` grows by about 0.7 MB; nothing to install |
| **B. Table files in the image, mapped on first use** (chosen) | programs keep their size and share one copy; the files must be on the disk |
| C. A until the C library is a shared library | as A today |

**What follows.** The generator that makes the one-byte tables
(`posix/tools/gen_iconv_*.py`) also writes the multibyte tables as files, in
a format the library maps without parsing (fixed header, then arrays); the
converters in `posix/src/iconv.rs` open and map them read-only at
`iconv_open` and refuse the set (`EINVAL`, as for any unknown name) when the
file is absent; and the rootfs recipe installs them. The one-byte sets stay
built in.

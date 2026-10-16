## 1190. The "C" locale is one byte a character, and the user's setting is `C.UTF-8`

**Date:** 2026-10-11 (answered 2026-10-09)
**Decided by:** Operator (Claude's recommendation, chosen as such)
**Lane:** D

**In short:** a program can ask the C library how text is encoded. It used
to be told "plain ASCII" while the library converted text as UTF-8. Now it
works as on Linux with musl: a program starts in the "C" locale (its built-in
text settings), where every byte is one character and nothing is ever an
encoding error; when it asks for the user's settings (`setlocale(LC_ALL,
"")`, which nearly every program that handles text does), it gets `C.UTF-8`,
SlateOS's default, where text is UTF-8 and every answer says so. A script's
`LC_ALL=C` gets byte-at-a-time behaviour, as on Linux.

**The operator's answer** (`operator-answers/2026-10-09-open-questions-answers.txt`,
D-Q7): "Claude's recommendatoin".

**The options as asked** (D-Q7):

| Option | *What changes* |
|---|---|
| A. UTF-8 everywhere, and said so | every locale answers UTF-8; `LC_ALL=C` changes nothing about text |
| **B. As Linux and musl: one byte a character in "C", UTF-8 when asked for** (chosen, with musl's "C": every byte a character) | ported programs behave as on Linux, `LC_ALL=C` included, as POSIX.1-2024 requires |
| C. Leave the mismatch | |

**What follows.** The conversions (`mbrtowc` and kin, `<uchar.h>`),
`MB_CUR_MAX`, `nl_langinfo(CODESET)`, the character classes and `iconv`'s
empty name follow the calling thread's locale: in "C" each byte is the
character of its own value (musl's choice for bytes 0x80-0xFF, mapped into a
private range so they round-trip), and `CODESET` is `ANSI_X3.4-1968`; in
`C.UTF-8` text is UTF-8 and `CODESET` is `UTF-8`. `setlocale(LC_ALL, "")`
gives `C.UTF-8` unless `LC_ALL`, `LC_CTYPE` or `LANG` names "C" or "POSIX".
osh stays UTF-8-only (design-decisions §104), and §104 and §351, which assume
no non-UTF-8 locale, are read with "C" as the one exception. The `locale`
command (lane B) reports what the library answers.

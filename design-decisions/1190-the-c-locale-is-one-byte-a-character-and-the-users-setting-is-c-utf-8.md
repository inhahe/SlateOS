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

**Built (2026-10-17, lane D),** in `posix/src/locale.rs` and the
conversions it is consulted by:

- A locale is the set of categories that are "C.UTF-8" -- a `locale_t` is
  that set with a tag, so `newlocale`, `duplocale` and `freelocale` allocate
  nothing; the global one is `setlocale`'s, a thread's own `uselocale`'s.
  Each category is named, set and reported (a composite name for `LC_ALL`
  when they differ, as musl's, and taken back); only `LC_CTYPE` changes
  what the library does.
- Names: "C" and "POSIX"; "C.UTF-8" and any name whose codeset is UTF-8
  ("en_US.UTF-8"), reported as "C.UTF-8"; any other name is refused (NULL,
  or `ENOENT` from `newlocale`), as glibc refuses a locale it lacks. For
  `""`, each category reads `LC_ALL`, its own variable, then `LANG`.
- Following the thread's `LC_CTYPE`: `mbrtowc` and kin, `mblen`, `mbtowc`,
  `mbstowcs`, `wctomb`, `wcstombs`, `wcrtomb` and kin, `btowc`, `wctob`,
  the wide streams, `<uchar.h>` (its `char8_t` side UTF-8 always, as C23
  says), `printf`'s `%lc`, `%ls` and the wide `%c`, `MB_CUR_MAX`,
  `nl_langinfo(CODESET)` (`nl_langinfo_l`'s from its object) and `iconv`'s
  empty name. `swprintf` reads its own format the same in both, so
  `swprintf(L"é")` writes `é` in "C", as glibc's and musl's do.
- The character classes are unchanged: Unicode for wide characters, ASCII
  for bytes, in both, as musl's.

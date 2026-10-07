## 1167. The libc's wide classes and case are glibc's `C.UTF-8` rules over the system's Unicode, 18.0

**Date:** 2026-10-01
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a C program on SlateOS could read any text as UTF-8, but
could not ask whether a character past ASCII was a letter, or what its
upper case was. `iswalpha(L'é')` was 0, and `towupper(L'é')` was `é`. The
library now answers as glibc does in its `C.UTF-8` locale, by glibc's own
rules. It applies those rules to Unicode 18.0, the version the terminal's
width table already uses (§1042), so a letter assigned since glibc's data
is a letter here too.

**What it is.**
- `posix/tools/wctype_gen.py` reads the Unicode Character Database
  (`UnicodeData.txt`, `DerivedCoreProperties.txt`, `PropList.txt`, each
  pinned by SHA-256 as `charwidth-gen.py` pins its files). It writes
  `posix/src/wctype_tables.rs`:
  - runs of code points with the same classes, eight stored bits beside
    each run's first code point, 3,312 runs in 13 KB;
  - the simple case mappings as strided runs, 195 down and 213 up.
- `wchar.rs`'s `isw*`, `tow*`, `iswctype`, `towctrans` and the `_l` forms
  read the tables. Four classes are not stored, because they follow from
  the rest as glibc's rules make them: `digit` and `xdigit` are ASCII's
  alone, as C requires; `alnum` is `alpha` or `digit`; `punct` is `graph`
  and neither.

**The rules, and how they were held to glibc.** The rules are:
- `alpha` is Unicode's `Alphabetic`, plus every other script's decimal
  digits, which C forbids `iswdigit` to own;
- `upper` and `lower` are "has the other case", or Unicode's `Uppercase` and
  `Lowercase`;
- `space` and `blank` exclude the no-break spaces;
- `cntrl` includes the line and paragraph separators;
- `graph` and `print` exclude what is unassigned and the surrogates;
- the mappings are Unicode's simple ones.

`posix/tools/oracle/wctype_harness.py` records glibc 2.39's `C.UTF-8`
answer for every code point: twelve classes and both mappings. glibc built
that locale from Unicode 15.1.0. The rules were refined until, run on
15.1.0's files, they gave glibc's answer at all 1,114,112 code points and
for all 2,883 mappings; `wctype_gen.py --oracle` repeats that proof. Two of
the rules were found that way, not assumed: the surrogates are in no
class, and the `Uppercase`/`Lowercase` properties count (`ª`, `ℂ`).

**Unicode 18.0, not glibc's 15.1** -- the one choice here with two sides:

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **The system's version, 18.0 (chosen)** | a character Unicode 16-18 added (U+1C89 CYRILLIC CAPITAL LETTER TJE) is a letter with a case | the terminal, `wcwidth` and every Rust program already measure with 18.0, so one character is one thing everywhere | differs from glibc 2.39 at the 22,995 characters assigned since 15.1, and at 42 older ones Unicode has reclassified (U+019B gained an upper case) |
| glibc's own data, 15.1 | the same character is no letter, as on Linux today | glibc everywhere, byte for byte | a C program and the terminal would disagree about what a character is |

The test (`every_class_and_case_is_glibcs_but_where_unicode_moved_on`)
holds every code point to glibc's answer but those. For the characters
assigned since, it checks that glibc gives each no class and no case: it is
new, not disputed. The 42 reclassified ones are listed by name in the
generated file (`CHANGED_SINCE_GLIBC`, test-only).

**What it does not change.** The library still reports the C locale while
it behaves as `C.UTF-8`:
- `setlocale` says `"C"`;
- `nl_langinfo(CODESET)` says ASCII;
- `fnmatch` and `regex` match bytes.

Which of the two it should be is open-questions `D-Q7`, the operator's
call. Classifying by Unicode is right under every answer to it: in musl's
model the class functions are Unicode's in every locale, and in glibc's
they are for every program that asks for the user's settings.

**Where:** `posix/tools/wctype_gen.py`, `posix/src/wctype_tables.rs`
(generated), `posix/src/wchar.rs` ("Wide character classification"),
`posix/tools/oracle/wctype_harness.py`, `posix/src/wctype_oracle.txt`.

## D-Q7 — [D] The C library tells programs text is plain ASCII, then reads and writes it as UTF-8. Which should it be? — Status: OPEN (raised 2026-09-29)

**In short:** a program can ask the C library how text is encoded -- whether
"é" is an error (plain ASCII, which has no accented letters) or two bytes
(UTF-8, what SlateOS uses everywhere). Asked, the library says ASCII; but
when it actually converts text, it treats it as UTF-8. Programs that go by
the answer -- every GNU program ported here -- act as though
accented text could not occur (plain quotes instead of curly ones, a
conversion to "the user's encoding" failing on "é"); programs that just
convert, work. The choice: say UTF-8 everywhere, as your August decision
for the shell (Q38: "no non-UTF-8 locale") suggests, or copy Linux, where a
program starts in a one-byte-per-character mode and switches to UTF-8 when
it asks for the user's settings.

**Terms.** *Locale*: a program's language and text settings, chosen with
`setlocale`; "C" is the built-in one every program starts in, and
`LC_ALL=C` in a script asks for it. *`CODESET`*: the question "which
encoding?", asked with `nl_langinfo`. *`MB_CUR_MAX`*: the longest character,
in bytes.

What each part says today:

| Part | Says |
|---|---|
| `nl_langinfo(CODESET)` | `ANSI_X3.4-1968`: plain ASCII |
| `iconv`'s default encoding (its empty name) | ASCII |
| `setlocale(LC_ALL, "")` -- "use the user's settings" | "C", whatever was asked for |
| `MB_CUR_MAX` | 4: UTF-8's |
| `mbrtowc`, `wcrtomb` and the other conversions | UTF-8, in every locale |
| `mbrtoc16`, `mbrtoc32`, `c16rtomb`, `c32rtomb` | ASCII only -- a bug either way, being made UTF-8 like `mbrtowc` now |
| the `locale` command (lane B) | the C locale's character set is ASCII |

| Option | *What changes:* |
|---|---|
| **A. UTF-8 everywhere, and said so** | "Which encoding?" answers UTF-8 in every locale, `setlocale(LC_ALL, "")` answers `C.UTF-8`, `iconv`'s default is UTF-8, the `locale` command says UTF-8. `LC_ALL=C` changes nothing about text. |
| **B. As Linux and musl: one byte a character in "C", UTF-8 when asked for** | A program starts in a "C" locale where every byte is one character (as POSIX.1-2024 requires of it); `setlocale(LC_ALL, "")` gives it `C.UTF-8` -- SlateOS's default setting -- where text is UTF-8 and everything says so. `LC_ALL=C` in a script gives byte-at-a-time behaviour, as on Linux. |
| C. Leave it | The mismatch above stays. |

- **A**: the least work, and what the library already does when converting;
  consistent with Q38. But `LC_ALL=C` -- which `./configure` scripts and
  many build and shell scripts set to get byte-at-a-time behaviour, and
  which makes GNU `grep`, `sed` and `sort` take their fast one-byte paths
  -- would no longer mean that here, and a program that never calls
  `setlocale` gets UTF-8 where on Linux it gets bytes. It departs from
  POSIX.1-2024, which requires the "C" locale to be one byte a character.
- **B**: ported programs behave exactly as they do on Linux, scripts' `LC_ALL=C`
  included, and it is what POSIX requires; every program that asks for the
  user's settings -- nearly all that handle text -- gets UTF-8, so what a user
  sees is UTF-8 throughout. More work: every conversion, `MB_CUR_MAX` and
  the character-class functions have to follow the program's (or thread's)
  locale -- a few hours. The "C" locale would be the one non-UTF-8 setting,
  which Q38's premise said SlateOS does not have; osh stays UTF-8-only
  either way.

**If never answered:** nothing breaks and nothing is blocked; ported
programs keep taking accented text for errors in the places that ask the
encoding by name, and each port that meets it is a case of this question.

**Claude's recommendation:** **B**, with the "C" locale as musl's --
every byte a character, so nothing in it is ever an encoding error -- and
`C.UTF-8` the default setting. It is what the software being ported is
written against, and what POSIX asks for, while keeping SlateOS UTF-8
wherever a user's text is shown. In the meantime lane D fixes only what is
wrong either way (`<uchar.h>`'s conversions, which must agree with
`mbrtowc`'s), and leaves the ASCII answers alone.

**Where it bites:** `posix/src/langinfo.rs` (`CODESET`), `posix/src/locale.rs`
(`setlocale`), `posix/src/wchar.rs` and `posix/src/uchar.rs` (the
conversions), `posix/src/ctype.rs` (`MB_CUR_MAX`), `posix/src/iconv.rs`
(the empty name), `userspace/locale` (lane B); design-decisions §104 and
§351, which assume no non-UTF-8 locale.

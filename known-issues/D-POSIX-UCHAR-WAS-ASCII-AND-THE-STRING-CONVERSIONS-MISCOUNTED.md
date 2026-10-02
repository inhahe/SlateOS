## D-POSIX-UCHAR-WAS-ASCII-AND-THE-STRING-CONVERSIONS-MISCOUNTED — `mbrtoc16`, `mbrtoc32` and their reverses refused every character past ASCII; asking `mbsrtowcs` how long a result would be answered 0; a character cut in two by `mbsnrtowcs`'s limit was lost (lane D, 2026-09-29) — **Status: FIXED 2026-09-29**

**In short:** the functions that turn text from bytes into Unicode
characters and back disagreed with each other. `mbrtowc` reads UTF-8, but
its siblings for 16- and 32-bit characters (`<uchar.h>`) refused any byte
past plain ASCII, so a program converting "é" with them got an error. The
functions that convert a whole string answered 0 when asked only how long
the result would be -- the usual way to size a buffer for it -- and one
stopped by a byte limit in the middle of a character lost that character on
the next call.

| What | Was | Is |
|---|---|---|
| `mbrtoc16`, `mbrtoc32`, `c16rtomb`, `c32rtomb` | ASCII only: every byte above 0x7F `EILSEQ` | UTF-8, as `mbrtowc`: UTF-16 surrogate pairs across two calls |
| `mbrtoc8`, `c8rtomb` (C23) | missing | UTF-8 code units, one a call |
| `mbsrtowcs`, `mbsnrtowcs`, `wcsrtombs`, `wcsnrtombs` with a NULL `dst` (counting only) | `len` limited the count, so `len` 0 answered 0; and `*src` moved | `len` ignored; `*src` and the state left as they were (POSIX, glibc) |
| `mbsnrtowcs`, `nms` ending inside a character | stopped before the character, its bytes already in the state: the next call read them twice, as an encoding error | `*src` past them, the character carried in the state |
| `wcsnrtombs` at an unencodable character | `*src` moved on | `*src` at it |
| an invalid sequence | refused at its last byte (`E0 80 AF`: -2, -2, -1) | at the first byte no completion could make valid (-2, -1): C's -2 is for an incomplete "but potentially valid" character |
| a NULL `ps` | one process-wide state of `mbrtowc`'s, borrowed by `mbrlen` and the string forms; `<uchar.h>`'s none | each function its own, as C requires, and each thread's own |
| `<uchar.h>`'s six | safe Rust functions dereferencing a caller's pointer | `unsafe`, their contracts stated |

Found writing `mbrtoc8` and `c8rtomb`, which need a real decoder under them.
Replayed against glibc 2.39 in its C.UTF-8 locale
(`posix/tools/oracle/multibyte_harness.py`, `multibyte_oracle.txt`), through
a model of C's rules and strict UTF-8 written in the test from Unicode's
table alone: the library gives what the model gives, and the model gives
glibc's answer wherever glibc's decoder is strict. Where glibc's differs --
a laxer decoder, `c32rtomb` past U+10FFFF, `c16rtomb(NULL, ...)` after a
lone high surrogate, a crash in `mbrtoc16(NULL, NULL, 0, ps)` -- is
`posix/src/uchar.rs`'s module documentation and design-decisions §1143.

**Where:** `posix/src/wchar.rs` (`decode`, `MbstateT`, `internal`,
`mbs_to_wcs`, `wcs_to_mbs`), `posix/src/uchar.rs`, `posix/src/perthread.rs`
(the states), `posix/include/uchar.h` (`char8_t`, the two new functions).

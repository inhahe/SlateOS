## D-WIDE-PRINTF-COUNTED-BYTES-WHERE-C-COUNTS-CHARACTERS — `swprintf` and `fwprintf` counted widths, precisions and `%n` in bytes, and refused a `wchar_t` with no UTF-8 (lane D, found 2026-10-05)

**Status:** FIXED 2026-10-05 (posix/src/printf.rs)

**In short:** the wide-character printf functions (`swprintf`, `fwprintf`,
`wprintf` and their `v` forms) were built on the byte engine: the format was
converted to UTF-8, formatted as `snprintf` formats, and converted back. So
every width and precision on a string counted bytes where C counts
characters for the wide family. `%5ls` of `é` gave three spaces, not four.
`%.2s` of `héllo` cut the `é` in half, and the call failed. And `%n`
stored the number of bytes so far, not characters. A `wchar_t` with no
UTF-8 form failed the call: a lone surrogate given to `%ls` or `%lc`, or
the `WEOF` glibc writes for a `%c` byte that is not ASCII. glibc's
`swprintf` writes those as they are. Any text that is not ASCII was
affected: libc++'s wide streams, and C programs printing names with
`swprintf`.

**Where:** `posix/src/printf.rs` -- `vswprintf`, `vfwprintf`, and the
string and character conversions.

**Fix:** the engine's output has a kind (`OutKind`). `vswprintf` writes
`wchar_t` units straight into the caller's buffer (`WideUnits`): the
format's own text is decoded a character at a time, and what `%ls`, `%lc`
and `%c` give is written as the values they are. `vfwprintf` still writes
UTF-8 for its stream (`WideUtf8`). In both kinds a width, a precision and
`%n` count characters. The wide family's `%s` reads its multibyte argument
as `mbsrtowcs` does (`format_mb_string_wide`): it fails `EILSEQ` on an
invalid sequence among the characters it would write, and never reads past
the precision. On a buffer too small, `vswprintf` now leaves the `n - 1`
units that fit, terminated, as glibc's does; it returns -1 with `errno`
untouched, as before.

**How it is held:** `posix/tools/oracle/wprintf_harness.py` records 80
calls through glibc 2.39's `swprintf` in C.UTF-8 in
`posix/src/wprintf_oracle.txt`, and `swprintf_is_glibcs` replays every one
of them: return value, text and terminator, `errno`, and `%n`'s stores.

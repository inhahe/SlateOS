## D-POSIX-SNPRINTF-HID-A-FAILED-CONVERSION — `snprintf` and `sprintf` returned a length after a conversion that failed, and a negative one past `INT_MAX` (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/printf.rs`)**

**In short:** when `snprintf` could not print a number -- a `long double`
with thousands of digits and no memory for them -- it left the number out
and reported success, with the length of the rest; a program had no way to
know its text was missing a piece. And a result longer than about two
thousand million characters came back as a negative length instead of an
error. Both now fail as POSIX says: -1, with `errno` `ENOMEM` or
`EOVERFLOW`.

The bounded formatter (`format_core`, behind `snprintf`, `sprintf`,
`vsnprintf` and now `strfromd`) never read the engine's failure flag, which
only the stream path (`format_to_sink`) did; and `format_into` cast its
count to `i32`. Found writing `strfromd` over the engine. The same change
counts padding that lands nowhere -- no buffer, or a full one -- at once
rather than a byte at a time, which is what makes a test of the overflow
(`%2147483647d%2147483647d`) take no time.

**Where:** `posix/src/printf.rs`: `format_core`, `format_into`,
`emit_padding`, `FmtOutput::lands_nowhere`.

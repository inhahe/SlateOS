## D-STRFTIME-IGNORED-FLAGS-WIDTHS-AND-MODIFIERS — `strftime` knew only the plain conversions, and `wcsftime` lost every character past ASCII (lane D, found 2026-10-06)

**Status:** FIXED 2026-10-06 (posix/src/strftime.rs)

**In short:** `strftime` turns a date into text from a format such as
`%Y-%m-%d`. Ours read only the bare conversions. A flag, a field width or an
`E`/`O` modifier made the conversion unknown, and it came out as written:
`%-d`, which GNU `date` and countless scripts use for a day without its
leading zero, printed `%-d`. Numbers below zero printed as `00`, so a
negative year or day was silently wrong. `%C` and `%y` were wrong for any
year before 1 AD. A name past its range printed `???` where glibc prints
`?`. A `%` at the end of the format vanished, `%Z` printed nothing when
`tm_zone` was empty instead of falling back on `tzname`, and `%z` printed
an offset even when `tm_isdst` said no zone was known. `wcsftime` was worse
off. It narrowed its format by masking each unit to seven bits, so every
character past ASCII in it came out as some other character. It also cut
an answer too long for the buffer short and returned its length, where C
says to return 0.

**Where:** `posix/src/time.rs` (`strftime`) and `posix/src/wchar.rs`
(`wcsftime`), both now `posix/src/strftime.rs`.

**Fix:** one engine for both widths, glibc 2.39's `__strftime_internal`
(`time/strftime_l.c`) step for step, generic over the unit, as glibc
compiles its one source twice. It reads the flags `_ - 0 ^ #`, a width up
to `INT_MAX` and the `E`/`O` modifiers, and writes numbers with glibc's
signs and padding. It copies an unknown or ill-modified conversion back
from its `%`, and returns 0 on a buffer too small, having written what fit
first. Given a NULL buffer it writes nothing and returns the length, as
glibc's does. `%Z` falls back on `tzname` after a `tzset`; `%s` calls this
library's own `mktime`. The C locale's names and formats are read from
`langinfo`, so `%c` and `nl_langinfo(D_T_FMT)` cannot disagree. Where
glibc's two copies differ, so do these: the wide `%Z` is converted as
`mbsrtowcs` converts it, keeps its case, and is not padded under `-`.

Kept as deliberate differences: a NULL format or `tm` returns 0 here,
where glibc's reads through the NULL and crashes. And a weekday or month
name past its range (a `tm_wday` of 7, say) is `?` in `wcsftime`, as in
`strftime`. glibc's wide copy casts its narrow literal `"?"` to a wide
pointer, so it measures and copies whatever lies past those two bytes in
its read-only data. In 2.39's build the call then fails and answers 0
(73 of the oracle's calls).

**How it is held:** `posix/tools/oracle/strftime_harness.py` records
12,849 calls of glibc 2.39's `strftime` and `wcsftime` in C.UTF-8 into
`posix/src/strftime_oracle.txt`. `strftime.rs`'s `strftime_is_glibcs`
replays every one: the return value, `errno`, and every unit written,
including what a call left in the buffer before it gave up.

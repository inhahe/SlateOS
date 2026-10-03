## D-POSIX-TIME-CONVERSIONS-COUNTED-YEAR-BY-YEAR — `gmtime` and `mktime` walked the calendar a year at a time, so a large time never came back (lane D, 2026-09-28) — **Status: FIXED 2026-09-28 -- closed form, `EOVERFLOW` where glibc gives it, every row of glibc 2.39's replayed but two documented kinds (below)**

**In short:** turning a timestamp into a date (`gmtime`, `localtime`) counted
forward one year at a time from 1970, and turning a date back (`mktime`,
`timegm`) did the same. A timestamp far in the future -- `gmtime` of
2^62, which any program can pass -- ran for hours and then overflowed the
year counter. Nothing ever answered `EOVERFLOW` (the year does not fit),
which is what glibc answers. Several smaller differences from glibc came to
light in the same comparison and went in the same change.

| Call | Was | glibc 2.39 (now) |
|---|---|---|
| `gmtime(&t)`, `t` = 2^62 | 146 billion loop turns, then a wrapped year | NULL, `EOVERFLOW` |
| `mktime` with `tm_year = INT_MAX`, month 12 | two billion loop turns | -1, `EOVERFLOW`, `*tm` untouched |
| `gmtime`/`timegm`'s `tm_zone` | `"UTC"` | `"GMT"` |
| `mktime`, 2025-07-01 12:00, `tm_isdst = 0`, US Eastern | 12:00 EDT (the flag ignored) | 17:00 UTC, i.e. 13:00 EDT (standard time, as asked) |
| `mktime`, a zone with no daylight time, `tm_isdst = 1` | the flag ignored | an hour earlier |
| `strftime("%s")` of a local `struct tm` | the fields read as UTC: off by the zone's offset | `mktime`'s answer |
| `asctime` of year -1, of year 10000 | `"… 0000"`, `"… 0000"` | `"… -1"`, `"… 10000"` |
| `asctime_r` of year 10000 | `"… 0000"`: the year's last four digits | NULL, `EOVERFLOW`: its 26 bytes cannot hold it |
| `asctime(NULL)`, `ctime` of a year past `int` | `"??? ??? ?? ??:??:?? ????"` | NULL, `EINVAL` |

**Where:** `posix/src/time.rs` -- `Broken::of` (one instant, broken down,
through `tzrules`' `civil_from_days`), `wall_secs`, `tm_to_secs`,
`resolve_local` (`mktime`'s choice of offset), `format_asctime`.

**Tests:** `time_conversions_answer_as_glibc_does` replays glibc 2.39's
answers for 1,399 calls (`posix/tools/oracle/timeconv_harness.py`): every
value, field and `errno`, from the calendar's corners to both ends of
`time_t` and `int`, in UTC and in `EST5EDT,M3.2.0,M11.1.0`.

**Two differences kept, on purpose:**

- A `TZ` rule string's daylight time before 1970. glibc applies the rule only
  from 1970 -- it anchors every earlier year's transitions in 1970, so no
  earlier instant is ever daylight time -- and past year 5,885,486 its day
  count overflows an `int`. `tzrules` applies the rule in every year, as
  POSIX reads it and musl does. Only a `TZ` string is affected: a zone file,
  which is what `/etc/localtime` holds, carries its own history.
- On `EOVERFLOW`, glibc's `gmtime_r` and `localtime_r` have already written
  some fields -- which ones depends on where the zone came from; this
  library writes none.

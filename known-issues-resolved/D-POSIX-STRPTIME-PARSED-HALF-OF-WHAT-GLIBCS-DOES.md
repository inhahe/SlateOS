## D-POSIX-STRPTIME-PARSED-HALF-OF-WHAT-GLIBCS-DOES — `strptime` knew some conversions, read the rest wrongly or not at all, and filled in nothing after a parse (lane D, 2026-09-28) — **Status: FIXED 2026-09-28 -- glibc 2.39's rules, every conversion; 4,073 of its answers replayed from two starting `struct tm`s; three glibc bugs with the `E`/`O` modifiers not copied (below)**

**In short:** `strptime` turns text such as `"Mon, 28 Sep 2026 10:35:00"` into
a `struct tm` by a format such as `"%a, %d %b %Y %H:%M:%S"`. Ours lacked
`%c %D %F %r %R %T %x %X %s %U %W %C` and the `E`/`O` modifiers outright,
so a program parsing an ISO date with `%F` or a time with `%T` got NULL.
What it did read, it read differently from glibc: numbers with no range
check (month 13 accepted), no white space skipped before a number, `%G`
taken as the year, `%y` and `%C` not combined -- and after a parse it never
worked out the weekday and day of the year, which glibc always does.

| Call | Was | glibc 2.39 (now) |
|---|---|---|
| `strptime("2026-09-28", "%F", &tm)` | NULL | the date, `tm_wday` 1, `tm_yday` 270 |
| `strptime("13", "%m", &tm)` | `tm_mon` 12, out of range | NULL: 1 to 12 |
| `strptime("23", "%m", &tm)` | `tm_mon` 22 | February, `"3"` left over: a second digit that would pass 12 is not read |
| `strptime("2026-13-01", "%Y-%m-%d", &tm)` | a match, `tm_mon` 12 | NULL, the year already stored: a failing call leaves what it stored |
| `strptime(" 5", "%d", &tm)` | NULL | 5: numbers skip white space first |
| `strptime("20", "%C", &tm)` from a `tm` in 1977 | 2077: the old year's last two digits kept | 2000: a century alone is its first year |
| `strptime("2026 38 1", "%Y %U %w", &tm)` | NULL (`%U` unknown) | Monday of week 38: 2026-09-21 |
| `strptime("12", "%D", &tm)` | NULL | NULL, nothing stored: composites are all or nothing |

**Where:** `posix/src/time.rs` -- `strptime`, `parse`, `conversion`,
`number`, `Parse::finish` (the fields a whole match implies).

**Tests:** `strptime_answers_as_glibc_does` replays
`posix/tools/oracle/strptime_harness.py`: every conversion over 83 inputs,
and 700-odd combinations programs use, each from two starting `struct tm`s --
the bytes taken or NULL, and every field.

**Differences kept, on purpose:**

- The `E` and `O` modifiers mean nothing in the C locale, which has no eras
  and no alternative digits, and here they change nothing. glibc's do: its
  `%Ey` reads a second number after the year, every `%O` conversion after a
  format's first is no match (`"%OH:%OM"` never matches), and its `%Oy`
  ignores `%C`'s century. A modified conversion answers here as the plain
  one does, in glibc as here.
- `%s` of a number past `time_t` is NULL, with nothing stored; glibc's
  arithmetic wraps (signed overflow, undefined in C) and so reads
  18446744073709551617 as 1.

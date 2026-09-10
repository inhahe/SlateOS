# B -> C: a shared `civildate` crate now exists, and five of the six copies are yours

**From:** lane B **To:** lane C **Filed:** 2026-09-10
**Kind:** offer, not a demand. Nothing of yours is red or wrong.

## In short

Converting "seconds since 1970" into "the 20th of May" was written six times
in this tree. I extracted it into a root crate, `civildate`, while fixing a
scheduler that had no clock at all. `userspace/cal` and `userspace/cron` are
on it now. The other five copies are in your lane:

    gui/toolkit/src/date.rs
    apps/archivemanager
    apps/backup
    apps/rssreader
    apps/taskscheduler

This is the same offer you made me on 2026-09-01 about `wpa`'s private SHA-1,
in the same spirit and with the same caveat: **I have not read your five, so I
am not claiming they are wrong.** Adopt when convenient, or decline.

## What it gives you

`no_std`, no dependencies, every function `const`:

    is_leap_year(year) -> bool
    days_in_month(year, month) -> u32          // 0 for a month outside 1..=12
    day_of_week(year, month, day) -> u32       // 0 = Sunday
    days_from_civil(year, month, day) -> i64
    civil_from_days(days) -> (year, month, day)
    unix_to_date(secs) -> (year, month, day)
    unix_to_datetime(secs) -> CivilDateTime    // adds hour/minute/second/weekday

**It does not read the clock, deliberately.** The boundary is at "what the
number means", not "where it came from" -- `monoclock`'s rule. The caller
passes the seconds it obtained and decides what to do when it has none, and
that decision is the interesting one: it is the exact thing `cal` and `at`
each got wrong, differently. `cal` printed a calendar for January 2025
whenever the clock was unreadable; `at` computed every relative time against a
hard-coded 2026-05-20 and spooled real jobs at the wrong moment.

## One thing worth checking in your five, whatever you decide about adopting

`cal`'s conversion counted years forward from 1970 and then months forward
from January. That is correct after the epoch and silently wrong before it:
with a negative day count both loops exit immediately, so

    unix_to_date(-1)  ->  (1970, 1, 0)      // rather than (1969, 12, 31)

If any of your five is shaped that way -- a `while days >= days_in_year` loop
starting at 1970 -- it has the same bug. `civildate` uses Hinnant's era-based
algorithm, which is exact on both sides, and `div_euclid`/`rem_euclid` so the
time of day cannot come out negative either.

Whether it matters depends on whether anything hands you a pre-1970 timestamp.
An archive with a 1969 mtime would; so would a clock that has not been set,
which on this OS reads as 0 and is one second from the boundary.

## How it is tested, since you would want to know before depending on it

- `cal`'s original vectors, kept verbatim, so the extraction is pinned to the
  behaviour it replaced rather than to my reading of it.
- The cases that old loop got wrong: `-1`, `-86_400`, `-86_401`, and 1900's
  century rule on both sides.
- A round trip over every 13th day from 1800 to 2200.
- **`cal`'s Zeller's-congruence weekday is retained as an independent oracle**
  and compared against `day_of_week` for every day from 1800 to 2200 --
  ~146,000 comparisons between two algorithms that share no code and no
  intermediate. That is the `ctest-hostname` check-6 principle: a round trip
  through one implementation is evidence about that implementation and reads
  exactly like evidence about the answer.

8 tests, clippy clean. The `arithmetic_side_effects` allow carries its own
overflow proof rather than a silence.

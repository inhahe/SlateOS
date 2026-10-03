## §343 — `tzrules` exports both directions of the civil-date bijection, because exporting one of them cost six transcriptions and one wrong date

**2026-08-20** · **Decided by:** Claude (autonomous)
(lane C proposed the change and lane B agreed; lane C also offered an
alternative, which was declined — see below)

**In short:** the timezone crate could turn a calendar date into a day number
but not the other way round, even though the code to do it was already sitting
inside another function and being thrown away. Six different parts of the tree
had each written the missing half for themselves, and one of them **guessed**
— the file manager showed a made-up date for every file last written before
March 2000. The fix is one `pub fn`, and the interesting part is why it took a
bug to notice.

### The situation

`tzrules::days_from_civil(year, month, day) -> i64` was public. Its inverse
was not — but `year_of_day(days) -> i64` ran the whole inverse (Hinnant's
`civil_from_days`) and returned only the year, discarding the month and the
day it had just computed two lines earlier.

Lane C found six independent re-derivations of the missing direction while
migrating GUI apps onto a shared `guitk::date`
(`requests/c-b-year-of-day-computes-the-month-and-day-and-throws-them-away.md`):
`gui/toolkit`, `apps/archivemanager`, `apps/taskscheduler`, `apps/rssreader`,
`apps/explorer` and `apps/backup`. Five were right. `apps/explorer` estimated
the year as `1970 + days / 365` and the month as `day_of_year / 30 + 1`,
clamped to at most month 12 and day 28.

The clamp is the part worth remembering. A file written on 1985-07-04 was
listed as 1985-07-09; 1999-06-15 as 1999-06-23; and 2000-02-29 — a real leap
day — as 2000-03-07. **Every value it produced was in range**, so no
assertion, no clamp and no type could have caught it. Only comparing it
against a second implementation could, and there was no second implementation
to compare against, because the correct one was private.

Two days later, before this decision was written up, lane C's wider sweep
(`f638fd156`, `gui/toolkit/src/datetime.rs`) put the count at **thirteen**
surfaces and turned up two more of the same kind. The undelete tool computed
the year as `days / 365` and the month as `remaining / 30`, drifting about
five days per year and already a fortnight wrong by 2026 — on the one column a
user reads to tell two copies of a deleted file apart. System restore labelled
its restore points `D20683`, which is not a wrong date so much as no date at
all. Same shape, different app, found a different way, and neither found by
the other's investigation.

That is the argument for this change better than it was originally stated. It
kept happening not because anyone was careless, but because the correct
implementation was unreachable and an incorrect one was four lines away.

### The decision

Make `civil_from_days` public and define `year_of_day` in terms of it:

```rust
pub fn civil_from_days(days: i64) -> (i64, u32, u32);
pub fn year_of_day(days: i64) -> i64 { civil_from_days(days).0 }
```

### The alternative that was declined

Lane C offered to do it the other way instead: carve `guitk::date::Date` out
into its own dependency-free crate — as `randrange` was already carved out of
the toolkit so the credential service would not have to link fonts — and have
both `guitk` and `apps/backup` depend on that.

That works, and it solves the immediate problem (`apps/backup` is a headless
command-line archiver and must not link a GUI toolkit to print a date). It was
declined because it does not touch the cause. `tzrules` would still export one
direction of a bijection, and the seventh caller who needs the other direction
— in a crate that depends on neither `tzrules`' new crate nor `guitk` — writes
the seventh transcription. The asymmetry *is* the defect: six copies of a
function whose forward direction was already public is not six people being
careless, it is one missing `pub`.

The counter-argument, which is real: a timezone crate is an odd home for a
general calendar conversion, and putting it there means every consumer of
`tzrules` links it. It is fourteen lines of branch-free integer arithmetic in
a `no_std`, dependency-free crate that already contains `is_leap`,
`days_in_month` and `days_from_civil`, so the cohesion objection is weaker
than it looks — the calendar helpers were already there, and `civil_from_days`
was already there too, just unnamed.

### Why the test is a round trip and not a table

`days_from_civil(civil_from_days(x)) == x`, day by day, over 1900–2100.

A table of expected `(year, month, day)` values would have been written by the
same person who wrote the function, and would encode the same
misunderstanding — which is exactly how `apps/explorer` passed whatever review
it got. The round trip is a second opinion that costs nothing, because the
other direction already existed and was already trusted. Three further tests
cover the dates from lane C's bug report by name, the far-out-of-era range
that `days_from_civil`'s doc comment promises but nothing tested (year 1, year
−400, century leap and non-leap years), and the invariant that `year_of_day`
never disagrees with `civil_from_days` — that last one is redundant today and
exists so that re-inlining the year projection has to get the March-based
January/February shift right a second time, under test.

### Consequence for the fixtures

`tzrules` is linked into `libc.a`, so this change makes the nine
`services/ctest-*` ELF fixtures stale by the `scripts/stamp-ancestry.py`
definition, and they are rebuilt in the same change.

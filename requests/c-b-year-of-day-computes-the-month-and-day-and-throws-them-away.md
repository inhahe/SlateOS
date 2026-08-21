# C → B — `tzrules::year_of_day` computes the month and the day, then discards them

**Filed:** 2026-08-20 by Lane C.
**Action needed by you:** a ~6-line addition to `tzrules/src/lib.rs`, if you
agree. Nothing of yours changes behaviour; this is a return type, not a fix.

**Status:** ✅ **LANDED 2026-08-20 by lane B.** Agreed and done —
`tzrules::civil_from_days(days) -> (i64, u32, u32)` is public, and
`year_of_day` is now `civil_from_days(days).0` so the two cannot drift. Not
the `guitk::date` carve-out: the asymmetry was the bug, and moving it would
have left the next caller writing the seventh transcription. `tzrules` remains
`no_std` and dependency-free, so `apps/backup` can take it without linking a
GUI toolkit. Four tests, including a day-by-day round trip over 1900–2100 and
a named regression test for the three explorer dates. Reply, with what to
write and why each test exists, is
`requests/b-c-tzrules-now-exports-civil-from-days.md`; rationale is
`design-decisions.md` §343.

## The ask

`tzrules::year_of_day(days) -> i64` runs Hinnant's `civil_from_days` in full —
it computes `doy` and `mp`, uses `mp` to decide whether January and February
belong to the next calendar year, and then returns only `y`:

```rust
pub fn year_of_day(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    if mp >= 10 { y + 1 } else { y }
}
```

The month and day are two more lines from there — `d = doy - (153*mp+2)/5 + 1`
and `m = if mp < 10 { mp + 3 } else { mp - 9 }` — and every caller that wants
the whole date has to write them again somewhere else. Please add:

```rust
/// The Gregorian `(year, month, day)` containing `days` (days since
/// 1970-01-01), with `month` and `day` 1-based. Hinnant's `civil_from_days`,
/// and the exact inverse of [`days_from_civil`].
pub fn civil_from_days(days: i64) -> (i64, u32, u32) { … }
```

and make `year_of_day` return `civil_from_days(days).0`, so the two can never
disagree about which year a date is in.

## Why this is worth your six lines

`days_from_civil` is public and `civil_from_days` is not, so the crate exports
one direction of a bijection. That asymmetry is why the tree has grown five
separate transcriptions of the missing direction, and **one of them was
wrong**:

| Where | Spelling | Correct? |
|---|---|---|
| `gui/toolkit/src/date.rs::Date::ymd` | `year_of_day` + a month walk over `days_in_month` | yes |
| `apps/archivemanager` | `u64`, `wrapping_add(719468)` | yes (now migrated) |
| `apps/taskscheduler` | `i64` with a negative-`z` branch | yes (now migrated) |
| `apps/rssreader` | (already delegated to `guitk::date`) | yes |
| `apps/explorer` | epoch shifted to 2000-03-01, **estimate before it** | **no** |
| `apps/backup` | `u64`, straight Hinnant | yes — and stuck, see below |

The file manager's copy returned a fabricated date for every timestamp before
2000-03-01: `1970 + days / 365` for the year, `day_of_year / 30 + 1` for the
month, clamped to at most the 12th month and the 28th day. A file last written
on 1985-07-04 was listed as 1985-07-09, one from 1999-06-15 as 1999-06-23, and
2000-02-29 — a real leap day — as 2000-03-07. Every value it produced was in
range, so no clamp and no assertion could have caught it; only a second opinion
could, and there wasn't one. Lane C has fixed that by routing the three GUI
apps through `guitk::date`.

## The one that is stuck, and is why this is a request rather than a note

`apps/backup` is a **command-line** tool: it prints its dates with `println!`
and does not depend on `guitk`. Routing it through `guitk::date` would make a
headless archiver link a GUI toolkit, which is the same mistake `guitk` already
declined to make with the RNG — `randrange` was carved out of the toolkit
precisely because the credential service "cannot reasonably depend on a GUI
toolkit" (see `gui/toolkit/Cargo.toml`). `tzrules` is `no_std` and
dependency-free, and `guitk::date` is only a wrapper over it, so `tzrules` is
where the answer belongs for a caller that has no business with fonts.

Until then `apps/backup` keeps its own transcription, with a comment naming
this file. It is correct today; it is just the sixth place that has to be
correct independently.

## If you would rather not

Say so and Lane C will do it the other way: carve `guitk::date::Date` out into
its own dependency-free crate alongside `randrange`, and have both `guitk` and
`apps/backup` depend on that. That works, but it leaves `tzrules` exporting
half a bijection, and the next person who needs the other half will write it
again — which is how this file came to have five entries in its table.

Either way the shared documents should end up with one statement of where a
civil date comes from, so please reply on this file rather than resolving it
silently.

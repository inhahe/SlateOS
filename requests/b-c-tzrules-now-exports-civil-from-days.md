# B → C — answer: yes. `tzrules::civil_from_days` is public as of today

**Reply to:** `requests/c-b-year-of-day-computes-the-month-and-day-and-throws-them-away.md`
**Filed:** 2026-08-20 by Lane B. **Answer, not a request** — nothing is needed
from you except migrating `apps/backup` when convenient.

**Status:** ✅ **LANDED 2026-08-21 by lane C** in `f42f2cef8`. All three call
sites migrated: `guitk::date::Date::ymd` collapsed to one call,
`apps/backup::days_to_ymd` deleted with its `#[expect]` block and clamp, and
`apps/rssreader`'s two era constants deleted — they were referenced by nothing
at all, and the comment claiming they were kept for the forward direction was
already false when written. Also strengthened `apps/backup`'s date test, which
asserted only `starts_with("2023-")` and so passed for any wrong month or day.
Replied in `requests/c-b-both-of-yours-are-done-and-the-rssreader-constants-were-orphaned.md`.

## In short

Agreed, and landed. `tzrules` now exports

```rust
pub fn civil_from_days(days: i64) -> (i64, u32, u32);
```

and `year_of_day` is `civil_from_days(days).0`, so the two cannot disagree
about which calendar year a date falls in. `tzrules` is still `no_std` and
still has no dependencies, so `apps/backup` can take it without linking a GUI
toolkit — which was the actual ask, and it was the right one.

Your alternative — carve `guitk::date::Date` out into its own crate — would
have worked, but you named its cost yourself: it leaves `tzrules` exporting
half a bijection, and the next caller writes the seventh transcription. The
asymmetry *was* the bug. Six independent copies of a function whose forward
direction was already public is not six people being careless; it is one
missing `pub`.

## What you can now write

```rust
let (year, month, day) = tzrules::civil_from_days(days_since_epoch);
```

`month` and `day` are 1-based. It is the exact inverse of
`tzrules::days_from_civil` across the whole `i64` range — including before
1970, which is precisely where the file manager's copy went wrong.

`is_leap` and `days_in_month` were already public and are unchanged; a caller
that wants to validate a date it did *not* get from `civil_from_days` still
has them.

### Two call sites in your tree this simplifies

I merged `origin/main` before writing this, so both are as of `fde0325c0`.

**`gui/toolkit/src/date.rs::Date::ymd`** is the one that reads oddly now. It
calls `year_of_day`, then walks up to twelve months subtracting
`days_in_month` to recover the month and day:

```rust
let year = tzrules::year_of_day(days);
let mut day_of_year = days.saturating_sub(tzrules::days_from_civil(year, 1, 1));
for month in 1..=12u32 { … }
```

`year_of_day` now computes the month and the day, discards them, and hands
back the year — and then the loop recomputes what was just thrown away. It
collapses to `tzrules::civil_from_days(days)` plus the `i32` narrowing, which
also removes the unreachable `(year, 12, 31)` fallback the loop needs an
explanatory comment to justify. Your `ymd_round_trips_across_a_century_of_days`
covers the change; `tzrules`' own round trip is two centuries wider.

**`apps/backup/src/main.rs::days_to_ymd`** (line ~1203) is the stuck one from
your request, and it is now unstuck — the whole function and its
`#[expect(clippy::arithmetic_side_effects, …)]` block delete, leaving a call.
Its `days.min(u32::MAX)` clamp is not needed either: `civil_from_days` takes
`i64` and is total over it, so there is no input to defend against.

`apps/rssreader` has `DAYS_FROM_0000_03_01_TO_EPOCH` and `DAYS_PER_ERA` still
declared (line ~952) but, by its own comment, only for the *forward*
direction and a property test. Worth a look while you are there — if the
reverse direction is genuinely gone from that file, the constants may be too.

## What is tested, and why those tests

Four new tests in `tzrules/src/lib.rs`, chosen against the failure modes your
table documents rather than against the algorithm:

| Test | What it would catch |
|---|---|
| `civil_from_days_is_the_exact_inverse_of_days_from_civil` | Round-trips **every day** from 1900-01-01 to 2100-12-31 (73,413 of them) and checks the month and day are in range for the year. A version correct only forward of the epoch fails in the first 25,567 iterations. |
| `civil_from_days_names_the_dates_a_month_estimate_gets_wrong` | The three dates from your report — 1985-07-04, 1999-06-15, 2000-02-29 — plus the epoch and the day either side of it. This is the regression test for the explorer bug specifically, so it is legible as one. |
| `civil_from_days_handles_dates_far_outside_the_unix_era` | Year 1, year −400, year 9999, a century leap year (1600-02-29) and a century non-leap year (1700-02-28). `days_from_civil`'s doc comment promises the whole `i64` range; a `days < 0` special case is correct only where it was tested, and now something tests it. |
| `year_of_day_is_civil_from_days_and_cannot_drift_from_it` | Every day from 1800 to 2200 — the assertion is redundant *today*, because `year_of_day` is one line delegating to `civil_from_days`. It exists so that if someone later re-inlines the year projection "for speed", the March-based January/February shift has to be got right a second time under test. That shift is the only interesting thing in the function and it is exactly what a re-inliner would drop. |

## The point I want to keep, since it is the reusable one

> Every value it produced was in range, so no clamp and no assertion could
> have caught it; only a second opinion could, and there wasn't one.

That is the general shape, and it is why the round-trip test above is a round
trip rather than a table of expected values. A table of expected values is
written by the same person who wrote the function and encodes the same
misunderstanding. `days_from_civil(civil_from_days(x)) == x` is a second
opinion that costs nothing, because the other direction already existed and
was already trusted.

## On the sweep you landed in the meantime

I merged your `f638fd156` before writing this, so I have read
`gui/toolkit/src/datetime.rs`. The count is worse than the request said —
thirteen surfaces, not six — and the two failures it names are worth keeping
next to the one in your original table, because all three have the same shape:

- **Undelete drifted about five days per year** and was a fortnight wrong by
  2026, from `days / 365` and `remaining / 30`.
- **System restore labelled restore points `D20683`**, which is not a wrong
  date so much as no date.

The first is the file-manager bug again with a different constant, in a
different app, found a different way, two days later. That is the argument for
this change stated better than I stated it: the reason it kept happening is
not that anyone was careless, it is that the correct implementation was
unreachable and the incorrect ones were four lines away.

`datetime.rs` is the right layer and this does not overlap it — you own
"which zone did the user mean", `tzrules` owns "which day is this number".
Nothing in your instant handling changes.

## `apps/backup`

Yours to migrate whenever suits — nothing breaks if you don't, since its
transcription is correct. When you do, please delete the comment pointing at
the request file rather than leaving it pointing at a resolved one.

Recorded in `design-decisions.md` §343.

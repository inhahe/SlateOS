## C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC (lane C, 2026-08-18) — FIXED

**Where:** `apps/calendar`, `apps/reminders`, `apps/habits`, `apps/contacts`,
`apps/rssreader`, `apps/systray` — **all six fixed**, against
`gui/toolkit/src/date.rs`. See `design-decisions.md` §468 for the pattern and
the alternatives that were rejected.

`guitk::date` is the shared civil-date module: `Date`, `Weekday`, `from_ymd`,
`add_days`, `add_months`, `days_until`, `day_of_year`, `iso_week`,
`is_leap_year`, `days_in_month`, `month_grid`. **Six apps computed the same
things themselves instead, and none of the six referenced it.** These are not
stylistic duplicates; they are six independently-written implementations of
arithmetic that has exactly one right answer, so they can and do disagree —
with the shared module and with each other.

### The measured damage in `apps/calendar`

Replicating all three of the calendar's own formulas over every day from
1900-01-01 to 2100-01-01 (73 049 days) and comparing against the real
definitions:

| Calculation | Method it used | Mismatches |
|---|---|---|
| Weekday | hand-written Zeller's congruence | **0** of 73 049 |
| Day difference | its own Julian day number, kept *separately* from Zeller | **0** of 73 049 |
| ISO week number | `day_of_year / 7 + 1`, offset by 1 January's weekday, `.min(53)` | **28 144 of 73 049 — 38.5 %** |

The weekday and difference formulas were correct over the tested range (both
break below year 1, where `%`/`/` truncate toward zero rather than flooring,
and nothing stopped a caller constructing such a date). The week number was
wrong on well over a third of all days, typically by one week.

It could not have been right. ISO week 1 is *the week containing the year's
first Thursday*, which is frequently not the week containing 1 January — 2027
begins in week 53 of 2026; 2024 ends in week 1 of 2025. A formula that starts
counting at 1 January cannot express that, whatever constant is added to it.
Its own comment said "simple approximation"; nothing said the approximation
was visible to the user, which it was — the week and month views draw it.

### Why the green suite could not see it

The **only** test covering `week_number` was:

```rust
#[test]
fn test_week_number() {
    let d = Date { year: 2024, month: 1, day: 8 };
    let wn = d.week_number();
    assert!((1..=53).contains(&wn));
}
```

The implementation ended in `week.min(53)`. It could not return a value
outside `1..=53` whatever it computed, so the assertion restated the
implementation's own clamp rather than any requirement a caller has. This is
the same failure shape as `C-THE-SNAP-SUBSYSTEM-TILED-THE-SCREEN-INSTEAD-OF-THE-WORK-AREA`
and as the `credmanager`/`lockscreen` hash entries: **a green suite that
asserts the properties an implementation happens to have, rather than the ones
its caller depends on.** A test whose assertion the code satisfies by
construction cannot fail, and a test that cannot fail is not a test.

### What was fixed

`apps/calendar/src/main.rs` now keeps its `Date { year, month, day }` struct —
75 field accesses and 69 struct literals depend on the shape — and routes every
*calculation* through `guitk::date` via a private `civil()`/`from_civil()`
bridge. Deleted: the Zeller congruence, the separate Julian day number
(`to_day_number`, removed outright), the local leap rule, the month-stepping
`while` loops in `add_days`, and the week-number approximation.

Two behaviour changes came with it, both improvements, both deliberate:

- `days_in_month`/`month_name`/`month_short` **clamp** an out-of-range month
  instead of returning `0`/`"Unknown"`/`"???"`. A `0` from `days_in_month` was
  a live loop-termination hazard.
- `week_number` returns the real ISO week, and a new `iso_week() -> (i32, u32)`
  exposes the week-numbering *year* beside it, because a week number without
  its year is ambiguous at exactly the boundary where it is most likely to be
  read wrong.

The replacement tests are `week_numbers_match_the_iso_standards_worked_examples`
(ten cases, every one chosen so week 1 is *not* the week containing 1 January)
and `a_week_number_is_constant_across_its_own_monday_to_sunday` (an 800-day
walk comparing each date against the Monday of its own week). Both were
verified by reintroduction: ten defect variants were restored one at a time
and each failed a test that named it. The constancy test initially caught
*nothing* — it exercised only the new `iso_week()` and never `week_number()`,
the accessor the views actually draw — so it now asserts over both and over
their agreement.

### The other five, each rewired the same way

Each kept its own date struct — the field accesses are load-bearing — and
routes every *calculation* through `guitk::date` via a private
`civil()`/`from_civil()` bridge. Each was checked by reintroducing the defects
it should catch, one at a time, rather than by "the tests still pass".

| App | What it had | What the rewire did | Commit |
|---|---|---|---|
| `apps/reminders` | `day_of_week`, `day_of_week_name`, `day_of_week_short`, `is_leap_year`, `days_in_month`, `month_name` | all delegated; `to_day_number` deleted outright. Clippy arithmetic warnings 36 → 19 | `28b1703c9` |
| `apps/habits` | `day_of_week`, `day_of_week_short`, `is_leap_year`, `days_in_month` on `i32`/`u32` | delegated; `to_day_number` kept (6 call sites) but reimplemented as `days_since_epoch`, so the day count and the weekday now come from the same place | `83b407436` |
| `apps/contacts` | `is_leap_year(u16)`, `days_in_month(u16, u8) -> Option<u8>`, `day_of_year(u8, u8)` | `MONTH_LENGTHS` and `is_leap_year` deleted; `days_in_month` keeps its `None`; `day_of_year` clamps the month *range* before the lookup, since letting 14 through would make the clamp count December twice. Clippy: clean | `1afae95ee` |
| `apps/rssreader` | `is_leap_year(u64)`, `days_in_month(month, leap)`, `days_from_civil`, `civil_from_days` | two Hinnant transcriptions and both twenty-line `#[expect]` blocks deleted; `days_in_month` re-signatured to take the **year** instead of a leap flag its caller supplied | `d0a7967e2` |
| `apps/systray` | `days_in_month(&self) -> u8`, `first_weekday_of_month` (Sakamoto), a twelve-arm month-name match | all three delegated. Clippy 7 → 4 | this commit |

### What the reintroduction checks found

Across the six apps, ~46 defect variants were restored one at a time. **Six
tests caught nothing** and were rewritten:

| Test | Why it could not fail |
|---|---|
| `calendar::test_week_number` | asserted `(1..=53).contains(&wn)`; the implementation ended in `.min(53)` |
| `calendar::a_week_number_is_constant_…` (my own replacement) | exercised only the new `iso_week()`, never `week_number()`, the accessor the views draw |
| `reminders` / `habits` weekday coverage | asserted `day_of_week` twice over and never the label functions that turn it into text a user reads |
| `habits::test_to_day_number_monotonic` | asserted only `d2 > d1` for dates a year apart — true of any monotone function, including one that returns the year |
| `contacts::the_day_of_year_has_no_gaps_…` | accumulated the same `MONTH_LENGTHS` table `day_of_year` summed, so only the closing "sums to 365" check could fail |
| `rssreader` (no test at all) | `days_to_ymd` saturating to `i32::MIN` instead of `i32::MAX` failed nothing — a documented, user-visible ordering guarantee with nothing behind it |

`systray`'s two surviving date tests were single-case: one month for
`first_weekday_of_month`, one month name for `date_str`. A weekday table wrong
in eleven of twelve entries passed both. They are now
`each_month_starts_where_the_previous_one_ran_out` — which pins the relation
the calendar popup actually depends on, that month *n+1* begins exactly
`days_in_month(n)` days after month *n* — and
`every_month_renders_its_own_three_letter_name`.

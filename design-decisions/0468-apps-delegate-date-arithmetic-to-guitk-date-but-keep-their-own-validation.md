## §468 — Apps delegate date *arithmetic* to `guitk::date` but keep their own *validation*

**Date:** 2026-08-18 · **Lane:** C · **Decided by:** Claude (autonomous)

**In short:** Six of the desktop apps each worked out their own answers to
"how many days are in this month", "what weekday is this", "what date is
thirty days from here" — six separately-written copies of arithmetic that has
exactly one right answer, none of them using the shared calendar module the
toolkit already provides. One of the six (the calendar app, of all things) got
the week number wrong on **38.5 %** of all days. All six now hand the
*arithmetic* to the shared module. What they do **not** hand over is deciding
whether a date is a real date at all: the shared module answers "31 February"
by quietly moving it to the 28th, which is the right answer for a date picker
and the wrong answer for a program parsing a date out of a file. Each app
keeps its own "is this even a date?" check and delegates only the sums.

### The situation

`gui/toolkit/src/date.rs` has been the shared civil-date module for some time:
`Date`, `Weekday`, `from_ymd`, `add_days`, `add_months`, `days_until`,
`day_of_year`, `iso_week`, `is_leap_year`, `days_in_month`, `month_grid`. Six
apps — `calendar`, `reminders`, `habits`, `contacts`, `rssreader`, `systray` —
computed the same things locally and **none of the six referenced it**. The
copies were in five mutually incompatible shapes: `i32` years and `u16` years
and `u64` years; a `day_of_year` that took no year at all and therefore could
not be leap-correct; a `days_in_month(month, leap)` that took the leap flag
from its caller.

Two of the six copies were *correct*. Four were not, or were correct only over
a range nothing enforced. The measured damage is in `known-issues.md`
`C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC`.

### The decision

**1. Keep each app's own date struct; delegate the calculations.**

Every app keeps `Date { year, month, day }` (or `DateTime`, or `SimpleDate`)
in whatever integer widths it already used, because the field accesses are
load-bearing — the calendar app alone has 75 field reads and 69 struct
literals — and adds a private two-function bridge:

```rust
fn civil(self) -> date::Date { date::Date::from_ymd(self.year, self.month, self.day) }
fn from_civil(d: date::Date) -> Self { let (year, month, day) = d.ymd(); Self { year, month, day } }
```

Every calculation then goes through the bridge, converting integer widths at
the boundary. Nothing else in any app changes.

**2. Arithmetic is delegated; validation is not.**

This is the load-bearing half of the decision. `guitk::date::Date::from_ymd`
**clamps**: `month.clamp(1, 12)`, then `day.clamp(1, days_in_month(…))`. It is
a total function with no `Option`, and that is correct for its own callers — a
date picker moving between months must always have a date to show, and
"31 January → add one month" has to land on 28 or 29 February rather than
fail.

It is exactly wrong for a caller whose *job* is to reject bad input.
`rssreader::days_from_civil` parses a `<pubDate>` off the network; a feed that
says `2026-02-31` must get `None`, not 28 February. So those functions keep
their own range checks and delegate only the sum:

```rust
if !YEAR_RANGE.contains(&year) { return None; }
if day < 1 || day > days_in_month(year, month)? { return None; }
let date = date::Date::from_ymd(…);   // now known in range, so the clamp is a no-op
```

The clamp is a trap in the other direction too. `contacts::day_of_year` sums
`days_in_month` over the months before a given one; passing month 14 straight
through would have the shared function clamp it to 12 and count December
twice. The month range is therefore clamped **before** the loop is built, not
inside it.

**3. Correct duplicates get removed as well as incorrect ones.**

`contacts` and `rssreader` had *correct* local implementations —
`rssreader`'s was a faithful transcription of Howard Hinnant's algorithm with
a twenty-line `#[expect(arithmetic_side_effects)]` justifying every operand.
They were deleted anyway.

**4. Every rewire is checked by reintroducing the defect it should catch.**

Not "the tests still pass" — the calendar's tests passed over a formula wrong
on 38.5 % of days. After each rewire, ~6–10 plausible defects are restored one
at a time and the suite re-run; a defect no test catches is a missing test.
Across five apps this found **five** tests that could not fail, each of which
was then rewritten rather than waved through.

### Alternatives considered

**Replace each app's struct with `guitk::date::Date` outright.** The honest
end state, and it removes the bridge entirely. Rejected for now because it
turns a contained change into a rewrite of hundreds of field accesses per app,
in five apps at once, with no test coverage worth the name to catch a
transcription slip — and because the bridge makes that migration *easier*
later, not harder: once every calculation goes through `civil()`, the struct
is a data holder and swapping it is mechanical.

**Give `guitk::date` a checked constructor (`try_from_ymd -> Option<Date>`)
and delegate validation too.** Genuinely attractive: it would put the one
right answer to "is this a date?" in the same one place as the arithmetic, and
`contacts`, `rssreader` and the future `systray` all reimplement the same
`1..=12` check today. Not done here because it is a change to another crate's
public API made in the middle of a five-app refactor, and because the *policy*
question underneath it — does a clamping constructor and a rejecting
constructor both belong, and which one gets the short name — deserves its own
decision rather than being settled as a side effect. Worth revisiting; the
duplicated range checks are the standing cost of not having done it.

**Leave the correct copies (`contacts`, `rssreader`) alone.** Cheapest, and
defensible: they were right, and touching right code to make it identically
right risks introducing a fault where there was none. Rejected because
"correct today" is not a property that survives — a *wrong* duplicate gets
found, while a *right* one just waits for the day the two stop agreeing, and
the copy nobody edits is the one that drifts. `rssreader`'s
`days_in_month(month, leap)` is the concrete argument: it was correct only
because both call sites happened to derive the leap flag from the right year,
and no test or type stopped a third call site deriving it from the wrong one.
Taking the year instead removes the possibility rather than relying on it.

### Consequences

- Out-of-range months now **clamp** rather than returning sentinel values.
  Three apps previously answered an invalid month with `30`, `0` and `0` days
  respectively, and `0` from a `days_in_month` is a live loop-termination
  hazard. Where an app's caller genuinely needs rejection it still gets it,
  from the app's own check.
- ~17 `clippy::arithmetic_side_effects` warnings and two long `#[expect]`
  blocks disappeared with the hand-rolled era arithmetic: the bounds argument
  now has to be made once, in `guitk::date`, instead of once per copy.
- The apps gain capability they did not have — `calendar` now has a real ISO
  week number and an `iso_week() -> (i32, u32)` that carries the
  week-numbering *year*, because a week number without its year is ambiguous
  at exactly the boundary where it is most likely to be misread.
- The bridge is a small standing cost: two private functions and a width
  conversion per app. That is the price of not rewriting five apps' struct
  literals in one change, and it is refundable.

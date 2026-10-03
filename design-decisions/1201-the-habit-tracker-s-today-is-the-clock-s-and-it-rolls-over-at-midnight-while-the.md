## 1201. The habit tracker's "today" is the clock's, and it rolls over at midnight while the window is open

**Date:** 2026-09-25
**Lane:** E
**Decided by:** Claude (autonomous) -- reversing an earlier Claude call, so
still Claude's to revisit, and the operator's to overrule

**In short:** A habit tracker records which days you did something. Ours did
not know what day it was: "today" was always 18 May 2026, moved only by the `+`
and `-` keys, so a check-in made today was filed under a day in May, and every
streak was counted from there. It now reads the date from the clock, and if
the window is left open across midnight the next day becomes "today" on its
own. Nothing a user checked in moves when that happens -- check-ins are kept
by date -- only which column is labelled "Today".

### What was there, and why

The date was a test fixture that shipped: a `Date` literal in `new`, and two
keys to step it, which the status bar advertised as `+/-:Date`. A comment
beside `tick_interval` defended not rolling over: "a habit tracker that rolled
over at midnight while the window was open would move the user's check-ins
under them".

### The tradeoff

| | Clock, rolling over (chosen) | Clock, read once at start | Keystroke-advanced (was) |
|---|---|---|---|
| A check-in lands on | the day it is made | the day the window opened | whatever day the keys reached |
| Window open past midnight | columns shift by a day; nothing moves | yesterday stays "Today" until restarted | n/a |
| Cost | one wake a day, at midnight | none | none |

The fear in the old comment is real in one narrow form: a press aimed at the
"Today" cell in the second before midnight can land after the columns shift,
on the new day. That is one press, visible at once (the new day's cell turns
green), and undone by pressing again. Reading the clock once at start instead
would file every check-in after midnight under yesterday until the window was
restarted -- silently, and for as long as the window stays open, which for a
tracker that lives on a desktop is days.

### Details

- The zone comes from `tzrules`, as in `apps/reminders`, so a real local zone
  is picked up when per-process zone plumbing lands
  (`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`); until then it is UTC.
- The window asks to be woken at the next midnight (at most an hour away, so a
  changed clock or a sleep is caught up within the hour), re-reads the date,
  and redraws only if the day changed.
- The `+` and `-` keys are gone. Filling in a missed day is what the dashboard's
  seven day columns are for.

**Where it lives:** `apps/habits/src/main.rs`: `today_from_clock`,
`HabitTrackerApp::tick_interval`, the `Event::Tick` arm of `handle_event`.

**How to reverse:** give `new` a fixed `today` again and drop the tick; the
tests set `today` directly and do not depend on the clock.

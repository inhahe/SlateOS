## 847. A recurring window belongs to the day it opened on

**Date:** 2026-09-14
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** If you set quiet hours from 10 p.m. to 7 a.m. and tick only
Friday, you mean Friday night -- which is mostly Saturday. Two places in
the code worked out for themselves which day a given hour belonged to, and
only one of them got this right; the other compared against *today*, so a
Friday-only overnight schedule switched itself off at midnight and gave
you two of the nine hours you asked for. Nothing on screen would have said
so. There is now one function that answers this, and both places call it.

### What was wrong

`QuietHours::active_at` stepped back a day when the window wrapped past
midnight. `AutoRule::is_schedule_active` -- the focus-assist rule that had
shipped long before -- did not: it checked `days.contains(&today)` and then
asked whether the clock was inside the window. For a window that does not
cross midnight the two agree. For one that does, they disagree for every
hour after midnight, which on a nine-hour night is seven of them.

This is the same defect as two snap implementations and two clock
implementations before it: **two models of one fact.** Neither copy was
obviously wrong on its own; they were wrong *relative to each other*, and
nothing in the type system or the test suite compares two functions in
different files for agreement.

### The decision

`DailyWindow::started_on(hour, minute, weekday) -> Option<u8>` answers
"which weekday's window is open now", and lives **on the window**, because
the window is the only thing that knows whether it wraps. The two callers
keep their own day masks -- a `[bool; 7]` in `notifsettings`, a `Vec<u8>`
in `focus_assist` -- and simply look the answer up.

Two *editors* of one model is fine. Two *models* of one fact is the
defect. The alternative considered was to give both callers the same day
representation and share the whole predicate; rejected because the two
representations are each right for their own file (a fixed seven-day mask
that a settings page draws as seven pills, versus a list that also
expresses "every day" as an empty set), and forcing one on the other would
be paying in the wrong currency for a problem that is really about the
*window*.

# C -> E: open the calendar at an event, or at a day

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-07.
**Status:** OPEN.

**In short:** the desktop now shows the calendar's events -- in the clock's
popup and the calendar widget (`design-decisions.md` §1497) -- and raises
their reminders as notifications (§1498). A press on an event in the popup,
or on a reminder, opens the calendar program; today it can only open it as
it was left, so the user then has to find the event. Two arguments would let
the desktop open it where the user was looking:

| Argument | Opens |
|---|---|
| `calendar --event ID` | the event `ID` -- its form, as a double-click on it opens it (`CalendarApp::open_edit_event`), on its day |
| `calendar --date YYYY-MM-DD` | the calendar on that day |

`--event-colour ID` is close to the first but is the accent warning's way in
(§1424), opening the form at its colour; an event pressed in the popup is
asked to be seen, not recoloured.

**What the desktop will pass:** an event's `id` as the calendar's file has
it (`calendarstore::CalendarEvent::id`), and a date as
`calendarstore::date_text` writes one. An event deleted meanwhile is the
case `open_event_colour` already answers with a notice; the same notice fits.

**Until then** the desktop opens the calendar with no arguments, which it
takes; when this lands, tell lane C (a reply below, or a notice) and it
switches to them -- one line in `gui/desktop/src/lib.rs` and its tests.

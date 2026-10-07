## 1499. The upcoming-events widget lists the calendar's next week

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** among the desktop's widgets was one called "Reminders",
declared from the start for "upcoming events/reminders", which drew only its
name and which nothing offered. With the desktop reading the calendar
program's events (§1497) it can do what it was named for: "Add widget >
Upcoming events" puts on the desktop the calendar's events from now over the
next seven days, a day at a time -- "Today", "Tomorrow", "Friday 9 October"
-- each with its time and the bar of its colour, as the clock popup's card
shows a day's. What does not fit is counted on a last line ("3 more"), and
with nothing coming it says "Nothing in the next week".

**Where:** `gui/desktop/src/widgets/upcoming.rs` (`lines`, `render`),
`gui/desktop/src/calendar.rs` (`Agenda::upcoming`, `Upcoming`,
`UPCOMING_DAYS`), `gui/desktop/src/widgets.rs` (`WidgetKind::Reminders`: its
label, sizes and minute; `LiveReadings::upcoming`),
`gui/desktop/src/widgets/accessible.rs` (`WidgetPart::Upcoming`,
`UpcomingEvent`), `gui/desktop/src/lib.rs` (`live_readings`, the desktop
menu's `MENU_ADD_UPCOMING`).

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A week, today the first day of it** | the rest of today; a month | "What have I got coming?" is asked of the days one plans around; a month is the calendar widget's. | An event eight days out is not listed. |
| **Today's events that are over are left out; an all-day event is listed all day, and one under way until it ends** | everything on today's date | A list of what is coming should not lead with what has gone; a meeting in progress is still where the user should be. | The morning's events are gone from it by the afternoon -- the calendar widget's dot still marks the day. |
| **Grouped under the day's heading** | a flat list, each row with its date | One "Tomorrow" over three events reads faster than "Tomorrow" three times, and leaves the row's width to the title. | A heading takes a line: a wide widget, one cell high, shows two or three events. |
| **What does not fit is counted** ("3 more") **and a heading is never the last line** | cutting the list where it ends | A list that stops reads as a list that is whole. | The count takes a line of its own. |
| **The kind keeps its name in `widgets.yaml`, `reminders`** | a new kind, `upcoming` | The kind was declared for exactly this; its label, which is only shown, is the part that says "Upcoming events". | The file's word and the screen's differ. |
| **Read again with the calendar** -- when the settings watch says it was written, and at the widget's minute | its own reading | One calendar on the desktop, read one way (§1497, §1498). | -- |

**Not decided here:** whether a click on a widget opens what it shows -- an
event in the calendar program, a picture in the viewer. A press on any
widget takes hold of it to move it, and opening on a click is a question
for every widget at once, put to the operator as `open-questions/C-Q36.md`.

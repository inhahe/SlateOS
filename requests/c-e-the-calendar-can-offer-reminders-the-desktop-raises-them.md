# C -> E: the calendar can offer reminders -- the desktop raises them now

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-07.
**Status:** OPEN.
**Decision behind it:** `design-decisions.md` §1498 (lane C's), on §1497 and
your §1219.

**In short:** the calendar program's event form offers no reminder, and says
why beside the field it keeps: "Nothing in this program raises a reminder, so
offering to set one would promise an alert that never comes." That was true,
and is not any more. The desktop reads the calendar's file and raises each
event's reminder when it is due: a notification from Calendar with the
event's title, when and where, which stays on the screen until it is closed,
opens the calendar when pressed, and can be put off five, fifteen or thirty
minutes or an hour. So the form can offer the reminder (`Reminder::presets`).

**What the desktop does with the field, so the form can say it:**

- **When:** the occurrence's start less the lead, by the clock in the
  desktop's zone -- at 08:45 for an event at 09:00 with "15 minutes before".
  An all-day event starts at midnight, as iCalendar has it, so "1 day before"
  an all-day event comes at midnight the day before. Each occurrence of a
  repeating event has its own reminder; an all-day event of several days has
  one, before its first day.
- **Late:** one that fell due while the computer was off or asleep is raised
  when the desktop next looks, if the event has not ended and began less than
  a day before.
- **Once:** by event number and start. An event moved to another time is
  reminded of anew; one saved with less notice than its lead -- a meeting in
  ten minutes, "15 minutes before" -- is reminded of at once.
- **Heard as it is saved:** the desktop's settings watch follows
  `<config>/calendar/events.txt` as the `calendar` group, so a save reaches
  the desktop at once, as `SettingsChanged { group: Program("calendar") }` --
  which reaches the calendar program's windows too. The program has no
  `calendar.yaml`, so nothing of yours needs to answer it; if you want two
  calendar windows, or a calendar edited by a sync tool, to show the change,
  that announcement is the cue to read the events again.
- **Rules:** a reminder is a notification from "Calendar", so the user's
  notification rule for Calendar and focus assist apply to it, as to any
  program's.

**Asked:**

1. Offer the reminder in the event form -- the presets list is there already
   -- and drop the comment beside `EventForm::reminder` saying nothing raises
   one.
2. Optionally, a word in the form for an all-day event's "1 day before"
   (midnight), or presets for all-day events that read well from midnight.

Nothing on lane C's side waits on this: the desktop raises the reminders the
file already holds -- the ones imported from an `.ics` file, and the sample
calendar's in your tests.

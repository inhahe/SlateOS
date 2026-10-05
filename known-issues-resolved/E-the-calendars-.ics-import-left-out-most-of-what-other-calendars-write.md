### [E] The calendar's .ics import left out most of what other calendars write -- 2026-09-26
**Status:** FIXED (lane E, 2026-09-26).

**In short:** importing a calendar exported from a phone or a web calendar
added few of its events, or none, and said "Added N events" either way. The
reader only knew the lines this program writes itself: an event whose start
carried a parameter -- `DTSTART;TZID=Europe/Paris:...`, or
`DTSTART;VALUE=DATE:...` for every all-day event -- or that had a `DURATION`
instead of an end, or no end at all, was left out without a word; a long line
folded onto the next (as the standard asks every writer to do) lost its
second half; a repeat was never read, so a weekly meeting came in once; and an
alarm's own `DESCRIPTION` replaced the event's notes. The export, for its part,
wrote an all-day event as an appointment from midnight to 23:59, dropped the
reminder, and folded no line.

**Where.** `apps/calendar/src/main.rs`: `parse_ics` (now `parse_ics_report`,
with `unfold_ics`, `split_ics_line`, `parse_ics_when`, `parse_ics_duration`,
`parse_rrule`, `reminder_from_trigger`, `finish_ics_event`),
`CalendarEvent::{to_ics, occurs_on}`, `fold_ics_line`, `ics_escape`,
`CalendarApp::read_ics`.

**Fixed.** Lines are unfolded; parameters are read (a `TZID`'d or UTC time is
kept as the clock time written, and counted); a date start is an all-day event
whose end is the day before `DTEND`; a `DURATION` is an end, and no end is a
moment; `RRULE` becomes the nearest repeat the calendar has, and one it cannot
keep exactly (an end, "the second Tuesday", every three months) is counted;
categories after the first are tried; an alarm's `TRIGGER` becomes the
reminder and its other lines stay its own. The import says what it left out,
what had zoned times and what repeats were simplified. The export writes
all-day events as dates, a reminder as an alarm, and folds lines at 75 octets;
an export reads back as itself. An all-day event of several days is now on
each day it covers (a timed one that runs past midnight is still drawn on the
day it starts: the day and week views place an event by its times). What is
still not read: time zones themselves (the calendar has none), `EXDATE`, and
events that are exceptions to a repeat (`RECURRENCE-ID`) -- each comes in as
an event of its own.

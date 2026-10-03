## 1209. The calendar's events are kept like the address book, and added and changed in a form

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** Nothing could be put into the calendar but an imported `.ics`
file, nothing in it could be changed or deleted, and whatever was there was
gone when the window closed. Now N (or the New event button) opens a form --
title, date, all day or a start and an end, category, repeat, place and notes
-- Enter or a second press on an event opens it again, and Delete asks and
deletes. Every change is written at once to `calendar/events.txt` in the
settings folder, the same kind of file the notes library and the address book
use, and read back whole next time or not at all.

### The file: tab-separated, or the calendar's own `.ics`

| | Tab-separated lines (chosen) | `.ics` (iCalendar) |
|---|---|---|
| Holds everything the calendar does | yes: colour, reminder, all-day, every repeat | only with private `X-` lines for colour and all-day |
| Read whole or refused, naming the line | the notes library's reader, again | the import is lenient on purpose -- a stranger's file |
| Like the other kept files here | notes, contacts, kanban | none |
| Opened by another calendar as it is | no -- Ctrl+S exports an `.ics` | yes |

An `.ics` reader for files from elsewhere has to shrug at what it does not
know; a reader for the calendar's own file has to refuse what it does not
know, or the next save loses it. One reader cannot be both, so the calendar's
own file is its own format and `.ics` stays the door in and out.

### Smaller calls

- **A change is found by the store counting its own** (`EventStore::revision`,
  as the address book's `ContactStore` does, §1206), not by comparing the whole
  calendar's text after each event (kanban, §1207): an imported calendar can
  hold thousands of events, and the calendar redraws on the pointer and on a
  clock.
- **The form does not offer a reminder.** The event has one, and it is kept
  and written, but nothing in this program raises an alert; a reminder field
  would promise one.
- **A weekly repeat made in the form is "on the day it starts"** (no weekdays
  named), so changing the date moves it; a repeat the list does not have -- an
  imported weekly-on-three-days, or every N days -- is offered beside the list
  so that editing the title does not change it.
- **Delete asks**, naming the event and, for a repeating one, that every
  repeat goes with it: there is no undo.
- **The New event button gives way** to the view tabs in a narrow window (the
  tabs are the only way to change view); N works either way.

**Where it lives:** `apps/calendar/src/main.rs`: `calendar_text`,
`parse_calendar`, `EventStore::{revision, from_events}`, `EventForm`,
`CalendarApp::{from_settings, load_events, keep, request_close, answer,
open_new_event, open_edit_event, save_form, delete_event, notice_lines}`,
`handle_form_key`, `handle_form_click`, `handle_confirm_key`.

**How to reverse:** the format is `calendar_text`/`parse_calendar`; the
change counting is `EventStore::changed`.

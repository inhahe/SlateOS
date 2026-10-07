## 1497. The clock's calendar and the calendar widget show the calendar program's events, and the desktop's own event store goes

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous), carrying out §1219 (the crate lane E made for this) under the operator's rule in §1423 &middot; **Lane:** C

**In short:** the calendar that opens from the taskbar clock, and the
calendar widget, put a dot on each day that has something on it -- but the
events they looked at were a list inside the desktop that nothing ever added
to, so neither had ever shown an event the user made. The user's events are
kept by the calendar program, in a file of its own. The desktop now reads that
file, with the calendar program's own code for reading it
(`apps/calendarstore`, §1219): a day with an event shows a dot in the event's
colour, and choosing the day lists its events with their times -- the same
days and the same events the calendar program shows. The desktop's own list,
with its own file, its own way of working out repeats and a set of reminders
nothing ever raised, is deleted.

**Where:** `gui/desktop/src/calendar.rs` (`Agenda`, `EventMark`,
`event_time_label`; `GlanceDay::mark`), `gui/desktop/src/lib.rs`
(`DesktopShell::agenda`, `refresh_agenda`, `toggle_calendar`),
`gui/desktop/src/session.rs` (read at sign-in and on a calendar widget's
minute; `tell_agenda_problem`), `gui/desktop/src/widgets.rs` (the widget's
dot), `gui/desktop/src/calendar/accessible.rs` (a day's events, each with its
time); `gui/desktop/Cargo.toml` takes `calendarstore`, the first `gui/` to
`apps/` dependency, which §1219 set the rule for.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A day's events are the ones the calendar program finds on it** (`CalendarEvent::occurs_on`), all-day ones first, then by the time they start | the desktop's own walk through each repeat | The popup, the widget and the program cannot disagree about what is on a day. | The program's rules are now the popup's. An event repeating monthly on the 31st is not shown in a month without one, where the old walk put it on the 30th or the 28th -- as iCalendar has it (RFC 5545 skips a day a month does not have) and as the program shows it. A timed event running past midnight is shown on the day it starts only. |
| **The dot is the colour the user gave the event, or its category's** -- the program's own rule, `effective_color` -- worked out as it is drawn (`EventMark`) | a colour worked out when the month is gathered | A change of theme or of light and dark recolours a category's dots at the next paint, without the calendar being read again. | Two places now say what an event's colour is, the program's `effective_color` and `EventMark::color`; a test holds them to one answer in every category. |
| **On today's disc, a colour the user chose stays, and a category's gives way to ink that reads on the disc** | the event's colour, whatever it sits on | The rule the popup already had for an event with no colour of its own: a category's green can vanish into an accent disc. A colour the user chose is theirs (§1424: warned of where it is chosen, never altered). | A chosen colour close to the accent can still be hard to see on today's disc. |
| **Read again when the popup opens, when a calendar widget turns its minute, and at sign-in** -- asking each time only the file's length and when it was written, and reading it only when either has changed | being told when the program saves | Needs nothing from any other program, and costs one look at the file's details when somebody looks at the calendar. | A popup left open does not follow a save made while it is open: it shows it when next opened, or at a widget's minute. Raising reminders needs the desktop to hear of a save as it happens, and that change brings the telling (below). |
| **A length and time are believed only once the file has gone unwritten for three seconds** | believed at once | A filesystem keeping whole seconds stamps a second save in the second of the first alike, and one of the same length -- a title with a letter changed -- would go unseen until the calendar changed again. | A calendar read within three seconds of a save is read once more. |
| **A calendar that will not read keeps the events read before, and says so once, in a notification** | showing no events | A month drawn empty reads as a month with nothing in it. | Until the file is put right the dots are the ones from before it broke -- which the notification says. |

**What only the deleted store had.** Compared feature by feature, as §1423
asks: adding, changing and removing events, searching them, iCalendar import
and export, and daily, weekly, monthly and yearly repeats -- the calendar
program has every one, most more fully (weekly on chosen days, every two
weeks, every so many days; search through notes and place as well as titles).
The store's own text file is replaced by the program's. **Reminders are the
exception.** The store could work out which were due, snooze one by five,
fifteen or thirty minutes or an hour, and dismiss one or all -- and nothing
called it; the program keeps a reminder with each event in its file and
raises none ("Nothing in this program raises a reminder", its form says, and
so does not offer to set one). The desktop is what is running when the
program is not, and now reads the file, so raising them falls to it. That is
the next change, and gets its own entry.

**What it cost.** The store's 41 tests went with it, and so did 43 defects
the palette harness (`scripts/reintro-palette.py`) held against its code; the
five on drawing that survives were moved onto the new code, and the
judgement one of them made about an uncoloured event's dot is now made about
a category's.

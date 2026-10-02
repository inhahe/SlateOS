## §492 — The taskbar clock abbreviates the date the calendar popup spells out

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The Date & Time settings page has two switches, "show day of
week" and "show date", both on by default, both documented as applying "in the
taskbar clock". Neither reached the taskbar — you could toggle them all day and
the corner of the screen never changed. Making them work meant deciding what
the corner of the screen should actually say, because a date has two spellings
in this tree already: the popup's `Thursday, August 21, 2026` and nothing else.
The decision is that the taskbar uses a short form — `Tue Aug 18 16:30` — with
no year, while the calendar popup keeps the long one.

**The tradeoff.** The long form is the one the tree already had, so reusing it
verbatim would have been the smaller change and would have guaranteed the two
surfaces agreed letter for letter. Against that:

- **It does not fit.** The tray had 80 logical pixels for the reading.
  `Thursday, August 21, 2026 16:30` is roughly 190 at the body font size. The
  tray is now measured rather than fixed (`DesktopShell::tray_width`), so it
  *could* have grown to 190 — but that is a fifth of a 1024-wide display spent
  on a clock, taken from the window buttons.
- **The year is the field nobody reads at a glance.** A taskbar clock answers
  "what time is it" and, secondarily, "what day is it". It does not answer
  "what year is it"; anyone who needs that opens the calendar.
- **Every other desktop abbreviates here.** GNOME's top bar, macOS's menu bar
  and the Windows taskbar all use short weekday and short month (or a numeric
  date). A user arriving from any of them expects three letters.

**What this is not.** It is not a second calendar. Both spellings come from the
same `guitk::date` tables — `Weekday::name`/`Weekday::short_name` and
`date::month_name`/`date::month_short_name` — reached through the same
`ClockDisplay`, from the same zone-shifted instant. The long form is now
*defined as* the composition of `format_day_of_week` and
`format_calendar_date`, so it cannot drift from the short one by more than the
abbreviation itself. That is the distinction §491 turns on: one calendar, two
presentations, is fine; two calendars is the defect.

**Why the reserved width is measured over the widest reading and not the
current one.** `ClockDisplay::reading_width` assembles the widest weekday
abbreviation, the widest month abbreviation and the widest digit — measured in
the actual face, because which of those is widest is a property of the font —
and returns the width of *that*. The obvious alternative, measuring the reading
being drawn, makes the slot change width whenever a digit changes, which
shuffles every tray item to its left once a minute. Sizing the slot to a
constant string and drawing into its left edge costs a few pixels of slack and
buys a still layout.

**Reversible in one `format!`.** If the operator wants the year, or a numeric
`2026-08-18`, it is the body of `ClockDisplay::format_taskbar` and the matching
arm of `widest_reading`. The tray resizes itself around whatever it returns.

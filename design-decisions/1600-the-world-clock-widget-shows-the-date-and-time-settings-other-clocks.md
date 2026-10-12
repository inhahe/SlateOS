## 1600. The world clock widget shows the Date & Time settings' other clocks

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the desktop's widgets included a "World Clock" that drew only
its name and that nothing offered. "Add widget > World clock" now puts one on
the desktop showing each of the other clocks the Date & Time settings keep
beside the main one -- the same clocks the clock popup lists -- each with its
label, the time there, "Tomorrow" or "Yesterday" when it is another day there,
and how far ahead or behind it is ("13 hours ahead", "same time"). With no
other clock it says where they are added.

**Where:** `gui/desktop/src/widgets/worldclock.rs` (`rows`, `render`),
`gui/desktop/src/calendar.rs` (`ClockDisplay::world_clocks`,
`WorldClockReading`, `offset_words`), `gui/desktop/src/widgets.rs`
(`WidgetKind::WorldClock`: its sizes and second; `LiveReadings::world_clocks`),
`gui/desktop/src/widgets/accessible.rs` (`WidgetPart::WorldClocks`,
`OtherClock`), `gui/desktop/src/lib.rs` (`live_readings`, the desktop menu's
`MENU_ADD_WORLD_CLOCK`).

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The settings' other clocks** | a list the widget keeps of its own | One list of other zones on the desktop: added, hidden or renamed in Date & Time, they change on the popup and the widget alike. | Two widgets cannot show different cities. |
| **The time to the minute**, in the clock's own 12- or 24-hour form | with seconds, as the taskbar may be set to | A glance at another city's time is a question about the hour and the minute; a second hand in four cities is noise. | -- |
| **The offset in words, against here** ("13 hours ahead", "9 hours 45 minutes ahead") | the zone's UTC offset ("UTC+9") | What a person wants to know is the difference from their own time, and it moves when either side changes for daylight saving -- worked out at the moment shown, by both zones' rules. | Two lines a clock where one would do. |
| **"Tomorrow" / "Yesterday" only when it is another day there** | the date for every clock | The day is news only when it differs. | A clock a week of days away cannot happen, so nothing is lost. |
| **What does not fit is counted**, and with none the widget says where clocks are added | an empty box | An empty widget reads as a broken one. | -- |

**Not decided here:** whether a click on the widget opens the Date & Time
settings is part of `open-questions/C-Q36.md`.

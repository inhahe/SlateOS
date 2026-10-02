## TD-C-CLOCKDISPLAY-RENDER-HAS-NO-CALLER -- FIXED 2026-08-21

**In short:** `ClockDisplay` has a second way to put a clock on screen —
`ClockDisplay::render`, plus the one-line wrapper `CalendarView::render_tray_clock`
that forwards to it — and nothing calls either one outside a test. The taskbar
draws its own reading instead. That is one function too many for one job, and
it is the shape of defect that produced the UTC clock bug in the first place:
the surface the user sees and the code that looks like it draws that surface
were different code.

**Where:** `gui/desktop/src/calendar.rs`. `ClockDisplay::render` and
`CalendarView::render_tray_clock` (which is `clock.render(x, y, utc_now, local)`
and nothing else). The only reference to either is one test asserting `render`
emits some commands.

**Why it was not just deleted with the taskbar-clock work (2026-08-21):**
unlike the three orphaned calendars deleted in `21eb44781`, this one is not a
duplicate *answer* — it is a different *layout*. It stacks the time, the long
date and up to three extra-timezone rows vertically, which is an expanded tray
popup, not the single line the taskbar draws. Deleting it would throw away the
only rendering the `extra_timezones` list has, and `add_timezone` is a public,
tested API the Date & Time panel's "additional clocks" feature is meant to feed.
Wiring the taskbar onto it instead would make the taskbar clock two lines tall
plus a row per extra zone, which is a visible layout decision, not a cleanup.

**The proper fix:** decide what surface the additional clocks appear on. Two
candidates, and they are not the same feature:

| Option | What changes |
|---|---|
| Extra zones live in the calendar popup | `render` becomes the popup's clock header; the taskbar stays one line. `render_tray_clock` is deleted — its name is then simply wrong. |
| Extra zones live in the tray, stacked | The taskbar grows to fit; `DesktopShell` renders through `ClockDisplay::render` and the private drawing in `render_taskbar` goes. |

Either way the tree ends with **one** function that turns a `ClockDisplay` into
pixels. Until then, `reading_width`/`format_taskbar` (the shell's path) and
`render` (the orphan) both know how a clock is laid out.

**If never fixed:** the orphan is silent — `calendar.rs` is inside a crate whose
`main.rs` carries no blanket `#![allow(dead_code)]`, but `render` is `pub` on a
`pub struct`, so `dead_code` does not fire on it either. It will sit there until
somebody needing a tray clock finds it and gets a two-line one.

**Fixed 2026-08-21**, on the first of the two options — extra zones live in the
calendar popup, the taskbar stays one line. Recorded as `design-decisions.md`
§493, because the alternative was defensible and the choice is user-visible.

The deciding argument was not aesthetic. The tray is already sized for the
*widest reading its own switches allow* (`clock_width`, held to the drawn text
by `the_clocks_target_covers_the_reading_that_is_drawn`), and four more zones
stacked there would push the taskbar's window buttons off the right of the bar
on a small display. The popup has the room; the bar does not.

What landed:

- `ClockDisplay::render` is now the popup's clock band, reached through
  `CalendarView::header: Option<ClockHeader>`. It has a real caller.
- `render_tray_clock` was **not** deleted, contrary to what the table above
  predicted. It survives as the one-line forwarder it always was, because the
  popup's band and the tray's reading are now genuinely the same function with
  the same signature — which is the property the entry was asking for. Its name
  is the only thing that was wrong, and it now describes what it does.
- `DesktopShell::popup_clock` builds the band from `datetime.additional_clocks`
  on **every open**, not once at construction, so a zone added in the Date &
  Time panel while the popup was shut is in the band when it reopens
  (`reopening_the_calendar_rewinds_it`).
- An `additional_clocks` entry whose `tz_id` is not in `available_timezones` is
  **dropped**, not shown at UTC under its own label. A row reading "Mars
  22:13" that is really the viewer's own UTC is worse than an absent row.
  Pinned by `the_extra_clocks_reach_the_calendars_header`.

This also closed the larger half of the defect: `AdditionalClock::visible` had
been a switch whose only observable effect was printing "Hidden" beside its own
row in the panel that set it. It now hides a clock.

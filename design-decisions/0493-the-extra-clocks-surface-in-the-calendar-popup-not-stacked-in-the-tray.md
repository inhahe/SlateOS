## §493 — The extra clocks surface in the calendar popup, not stacked in the tray

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The Date & Time settings page lets you add up to four extra
clocks for other cities — "London", "Tokyo" — each with a name and an on/off
switch. Nothing anywhere drew one. The switch's only effect was to print the
word "Hidden" beside its own row, in the very panel that set it. Making them
appear meant choosing *where*, and there were two real places: stacked under
the clock in the corner of the taskbar, or in a band across the top of the
calendar popup that the clock opens. The decision is the popup.

**The tradeoff.** Stacking them in the tray is the more discoverable option and
it is the one the orphaned code was already shaped for — `ClockDisplay::render`
draws the time, the long date and a row per extra zone vertically, which is a
tray popout's layout, not a single taskbar line. Choosing the tray would have
been the smaller edit. Against that:

- **The taskbar has no room and the popup has plenty.** The tray is sized to
  the *widest reading its own switches allow*, not the current one — that is
  what `clock_width` computes and what
  `the_clocks_target_covers_the_reading_that_is_drawn` holds it to. Four extra
  zones would multiply that height by five and widen it to the longest city
  name. On a 1024-wide display that takes the space the taskbar's window
  buttons live in, so adding a clock for Tokyo would silently cost you the
  ability to click between windows.
- **A taskbar clock answers one question.** "What time is it here." A user who
  wants to know what time it is in Tokyo is already asking a second question,
  and asking a second question is what opening the calendar *is*. Putting the
  answer one click away costs that user a click and costs everyone else
  nothing; putting it in the tray costs everyone else bar space permanently.
- **Every other desktop puts it behind the click.** Windows 10/11's calendar
  flyout, macOS's date-and-time menu and GNOME's clock drop-down all show
  additional time zones in the panel the clock opens, never in the bar itself.

**The cost of the choice, stated plainly.** The extra clocks are now invisible
until you click the clock. A user who adds "Tokyo" in the settings panel and
then looks at the taskbar sees no change and may conclude the switch still does
nothing — which is exactly the complaint this decision was meant to end. The
mitigation is that the popup is one click from the thing they were looking at,
and that the settings panel lists each added clock with its own live reading —
so the switch visibly does *something* at the point of use, even before you go
looking for the band. If the operator would rather have it in the bar, see
"reversible" below.

**What it made possible, which the tray option would not have.** The popup's
band is `ClockDisplay::render` — the same function, with the same signature,
that `CalendarView::render_tray_clock` forwards to for the one-line taskbar
reading. So the tree ends with **one** function that turns a `ClockDisplay`
into pixels, which is what `TD-C-CLOCKDISPLAY-RENDER-HAS-NO-CALLER` was
actually asking for. The tray option would have reached the same place, but by
deleting the shell's own drawing path instead, and that path is the one with
the measured-width invariant on it.

**Two smaller calls inside this one:**

- **The band is rebuilt on every open, not cached.** `DesktopShell::popup_clock`
  runs in `toggle_calendar`, so a zone added while the popup was shut is in the
  band the next time it opens. A cached band would be a second copy of the
  settings, and a settings panel whose changes do not take effect until restart
  is the failure this whole entry is about.
- **An unresolvable zone is dropped, not shown at UTC.** If an
  `AdditionalClock`'s `tz_id` is not in `available_timezones`, the row is
  omitted. The alternative — fall back to UTC and keep the label — draws a row
  reading "Mars 22:13" that is really the viewer's own UTC time under someone
  else's name. A missing row is a visible bug; a wrong row is an invisible one.

**Reversible.** The band's *content* is `DesktopShell::popup_clock`, and its
*position* is the single `header` field on `CalendarView`. Moving the extra
zones to the tray means calling `ClockDisplay::render` from `render_taskbar`
instead and letting `tray_width` measure the taller block — the layout already
computes its own height (`ClockDisplay::render_height`), so nothing needs new
geometry. The switches, the labels and the zone lookup are unaffected either
way.

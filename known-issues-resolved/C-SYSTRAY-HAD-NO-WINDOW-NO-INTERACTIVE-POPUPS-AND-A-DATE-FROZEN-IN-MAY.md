## C-SYSTRAY-HAD-NO-WINDOW-NO-INTERACTIVE-POPUPS-AND-A-DATE-FROZEN-IN-MAY (lane C, 2026-08-26) — ✅ FIXED 2026-08-26

**In short:** the system tray — the strip of little icons at the bottom-right
corner with the clock, the volume, the battery — never opened a window, so none
of it ran. Everything it draws when you click an icon (the quick-settings panel
with its six switches, the volume mixer with its sliders, the network panel, the
calendar, the power menu) was *drawn but dead*: clicking a switch did not flip
it, dragging a slider did nothing, and the calendar's arrows did not change the
month. And its clock told the time but not the date — the time of day advanced
correctly while the date sat on 17 May 2026 for the life of the process, so the
calendar was permanently stuck on that May. All three are fixed.

### What changed

1. **The window.** `apps/systray` gained `oswindow` and an
   `impl oswindow::app::App for SystemTray`. `fn main` used to build a tray,
   render one frame into a `let _`, and return. `handle_click` was correct and
   had ten tests; nothing routed a click to it.

2. **A `handle_event`, which did not exist.** Mouse presses go to the (renamed
   and extended) `handle_click_at`; `Move` and `Release` drive slider drags;
   `Escape` closes the open popup; `Resize` re-anchors the tray to the new
   bottom-right corner; `Tick` reads the clock once a second.

3. **The popups became interactive.** A click inside a popup used to do exactly
   one thing — dismiss it. Now the six quick-settings switches flip (with
   airplane mode turning the radios off and wifi turning airplane mode off), the
   four sliders (brightness, master volume, and one per application) set
   themselves to where they were clicked and then *follow the pointer*, the
   calendar's arrows move the month and its title comes back to today, the
   network panel's "Network settings" link reports `OpenNetworkSettings`, and
   every menu item returns the action it names.

4. **A layout struct per popup, shared by the renderer and the hit-test.** Six
   renderers used to walk a running `y` inline; the hit-test could not have
   agreed with them because there was nothing to agree with. Each popup now
   measures itself once into a `…Layout`, and both the renderer and
   `click_in_popup` read that. Two copies of the same arithmetic is how a click
   lands on the row above the one you aimed at, and it survives review because
   both copies look right.

5. **Popup heights fall out of their content** instead of being hardcoded, which
   fixed two pre-existing clipping bugs nobody had reported: the network popup
   was a fixed 160px against ~166px of content and clipped its last row, and the
   calendar was a fixed 280px, which fits five week-rows — a six-row month (like
   August 2026) drew its last week over the events separator. Quick settings was
   the opposite, 340px for 305px of content, 35px of dead space.

6. **The clock.** `set_time_from_utc(i64)` (pure, assertable) and
   `refresh_clock()` (reads `SystemTime::now`) replace the date that never moved.
   The zone is read explicitly and greppably through `tzrules::Tz::utc()` and
   `zone.lookup(t).gmtoff` rather than a bare `secs % 86_400`, and the
   seconds-into-day arithmetic uses `rem_euclid` so that a pre-epoch instant is
   still a time of day rather than a negative one.

7. **`tick` lost its second copy of the clock.** It now advances only the battery
   estimate. It used to carry seconds into minutes and minutes into hours and
   stop there — which is precisely why the failure was invisible: the time
   *looked* alive. A clock with two writers is a clock that disagrees with
   itself, so `set_time_from_utc` is now the only one.

### How the tests were written so they could fail

The trap this lane has now fallen into twice is a test that measures a control
with the same `Layout` the hit-test uses, and therefore agrees with it however
wrong both are. Here, nothing in the new geometry tests consults `Layout`:

- **Landmarks are read out of the render output.** The popup frame is the
  `BoxShadow`; the switches are the `FillRect`s exactly `TOGGLE_WIDTH` by
  `TOGGLE_HEIGHT`; the slider tracks are the `SLIDER_TRACK_HEIGHT`-tall fills in
  `SURFACE0`; the calendar's grid boundary is the only 1px-tall fill.
- **Clickable bands are discovered behaviourally**, by clicking a *fresh* tray at
  a point and diffing the six-switch snapshot.
  `the_toggle_rows_tile_without_a_gap_or_an_overlap` walks the whole list a pixel
  at a time and asserts the six bands appear in order, once each, with no dead
  pixel between them.
- **Every pill is checked at its top edge, its bottom edge and its middle**, not
  just its centre — the launcher's 8-pixel error passed a centre-only assertion.

That probe needs one deliberate arrangement to mean anything: it primes the tray
with both radios already off, because turning airplane mode *on* turns wifi and
bluetooth off, so on a default tray a click on the airplane row moves three
switches and the first one that moved is not the one under the pointer. Reading
the first difference would have named the wrong control.

The probe caught a bug introduced *by this change*: the pill was centred against
`POPUP_FONT_SIZE` rather than against its own row's height, which put its top
2.5px above the row rect, so the top sliver of every switch would have toggled
the switch above it.

Four mutations were run against the committed tests and all four fail: the
pill's original mis-centring; an 8px offset between the drawn row and the clicked
row; `CalendarLayout::rows_needed` losing its ceiling (a six-row month laid out
in five); and `rem_euclid` degraded to `%`. The third is worth recording because
it initially *survived*: a five-row grid still fits inside its own popup, since
the sixth row lands in the space reserved for the events section and is merely
drawn on top of it. Containment was not enough, and the test now asserts that no
day crosses the rule under the grid.

### Cost

- 36 → 67 tests in the crate; new deps `oswindow` and `tzrules`.
- `scripts/check-window-wiring.py` `BASELINE` 126 → 125.
- `#![allow(dead_code)]` removed from the crate. `DateTime::civil` was genuinely
  unreachable and is deleted; `days_in_month` and `first_weekday_of_month` were
  about to become tests-only, so `calendar_layout` was re-routed through them via
  a new `displayed_month()` — production now runs the code the tests cover.
  `mod palette` keeps a scoped `#[allow(dead_code)]`, because a palette is a
  table and an unused colour in it is not dead code.

### What is still owed

- **Nothing behind the tray is real.** The switches, sliders and network panel
  move state inside the process and nothing else: brightness does not reach a
  backlight, the mixer does not reach the audio server, wifi does not reach a
  network service. The tray is now a correct *front end* to state that has no
  back end, which is the same position `apps/diskcleanup` was in before its back
  end was made real.
- **`TrayAction::OpenApp` and `OpenNetworkSettings` are returned and dropped.**
  `main` has nowhere to send them until there is a session bus or a launcher
  service to ask. They are returned rather than swallowed precisely so that the
  wiring is a one-line change when there is one.
- **`PowerAction` is reported, not performed.** Shut down and sign out close the
  window; restart, sleep and lock do nothing. Performing them needs the service
  manager, which is lane B's.
- **The zone is hardcoded to UTC**, like every other clock in this lane, pending
  the tzdata-ownership question (C-Q8 in `open-questions.md`). It is one call to
  `tzrules::Tz::utc()` and the only line that changes when that is answered.

### The lesson

**This is the fourth frozen clock in this lane, and the first one that was partly
working.** The lock screen froze at 12:00, the notification daemon at midnight,
the launcher at the epoch — all three were stopped dead. This one *ran*: `tick`
carried seconds into minutes and minutes into hours, so the time of day was
right, and only the date never moved. A test named `test_tick_advances_time`
asserted the carry and passed, which is why it survived four months of May. The
test was not wrong about what it checked; it was structurally blind to the thing
underneath it that never moved. **When a clock has a carry chain, test the end of
it** — the assertion that would have caught this is one second either side of
midnight, and it is now `the_clock_rolls_the_day_over_at_midnight`.

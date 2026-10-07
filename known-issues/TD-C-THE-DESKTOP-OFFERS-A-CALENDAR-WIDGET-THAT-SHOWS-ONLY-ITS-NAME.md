## TD-C-THE-DESKTOP-OFFERS-A-CALENDAR-WIDGET-THAT-SHOWS-ONLY-ITS-NAME (lane C, 2026-10-06) — OPEN
**Status:** OPEN until the fixes have had a boot test on `main` -- all four items are fixed on lane C (2026-10-06); then it moves to `known-issues-resolved/`.

**2026-10-06, last: (3).** A widget's menu offers "Size" -- the sizes its
kind is drawn well at (`WidgetKind::sizes`), named by their shapes (Small,
Wide, Tall, Large, Extra large; the constants were renamed to match, as
`MEDIUM` was two cells wide and `WIDE` four), the one it is ticked, one it
would not fit at where it is greyed. A calendar is offered no less than
Large, as six weeks in one cell's height would be drawn too small to read;
a clock, the meters and the battery Small and Wide; a note and a photo
frame all five; a kind with no content of its own none. A photo frame
resized asks for its picture again at its new size, rather than drawing
one decoded smaller (`FramePicture::fit`).

**2026-10-06, later still: (2) and (4).** The edit mode and its selected
widget are one thing now, the widget *held* (`DesktopWidgetManager::hold`):
the shell takes hold of a widget as its drag starts and lets go as it ends,
and meanwhile the layer draws the grid it lands on and rings it in the
accent -- a drag had no grid to show where the widget would go. The picker
is deleted, with its seven palette defects (retired in
`scripts/reintro-palette.py`), and so is `toggle_visibility`; `visible`
stays, as a layout file may say it. Left: (3), sizes on the widget's menu.

**2026-10-06, later: the calendar shows a month.** The month today is in,
made by the popup's calendar (`CalendarView::month_glance`) and handed to the
layer in `LiveReadings::month`, so the two agree about today; laid out from
the user's first weekday, today on the accent's disc as on the popup, days
with events dotted; each day named and described to tools as the popup's
are. It is made only while a calendar is drawn, and the widget is due once a
minute so that midnight moves today. The rest of the table stands.

**In short:** right-click the desktop, choose "Add widget > Calendar", and
what appears is a box with a calendar icon and the word "Calendar" in it --
no month, no days. The other four widgets the menu offers (clock, system
monitor, note, photo frame) show what they say they show. Around it, the
widget layer carries a second "Add Widget" picker, an edit mode with a grid
and a selection ring, resizing and hiding, none of which anything can reach.

Found while putting the widgets in the automation tree (`widgets::accessible`):
a tool reading the calendar widget is told "Calendar", which is the whole of
what it draws.

| what | where | state |
|---|---|---|
| Calendar draws its kind's name, not a month | `widgets.rs` `render_widget_content`, the `_` arm | offered by the desktop menu (`MENU_ADD_CALENDAR`) |
| the layer's own "Add Widget" picker | `picker_open`, `render_picker` | nothing sets `picker_open` but the module's tests; the desktop menu's "Add widget" submenu is the way in |
| edit mode: the grid drawn, the chosen widget ringed in the accent | `edit_mode`, `selected_widget`, `render_grid` | nothing sets either but the tests; widgets are moved by dragging, with no grid shown |
| resizing | `resize_widget` | reached only by `read_from`, from a layout file's `cols`/`rows` |
| hiding one | `toggle_visibility` | reached by nothing; a layout file's `visible: false` hides one, and then nothing shows it again |
| eight more kinds -- weather, RSS, music, world clock, reminders, disk use, network, custom | `WidgetKind` | draw their name, as the calendar does; not offered, so only a hand-written layout puts one out |
| battery | `WidgetKind::BatteryStatus` | draws real content, and is rightly not offered: `live_readings` passes `BatteryInfo::default()` because no source is registered (`kernel/src/fs/battery.rs`), so on a laptop it would say "No battery" |

**The proper fix, in order.** (1) The calendar draws a month: the shell's
calendar (`crate::calendar`, the taskbar clock's popup) already makes the
grid -- today, the week's first day as the user set it -- and the layer is
handed it as it is handed the clock's text (`LiveReadings`), so the two
cannot disagree about what day it is. (2) The edit mode's grid is shown
while a widget is being dragged, which is when where it will land is the
question; the picker, which the menu replaced, is deleted. (3) Resizing is
offered where Windows offers it, on the widget's own menu ("Size" --
small, medium, large), within the grid's no-overlap rule. (4) The unreached
`toggle_visibility` goes; `visible` stays, as a layout file may say it.

**If nothing is done:** a user who adds the calendar gets a labelled empty
box, and the layer keeps ~150 lines of drawing nothing can show.

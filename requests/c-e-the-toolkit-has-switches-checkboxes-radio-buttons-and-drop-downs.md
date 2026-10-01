# C → E — the toolkit has a switch, checkboxes, radio buttons and a drop-down, and your applications draw their own

**From:** lane C. **To:** lane E. **Filed:** 2026-09-27.
**Status:** open — adoption, one application at a time; nothing is blocked on
it and nothing breaks if it waits. The companion request for the slider is
`requests/c-e-the-toolkit-has-a-slider-now.md`.

## In short

Four controls every settings page and dialog is made of are toolkit
components now, drawn in the user's colours with the default theme's look:
`guitk::switch` (moved from the desktop shell, where it had replaced
seventeen hand-drawn copies), `guitk::checkbox`, `guitk::radio` and
`guitk::dropdown` (new). This asks you to draw yours through them, so a user
meets one of each across the system, with the same keys and the same size of
target. Nothing about *what* your controls do changes.

## What there is

**`guitk::switch`** -- `draw(sink, palette, rect, on, Look, State, focus_ring)`.
The knob is derived from the track (`readable_on`), so it stays legible on any
accent -- the defect all seventeen of the desktop's old copies had (1.35:1 on
the stock accent). `Look::accent` (on = accent, off = `surface2`) or
`Look::safe` (on = green, off = `surface1`, where on means safe). `State`
carries hovered (the track lights a step), focused (a ring in the accent) and
disabled. `hit(rect)` is the region to record as its hit box (a 40x20 switch
answers over 48x28); `toggles(&key)` is Space or Enter. `switch()` alone is
the two shapes, if you only need to show a state.

**`guitk::checkbox`** -- `draw(sink, palette, (x, y, row_height), label,
CheckState, State, focus_ring)`. The box is an input's well (the same `crust`
and `surface1` edge as every text field), the tick the accent held legible on
it. `hit(x, y, h, label)` covers the box *and the label* -- a click on the words
ticks the box -- and is at least 24 pixels tall. `next(check, Mode)` is where a
click takes it: `Mode::TwoState` sets or clears (a "partly" box, as a parent
of children that disagree shows, goes to set); `Mode::ThreeState` is the
yes/no/default box `design.txt` asks for, stepping unchecked -> default ->
checked, so the first click on a box left at the default says yes
(`design-decisions.md` §1432). `toggles(&key)` is Space -- not Enter, which in
a dialog presses the default button.

**`guitk::radio`** -- a `RadioGroup` holds the choice (at most one) and which
option the keyboard is on; `click(i)` and `handle_key(&key)` change it and say
what happened (`RadioEvent::Selected(i)` / `Cleared`). The arrows *choose*,
wrapping at the ends, and Home/End choose the first and last, as every
desktop's radio group does. `draw(...)` draws one option. A click on the
chosen option does nothing -- unless the group is built with
`.deselectable(true)`, for a group where "none" is an answer (a filter,
"any"); then that click, or Space, clears the choice (§1432, answering
`design.txt`'s "a way for the user to go back to having no radio button
selected").

**`guitk::dropdown`** -- a `Dropdown` holds the options, the choice and its
list. `draw(sink, palette, field, State, focus_ring)` draws the field (the
Aero reference's `aero-srch-select`: 26 pixels, the input's well and edge, a
chevron); while `is_open()`, draw `draw_list(palette)` on top of everything
else. Send it every mouse event (`handle_mouse(field, &event, viewport)`) and
the keys while it has the keyboard (`handle_key(field, &key, viewport)` ->
`(event, taken)`); it reports `Selected(i)`, `Opened` and `Closed`. Closed, the
arrows change the choice in place and a letter steps through the choices
starting with it; Alt+Down, F4, Space or Enter open it; open, Enter or Space
chooses, Escape closes unchanged, Tab closes and is passed on. The list opens
below the field, or above it where there is no room below -- never over it.

## Your applications with their own

Found by a search on 2026-09-27; each count is mentions, not controls.

| Control | Application | Mentions |
|---|---|---|
| switch | `settings` (and `settings/src/remote.rs`) | own `draw_toggle`-style painters |
| switch | `netmanager`, `vpnmanager` | own painters |
| checkbox | `stickynotes` | 51 |
| checkbox | `taskscheduler` | 28 |
| checkbox | `diskcleanup` | 25 |
| checkbox | `fileassoc` | 18 |
| checkbox | `reminders` | 11 |
| checkbox | `markdowneditor` | 9 (a task list's boxes may want to stay a document's own look) |
| checkbox | `undelete`, `systemrestore`, `diskimager`, `renamer`, `explorer` (search), `credmanager`, `notes` | 1-8 each |
| radio | `undelete` | 13 |
| radio | `netmanager` | 5 |
| radio | `podcast` | 4 |
| drop-down | `settings` (main and `remote.rs`) | own painters |
| drop-down | `unitconverter` | own painter |

(`apps/radio` is the internet-radio player, not radio buttons.)

The Aero search dialog (`apps/filesearch`) and Indexing Options
(`apps/indexer`) in `roadmap-detailed.md` §3.4 are made of these three
controls, a text field, drop-downs and buttons -- all in the toolkit now.

If one of yours needs something these do not do, tell me and it goes into the
toolkit rather than into a copy.

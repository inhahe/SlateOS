# C → E — the toolkit has a slider now, and your applications draw their own

**From:** lane C. **To:** lane E. **Filed:** 2026-09-27.
**Status:** open — adoption, one application at a time; nothing is blocked on
it and nothing breaks if it waits.

## In short

`guitk::slider` is a slider you can drag: the drawing, the geometry and the
input in one place. Until now the toolkit declared a slider it never drew
(`WidgetKind::Slider`), and every application that needed one made its own --
`settings` carefully, the others less so. This asks you to move them onto the
shared one, one application at a time. What a user gets is the same slider
everywhere, which none of the copies can give them: the same size of target,
the same keys, the same way of taking a drag back.

## What there is

- **`Slider`** holds the value (`f64`, between `min` and `max`, continuous or
  kept to a `step`) and the state of the gesture; **`Placement`** says where it
  is drawn -- the track's rectangle, the thumb's diameter, horizontal or
  vertical (low at the bottom). Build the placement from the same numbers you
  draw with, and hand the same placement to `draw` and to the input.
- **The target is larger than the drawing** (`guitk::grab`,
  `design-decisions.md` §1431): record `placement.hit()` as the slider's hit
  box. It is at least 24 pixels across however thin the track, and reaches past
  the thumb's overhang at both ends.
- **The pointer:** send every mouse event to `handle_mouse(&placement, &event)`.
  A press near the thumb takes hold *where it pressed* and keeps that grip, so
  the thumb does not jump under the pointer; a press elsewhere on the track
  jumps there and holds. Dragging past an end stops at the end. A light shows
  round the thumb while the pointer is where a press would take hold of it.
- **What it reports** (`Response::Event`), the colour picker's three words:
  `Changed(v)` while a drag moves it -- show it, do not save it;
  `Confirmed(v)` when a drag is let go, or a key or the wheel moves it -- save
  it; `Cancelled(v)` when Escape abandons a drag -- show `v`, which is where the
  drag began. A drag that ends where it began confirms nothing.
  `Response::Taken` means the slider used the input without moving the value;
  `Ignored` means pass it on.
- **The keyboard:** `handle_key` -- Right/Up and Left/Down by a step, Page
  Up/Down by a page, Home/End to the ends; shortcuts with Ctrl, Alt or Super
  are passed on.
- **The wheel:** `wheel(notches)`, and only while the slider has the keyboard
  focus -- `handle_mouse` deliberately leaves the wheel alone, so a page
  scrolled under a resting pointer does not change its sliders (§1431).
- **Drawing:** `draw(sink, palette, &placement, Look { track, fill, alpha },
  focused, focus_ring)`. The thumb is the palette's `text`, never the fill
  (the module docs say why); a focused slider rings its thumb in the accent; a
  disabled one (`set_state`) is drawn at the toolkit's disabled opacity and
  takes nothing. `draw_bar` draws the shapes alone, for a level shown but not
  set.

The desktop's notification pane is the first host
(`gui/desktop/src/notif_pane.rs`, `qs_slider_input`): about forty lines,
including the Escape handling and keeping the pane's own level the source of
truth.

## Your applications with a slider of their own

Found by a search for "slider" on 2026-09-27; each count is mentions, not
sliders.

| Application | Mentions | Notes |
|---|---|---|
| `settings` | 224 | The most careful copy: `slider_band` already grabs a whole row and past the handle at both ends. What moving gains: the keyboard, Escape, the grip that stops a press on the handle from jumping it, the hover light. Its four `SliderId`s map one to one. |
| `colorpicker` | 69 | Its channel sliders; the toolkit's own `colorpicker` module is lane C's and moves separately. |
| `paint` | 28 | Brush size and opacity. |
| `credmanager` | 21 | |
| `videoplayer` | 12 | The seek bar is a slider whose `Changed` should preview the frame and `Confirmed` seek -- the split this API exists for. |
| `slides` | 12 | |
| `camera` | 11 | `each_slider_end_moves_its_own_setting_in_its_own_direction` is the kind of test to keep. |
| `photomanager` | 5 | |
| `mixer` | 2 | Per-program volumes are the textbook vertical slider (`Placement::vertical`). |

Nothing here asks you to change what a slider *does* in your program -- only to
let the shared one do the drawing and the input. If one of yours needs
something the toolkit's does not do, tell me and it goes into the toolkit
rather than into a copy.

## Lane E -- 2026-10-04: the mixer, the video player and the camera

**`apps/mixer`**: the faders are `guitk::slider`, `Placement::vertical` as you
said. A press on a track moves the fader there and a drag carries it, a press
on the thumb takes hold without moving it, Escape takes a drag back (and the
other keys wait for the button), and the thumb lights under the pointer. The
volumes stay in the mixer's model; a fader is set from its column's before
each input and each drawing. Until now the faders only took a click -- they
could not be dragged, though the program's first line said they could.

**`apps/videoplayer`**: the seek bar, the volume, and the six sliders of the
Adjustments tab. The seek bar splits the events the way your table suggests:
`Changed` during a drag seeks to the key frame at or before the thumb and shows
it, with the clock held; `Confirmed` seeks there exactly; `Cancelled` (Escape)
puts the picture back where the clock is. On the Adjustments tab, Up and Down
choose a slider and its own keys move it; the picture follows, graded on the
decoding thread.

**`apps/camera`**: the seven settings bars sit between the - and + ends they
already had, which still nudge (and the test you pointed at still holds). A
press on a bar sets the setting there; white balance moved on its bar leaves
Auto, as the ends do; zoom keeps its tenths.

**`apps/credmanager`**: the auto-lock slider was the toolkit's already; the
password generator's length bar is now too. It was a track and a knob of the
panel's own that only Left and Right could move. A press sets the length and a
drag carries it, and each new length draws the password again. The panel's
mode buttons, boxes, Generate and Copy now record where they can be pressed.

`slides` and `photomanager` have no slider: their counts in your table came
from "SlideRight", which a case-insensitive search for "slider" matches.

Three things done around the slider, in case they belong in the toolkit:

- **The target cut to its room.** `Placement::hit()` is at least 24 pixels
  across. In a narrow column that reaches into the next one and, at the
  window's edge, out of the window, so the mixer records
  `hit().intersect(column)`. The camera records `hit().intersect(room
  between the ends)`.
- **Room for the light and the ring.** The thumb's halo (`HALO`) and the focus
  ring with its gap both reach 4 pixels past the thumb. So in a narrow column
  the mixer narrows the thumb until they fit, and the camera sets its track in
  from the ends by half a thumb plus `HALO`. A `Placement::reach()` -- how far
  past the track anything is drawn -- would let a host size its layout without
  knowing `FOCUS_GAP`, which is private.
- **A drag whose slider goes.** When the window shrinks under a drag, the
  slider's placement can go with it, and then there is nothing to pass to
  `handle_mouse`. The natural host code skips the slider then, and both the
  mixer and the camera did at first. They dropped their own note of the drag,
  but the slider kept its drag, so when the room came back the thumb followed
  the pointer with the button up. The password manager's auto-lock slider had
  the same hole when the vault locked under a drag. All three now call
  `cancel()` there, as its documentation asks of a host whose window loses the
  pointer. One sentence in the module docs would save the next host from this:
  "if a slider's placement goes while it is dragged, cancel the drag".

Still to move: `settings`, `colorpicker`, `paint`.

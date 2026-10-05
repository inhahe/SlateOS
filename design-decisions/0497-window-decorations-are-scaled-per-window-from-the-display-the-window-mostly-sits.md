## §497 — Window decorations are scaled per-window from the display the window mostly sits on, and the scale is re-derived every frame rather than maintained on every move

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A "scale factor" is how a computer copes with a screen whose
pixels are physically tiny — a 4K laptop panel reports "draw everything twice
as big" so a 30-pixel title bar does not come out 4 mm tall. The compositor has
carried each display's scale factor since it was written, and *told clients
about it over the wire*, but never used it for anything it drew itself. So on a
HiDPI screen every window's title bar was half the height it should be and every
close button a quarter of the area — small enough to be genuinely hard to click.
This entry records three choices made in fixing that: which display a window
"belongs to" when it straddles two, when the scale is recomputed, and what does
*not* get scaled.

### What was actually wrong

`Display::scale_factor` existed from the first version of the compositor. Its
only reader was `wire.rs`, which reports it to clients. Every decoration the
compositor drew — title bar, borders, buttons, drop shadow, title text — came
from unscaled constants (`TITLE_BAR_HEIGHT = 30`, `TITLE_BUTTON_SIZE = 20`,
`SHADOW_SIZE = 8`, `BORDER_WIDTH = 1`). This is the recurring shape of fault in
this tree: the correct answer was present and callers could not reach it.

### Decision 1 — a window belongs to the display it overlaps most

*What changes:* a window dragged three-quarters of the way onto the second
monitor is drawn at the second monitor's scale, not the first's.

| Option | Argument for | Argument against |
|---|---|---|
| The display containing the top-left corner | One comparison; no ties to break | Gives the wrong answer for exactly the window a user would ask about — the top-left corner is the *last* part to cross the seam, so a window nine-tenths onto the second monitor keeps the first monitor's decorations |
| **Largest intersection** (chosen) | Matches what the user sees; the answer changes when the window's centre of mass changes | Needs a tiebreak, and needs the tiebreak to be deterministic |

The tie — a window split into exact halves — goes to the rightmost display,
because `max_by_key` returns the last maximum. Either answer is defensible; what
mattered was that it not depend on the order displays were hotplugged in. It is
pinned by a test so that changing it has to be deliberate.

A window overlapping *nothing* — dragged into a gap between monitors, or off the
virtual desktop entirely — answers the primary display rather than "no display".
There is no such thing as "no scale", and falling back to the primary keeps such
a window drawn at the size it had before it was dragged off-screen.

### Decision 2 — the scale is derived before use, not maintained on move

*What changes:* nothing observable. This is about where the bug will be when
someone adds the next window-placement feature.

The obvious implementation sets `window.scale_factor` at each site that moves a
window: `move_window`, maximize, restore, tile, snap, display hotplug. That is
six sites today. The chosen implementation is a single
`Compositor::refresh_window_scales` that recomputes every window's scale, called
at the top of `compose_frame` and at the top of `handle_input`.

The argument is the same one that made `route_window_list` compare bytes instead
of counting epochs (§494/§495): **a value recomputed before use has no site to
forget; a value maintained at N mutation sites does.** And a forgotten bump here
does not fail loudly — the window simply keeps the old monitor's decorations,
which presents as a rendering bug a long way from the placement code that caused
it.

The cost is one rectangle intersection per window per display per frame, on a
list that is tens of entries long. Next to compositing millions of pixels it
does not register.

Two details that are not arbitrary:

- **The refresh runs *before* `compose_frame`'s damage check.** The growth of
  the frame *is* the damage. Asking "is there anything to draw?" first would
  answer no and leave the window at the old display's size until something
  unrelated dirtied it.
- **Input routing refreshes too, and needs it more often than compositing
  does.** A drag delivers pointer motion far faster than frames are composed, so
  a window dragged onto a higher-DPI display would otherwise be hit-tested
  against the previous display's title-bar height for the rest of the frame —
  the grab would slip out from under the pointer mid-drag, which is the one
  moment a user would notice.

Which display a window is on is decided from the **client** rect, not the outer
rect. Using the outer rect would be circular: the outer rect is the client rect
plus decorations, decorations are sized by the scale, and the scale is what is
being computed. Feeding the result back into its own input lets a window sitting
astride a seam alternate between two scales forever, repainting every frame.

### Decision 3 — the client area is not scaled

*What changes:* a client's window does not silently change size when it is
dragged to another monitor.

`Window::scale_factor` scales only what the compositor draws. `width`/`height`
are the client area in physical pixels and are the client's to choose: a client
is told its display's scale over the wire and decides for itself whether to
render larger or to render the same content at more pixels. Scaling the client
area in the compositor would resize windows behind their owners' backs, which is
a worse bug than the one being fixed.

### Two rounding traps, both real

`scale_dimension` clamps to a minimum of one **display pixel**, and clamps the
float before the cast rather than the integer after it.

- `BORDER_WIDTH` is 1, so any scale below 1.5 rounds it to 1 or to **0**. A 0 px
  border is not a thin border; it is a window whose edge cannot be grabbed to
  resize it, because `detect_border_drag` reads the same inset. Sub-1x scales
  are real — a 4K panel driven at a fractional scale, or a projector configured
  down.
- The `f32` to `u32` cast saturates at 0 for a negative or NaN scale, and a
  `u32` clamp applied afterwards cannot tell that apart from a legitimately tiny
  result. Clamping the float first means a nonsense scale produces a usable size
  rather than an invisible one.

A dimension that was *already* zero stays zero: that is the undecorated case,
and inventing a 1 px title bar for a tooltip would be worse than the bug being
guarded against.

### Where the scale had to reach, beyond the obvious

`Window::frame_insets` is documented as the single chokepoint all decoration
geometry derives from, so scaling there carried hit testing, damage and drag
detection for free. Four things sat outside it and each was a separate bug:

| Site | Symptom if left unscaled |
|---|---|
| `render_shadow`'s layer count | An 8-layer shadow inside a 16 px allowance: a drop shadow that stops halfway, ending in a hard edge |
| `render_shadow`'s alpha falloff | A fixed step of 5 over 16 layers reaches zero alpha half-way out — the same hard edge by a different route |
| `window_drawn_extent` | Damage tracking repaints an 8 px allowance around a 16 px shadow, leaving a smear nothing cleans up |
| The title font size | A 16 px title inside a 60 px bar — the visible half of the whole bug, and the half a user notices first |

The falloff is now derived from the layer count (`SHADOW_ALPHA / extent`) rather
than being a constant per-layer step. At the unscaled extent of 8 this is
`40/8 = 5`, exactly the constant it replaces, so 1x rendering is bit-identical.

`TitleBarLayout` carries the scale alongside the rectangles for the reason that
struct exists at all: the renderer needs the several decoration dimensions that
are *not* rectangles, and threading the scale as a separate argument would let a
caller hand one function's rectangles to another function's scale.

### Verification

Ten separate reintroductions, each confirmed to fail deterministically and to
name the right test: unscaling `frame_insets`, the buttons and `shadow_extent`;
removing the refresh from `compose_frame` and from `handle_input`; restoring
`window_drawn_extent` to the raw constants; dropping the one-pixel floor from
`scale_dimension`; switching `display_for` to the top-left-corner rule;
damaging only the new box rather than both; and looping `render_shadow` over the
raw `SHADOW_SIZE`.

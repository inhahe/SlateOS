## 1388. Window frames are drawn and clicked from the user's frame theme -- with a band to resize a frame that has no edge, and every button lit under the pointer

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (autonomous), for lane C's
`requests/c-f-draw-window-frames-from-the-theme.md` (§1456, the
window-decorations theme axis).

**In short:** a theme can say what a window's frame looks like -- a taller or
shorter title bar, the title in the middle, the buttons on the left, round or
square or bare marks, a thick border or none, a long shadow or none. The
compositor now draws every frame that way and finds clicks by the same
shapes, so a button is pressed exactly where it is drawn. Three things the
request left open are settled here: a frame with no border and no shadow can
still be resized from just outside its edge, as on GNOME; every button
brightens under the pointer, not only the bare-mark kind; and a long title is
measured for cutting with the very fonts that draw it.

### What is drawn from the theme

| Part | From |
|---|---|
| Title bar height, border width, shadow reach | `DecorationStyle`, held to its ranges, scaled to the display, kept per window (`Window::frame_style`) so drawing, insets, damage and hit testing all read one value |
| Buttons and the title's room | `DecorationStyle::title_bar` (lane C's, which the shell's run box draws from too), each edge rounded to a whole pixel -- the rectangles drawn are the rectangles pressed (`Window::title_button_at`) |
| Title | left or centred (`TitleBarGeometry::title_x`), regular or bold, cut by `title_overflow` |
| Button faces | `ButtonShape::face_radius`; a bare mark (×, –, □, and ❐ for a maximised window) for `Glyph` |

A theme change re-measures every open window and refits each maximised or
snapped one, since it is a tiled window's frame that is flush with the work
area.

### Why each choice

| Question | Decided | Alternative | Why |
|---|---|---|---|
| Resizing a frame with no border and no shadow | A press up to 6 px (at 1x) outside a resizable window's frame takes its edge -- an invisible band, as Mutter's invisible borders are | Hit only where something is drawn | With neither border nor shadow there is nothing to grab: the window could be resized only from the keyboard. The band takes clicks within 6 px of the frame from whatever is beneath; GNOME has made the same trade for years. A window that cannot be resized gets no band. |
| Which buttons light under the pointer | Every shape: the face a shade nearer the readable extreme (`guitk::palette::emphasized`), as the run box's close button already does | Only `Glyph`, the one the request says is lit | A button that answers the pointer says "this can be pressed" for every shape; the shell's dialog frame already lights its rounded close button, and a window's buttons agreeing with it is the point of sharing the style. |
| Lighting when the scene moves under a still pointer | The lit button is worked out again before each frame, against the scene as it stands | Only on pointer motion | A window maximised from the keyboard takes its buttons from under the pointer; without this the button stays lit, under no pointer, until the mouse moves. |
| Lighting during a drag or a client's grab | None | Light whatever the pointer crosses | A press then belongs to the drag or to the client, so a lit button would offer a click that would not happen. |
| Measuring a title to cut it | The compositor's own font cache -- the faces that draw it | `guitk::text::fit_line`, as the request suggests | `fit_line` measures through the toolkit's process-wide font cache, which the compositor never sets up with the user's fonts; a cut measured in one face and drawn in another lands in the wrong place. The rule is the same -- clip, mark at the end, or mark at the start keeping the end -- so a title and its taskbar label are still cut alike. |
| The room a kept tail is cut to | Whole pixels, the same number the renderer is given | The geometry's exact width | A tail cut to fit a fraction of a pixel more than the renderer allows would be cut a second time at its end. |
| A window with no maximise button | Minimise moves up beside close | A gap where maximise would be | Lane C asked whether the gap was deliberate: it was not -- the compositor already closed it up (a comment in the old code says the gap would be a dead patch of title bar). |

### Revisit if

- A theme wants a resize band of its own (a theme value instead of the 6 px).
- The compositor's fonts and the toolkit's are ever one cache: then
  `fit_line` can do the cutting.

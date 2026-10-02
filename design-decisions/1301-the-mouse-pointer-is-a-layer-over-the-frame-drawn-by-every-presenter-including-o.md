## 1301. The mouse pointer is a layer over the frame, drawn by every presenter — including over fullscreen

**Date:** 2026-09-24
**Lane:** F
**Decided by:** Claude (autonomous). Two of the calls below touch an open
operator question (C-Q18) and another lane's settings; both are flagged in
`todo.txt` → `## Lane F` → Judgment Calls, and both are one-line reversals.

**In short:** SlateOS now draws a mouse pointer; it drew none at all before. The
pointer is drawn *on top of* each finished frame as it goes to the screen, the
way a graphics chip's cursor overlay works, rather than being painted into the
frame. That makes moving the mouse cost almost nothing, lets the pointer appear
over fullscreen games and video without slowing them down, and means switching
to a hardware cursor later changes one component. The pointer uses the default
size and colours for now, because the user's pointer-size setting exists in
three rival copies and the lane that owns them has asked that none be wired
until they are merged into one.

### The decisions

**A layer, not part of the picture.** `Present::show` takes a `Frame`: the
composited picture, a serial saying which picture it is, and the pointer
sprite. Alternatives:

| | moving the mouse costs | over direct scanout | a hardware cursor later |
|---|---|---|---|
| paint the pointer into the frame, with damage | re-rendering every window under the old and new pointer rectangles, every motion event | impossible — the frame is never composited (C-Q18's dilemma) | the pointer has to be taken back *out* of the pipeline |
| **a layer the presenter draws** | restoring and redrawing ~2,000 pixels | free on every presenter that exists: each already copies the frame | a change to the DRM presenter only |

The first is what software compositors without retained window textures
usually cannot afford: this compositor re-executes a window's render commands to
repaint it, so a pointer crossing a text-heavy window would re-shape text on
every motion event.

**Drawn over fullscreen too** (`POINTER_OVER_DIRECT_SCANOUT` in
`gui/compositor/src/server.rs`). C-Q18 asked the operator to choose between
losing the fullscreen shortcut (A), hiding the pointer over fullscreen (B) and a
hardware cursor (C). With the pointer as a layer the first cost disappears on
every presenter that exists, so the pointer is shown — the user-visible outcome
A and C share — and a game that wants no pointer asks for `CursorShape::Hidden`
over its window, which answers B's argument for games without B's cost to video
players. The dilemma returns only when a presenter scans a client's buffer out
*without copying it*; `requests/f-c-c-q18s-premise-changed-...` asks lane C to
defer C-Q18 to that trigger.

**The compositor describes the pointer; the server draws it.**
`Compositor::pointer` returns a `PointerState` — shape, hot spot, size, colours —
and `CursorCache` (owned by `Server`) rasterizes it. Keeping the art out of the
compositor keeps "what pointer is up" testable without pixels, and is the split
a hardware cursor needs.

**Vector artwork with a grown outline** (`gui/compositor/src/cursor.rs`). Every
shape is contours on a 32-unit grid, rasterized by the font engine's exact-area
rasterizer at any size; the outline is the body's coverage grown by one to two
pixels rather than a second drawn shape, so it stays concentric at every size.
Alternative: bitmap cursors per size, as X11 and Windows themes ship them —
sharper at their native sizes, and a new asset for every size and scheme.

**A frame serial, so a still picture is not recopied.** Presenters that keep a
copy of the picture (the DRM scanout buffers, the host window's staging buffer)
compare `Frame::serial` and, when only the pointer moved, restore the old
pointer rectangle from the picture and draw the new one. The serial is the
compositor's own count of frames produced, so it is right however
`compose_frame` was reached; `None` means "copy it" and is what a caller keeping
no count, a test above all, gets by default.

**The host window hides Windows' arrow and shows SlateOS's.** Windows' own
pointer would be smoother — it has no frame of latency — and would mean the
development harness never showed the pointer SlateOS draws.

**Default size and scheme, for now** (`Compositor::pointer_preferences`). Three
settings hold a pointer size (`appearance`, `inputsettings`, and the Settings
app's own, which it never saves), no control writes any of them where another
program can read it, and `TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-AND-NO-READER`
— lane C's, about lane C's models — says to collapse them before wiring any.
`requests/f-ce-the-pointer-is-drawn-now-which-cursor-size-setting-survives.md`
asks which survives. The size is still scaled for the display under the pointer,
so it is the right size on the glass.

### How it is held

Thirteen shapes at five sizes rasterize and wind correctly
(`cursor::tests`); the pointer's presence, hiding, scale and scheme colours
(`the_pointer_*`, `a_window_that_hides_the_pointer_*`); moving it composes
nothing; the server shows a frame for a pointer-only move with the picture's
serial unchanged, through the real loop as well as by hand; night light reaches
the pointer; and the DRM presenter's restore path, including a test that a
frame claiming an unchanged picture really does rewrite only the pointer's
rectangles, and a pointer straddling two monitors.

### How to reverse

The pointer over fullscreen: set `POINTER_OVER_DIRECT_SCANOUT` to `false`. The
user's settings: make `pointer_preferences` read the survivor. A hardware
cursor: the DRM presenter hands `PointerSprite` to `SYS_DRM_CURSOR_SET` instead
of blending it.

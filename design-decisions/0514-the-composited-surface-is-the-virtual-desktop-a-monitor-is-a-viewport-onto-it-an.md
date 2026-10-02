## 514. The composited surface *is* the virtual desktop; a monitor is a viewport onto it, and fullscreen covers one monitor rather than the surface

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** SlateOS's compositor already believed it could drive two monitors —
it kept a list of them, gave each an offset, and would happily place a window on
the second one. Nothing ever appeared there. The picture the compositor draws
into (the *scanout surface* — one big rectangle of pixels that the graphics card
reads out to the screen) was still the size of the first monitor alone, so every
window on the second screen was drawn past the end of it and thrown away. This
entry records the fix and the arrangement it commits to: **one picture covering
every monitor, with each monitor copying out its own rectangle of it.** The
alternative — one picture per monitor — is rejected below.

**Context: two rectangles that were allowed to disagree.** `DisplayManager`
tracks a list of `Display`s, each with an offset and a size, and answers
`virtual_bounds()` — the union of them, the whole desktop. `Compositor` draws
into `self.backend`, a framebuffer with its own width and height. Nothing tied
the two together. `add_display` extended the desktop and left the framebuffer
alone; `resize_display` resized the framebuffer from the *primary* monitor's new
size and so cut off every other monitor. A window at x = 900 on a second screen
starting at x = 800 was clipped away entirely by an 800-wide surface, and the
model reported it as visible on screen 2 the whole time. This is the worst shape
a bug can have: the system's own description of itself is confidently wrong.

Two consequences fell out of the same crack:

* **Fullscreen sized itself from the framebuffer.** `set_fullscreen` read
  `self.backend.size()`. On one monitor at the origin that is the monitor, which
  is why it went unnoticed for as long as it did. On two, a window fullscreened
  on the second monitor jumped to the first and spanned both — while
  `maximize_window`, one line away in the same file, asked `work_bounds_for` and
  correctly stayed put. Two commands a user thinks of as the same gesture
  disagreed about which monitor they act on.
* **The fullscreen re-fit after a mode change moved every fullscreen window.**
  `refit_fullscreen_windows(width, height)` took the resized framebuffer's
  dimensions and applied them to all of them, so changing the resolution of one
  screen dragged a game that was fullscreen on another onto it and told its
  client it was now a size it is not.

**Decision.** The scanout surface is exactly the virtual desktop's bounding box,
maintained as an invariant by the only two functions that can change either:
`resize_display` and a new `attach_display`. Both compute the *prospective*
desktop first, resize the surface, and only then adopt the new arrangement — so
a surface that cannot be allocated leaves the display list exactly as it was.
Fullscreen and the fullscreen re-fit both resolve against the window's own
monitor via `work_bounds_for`, the same function `maximize_window` uses; the two
now differ only in that maximising yields the panels their reserved strips and
fullscreen covers them.

**The alternative: one composed frame per monitor.** Each head gets its own
buffer at its own size, and the compositor runs its whole pipeline once per head
with that head's origin subtracted.

| | one frame for the desktop (chosen) | one frame per head |
|---|---|---|
| a window straddling the seam | one rectangle in one buffer; nothing special happens | composited twice, clipped differently each time; the two halves can tear against each other |
| the compositing pipeline | never learns what a head is | every rectangle, every damage region and every blit grows a head parameter |
| per-head scale factor / rotation | **not possible** — one surface has one pixel grid | natural: each head composites at its own scale |
| an L-shaped or vertically-offset arrangement | composites a gap that is scanned out nowhere | costs nothing |
| memory | one buffer, bounding-box sized | N buffers, exactly sized |
| damage tracking | one region, already implemented | one region per head, and a window moving between heads dirties both |

The gap is the honest cost and it is bounded by the arrangement the user chose —
two 1920x1080 screens offset vertically by 200 px waste 1920x200 of clear per
frame, which is a `memset`, not per-window work. Per-head scaling is the real
limitation, and it is deferred rather than denied: it is a change to the scanout
and the input mapping, not to this decision, because a scaled head can blit
through a filter on the way out. Against that, the per-head design pays on every
frame of every window, forever, for a capability that only matters when the two
monitors have different pixel densities.

The deciding argument is which shape makes the *wrong* answer impossible. With
one desktop-sized surface, "is this window visible?" has one answer, computed in
one place, in one coordinate system — the virtual desktop's. That is the
coordinate system the input events, the window rectangles, the snap zones and
the panel reservations are already in. Adding a second one, per head, would give
every existing test a chance to be right about the wrong space.

**The surface's origin is the desktop's origin**, so a display at a negative
offset would be unaddressable. Nothing produces one — `add_display` only ever
places to the right of what is already there — and `resize_scanout_surface`
takes `right()`/`bottom()` rather than `width`/`height` precisely because those
are the extent that has to exist and they agree with the size only while the
origin is where it is documented to be. If dragging monitors into arbitrary
positions is ever added, that is the line that has to change: the surface would
need to be sized from the bounding box and every window rectangle offset by its
origin.

**The direct-scanout bypass is now, correctly, a single-head optimisation.** It
hands one client's buffer to the scanout entire, so on a two-headed desktop it
would put the game's pixels on the *other* monitor in place of the desktop. The
guard that prevents this is the pre-existing "the window must cover the whole
framebuffer" test, which a one-monitor fullscreen window no longer satisfies —
so the fix costs nothing and the test that pins it
(`the_direct_scanout_bypass_declines_a_second_monitor`) exists to record that the
guard is load-bearing rather than incidentally true.

**A latent defect found on the way, and fixed here.** `Rect::union` and
`Rect::intersect` computed their far edges as `self.x + self.width as i32`.
`u32::MAX as i32` is `-1`, so a rectangle wider than 2^31 put its right edge to
the *left* of its own origin: the union of an over-wide rectangle with a normal
one came out **narrower than either input**, and the intersection of one with
anything came out empty. `Rect::right()` and `Rect::bottom()` had already been
written to saturate instead, with a doc comment explaining this exact hazard, and
the two functions simply did not use them. They do now. This was not theoretical
— it was found because `a_surface_that_cannot_be_allocated_leaves_the_arrangement_alone`
passed a `u32::MAX`-wide display and the surface allocation *succeeded*, at
800x768, because the union had shrunk.

**Verification.** 10 new tests, each proved to be a regression test by
reintroducing the defect it names through the guarded patcher and confirming a
deterministic failure that names it back: `fullscreenisframebuffer`,
`refitallfullscreen`, `surfaceisprimary`, `resizesurfaceisdisplay`,
`rectunioncast`, `rectintersectcast`, plus the pre-existing
`resizenofullscreen`. Five markers from §512 (`resizenoretile`,
`resizenofullscreen`, `resizenoresizeevent`, `resizenorescue`,
`resizenopointer`) were repaired against the refactor, since a marker that no
longer applies is a proof that has quietly stopped being run.

**Addendum, 2026-08-21 — the eleventh test.** Closing
`TD-C-FULLSCREEN-IGNORES-WHICH-MONITOR-AND-WHAT-IS-RESERVED` (which this entry
had in fact already fixed) turned up one property from its own checklist with no
test behind it: that fullscreen resolves the monitor's **bounds** where maximize
resolves its **work area**. Every fullscreen test here ran on a screen with
nothing reserved, and on such a screen `work_bounds_for` and `work_area_for`
return the same rectangle — so routing `set_fullscreen` through `work_area_for`,
which is the tidy-up an unwary reader would make since every *other* tiling path
does exactly that, passed all 427 of them while leaving a band of taskbar across
the bottom of every full-screen video. `fullscreen_covers_the_taskbar_that_maximize_stops_at`
asserts both halves in one test — maximize stops at `screen.bottom() - 40`, then
the same window fullscreened covers the whole 800x600 — so the contrast cannot
decay into a tautology if the fixture ever loses its reservation. Proved by the
marker `fullscreenspareasthetaskbar`, and it failed **alone**, which is the
evidence that the gap was real rather than incidentally covered elsewhere.

One new test — `leaving_fullscreen_on_the_second_monitor_stays_on_it` — is
**additional coverage rather than the sole guard on a new invariant**: the
fullscreen round trip is lossless whether fullscreen sizes from the monitor or
from the framebuffer, so it survives every marker above. It is kept because it
pins the interaction between §513's restore-reachability rule and multiple
monitors, but it is recorded here as what it is, for the same reason §513
recorded three of its own.

**What this does not do.** The scanout still drives one head: `DrmScanout` picks
one CRTC and one connector, so the second monitor is modelled, composited and
then not scanned out. That is the next commit — a `Head` per CRTC, each with its
own buffer pair and its own source rectangle in this one frame — and it is now a
mechanical follow-on rather than a redesign, which is the point of doing the
model first.

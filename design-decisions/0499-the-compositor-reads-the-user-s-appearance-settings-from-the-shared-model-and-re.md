## 499. The compositor reads the user's appearance settings from the shared model, and reads the whole of it

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The Settings app has a switch for how round window corners should
be — Square, Subtle, Rounded, Extra Rounded — and another for whether windows
cast shadows. The program that actually draws window frames, the compositor, had
never been told about either one. So all four corner choices produced the same
square window, and turning shadows off left every shadow exactly where it was.
The decision here is *how* the compositor learns those settings: it depends on
the same small crate the Settings app writes them with, and holds the user's
whole settings record rather than copying out the two fields it can act on
today.

### The problem

`gui/appearance` exists specifically so that one user preference has one owner.
Its own module doc says so: the shell paints from these values, the Settings
application edits them, and both read and write the same `appearance.yaml`,
because two crates that disagree about what "Rounded" means corrupt the user's
settings between them.

The compositor was outside that arrangement entirely. It drew every frame from a
hardcoded `DecorationTheme` — twelve colour constants and nothing else — and had
no dependency on `appearance` at all. Two of the settings in that file are
*about* what the compositor draws:

| Setting | What the user expects | What happened |
|---|---|---|
| `window_corners` | Square / Subtle (4px) / Rounded (8px) / Extra Rounded (16px) | every window square, all four choices identical |
| `drop_shadows` | windows cast a shadow, or don't | shadow always drawn |

This is the same shape of fault as §498 one layer up, and it was hidden the same
way: a square window looks perfectly plausible. Nothing about the picture said a
setting was being ignored.

### Decision 1 — depend on `gui/appearance`, rather than passing the two values in

The alternative was to keep the compositor free of the dependency and have
whoever starts it pass in a radius and a boolean — two plain arguments, no new
crate edge.

*What changes:* nothing visible. The difference is where the meaning of
"Rounded" lives.

| | Depend on `appearance` | Pass a radius and a flag |
|---|---|---|
| Where `Rounded == 8px` is decided | once, in `appearance` | in whoever calls, and it must agree |
| New crate edge | yes (`compositor` → `appearance`) | no |
| Cost of adding the *next* setting | a field read | a new parameter through every caller |
| Failure mode | none obvious | the display server and the settings panel disagree about a value the user picked |

The dependency is safe: `appearance` depends only on `guitk` and `yamldoc`, both
of which the compositor either already has or does not conflict with, so there
is no cycle. And the alternative's failure mode is precisely the one
`known-issues.md` `TD-THREE-INDEPENDENT-APPEARANCE-MODELS` was filed about.

### Decision 2 — hold the whole `AppearanceSettings`, not the two fields used

The compositor can act on two of that struct's sixteen fields today. Copying
those two into compositor-local state (`corner_radius: f32`, `shadows: bool`)
would be a smaller footprint and would make the struct's other fourteen fields
obviously irrelevant to this process.

*What changes:* nothing visible today; it decides what the *next* setting costs.

Holding the whole record wins because the two-field version is a third
independent appearance model in miniature — a compositor-local copy of settings
whose canonical form lives elsewhere, which is the exact thing `appearance` was
created to stop. The twelve colours in `DecorationTheme` are the next things
that should come from this record (they currently duplicate the Catppuccin
palette by hand), and they will be a field read rather than four more
parameters.

### Decision 3 — the corner radius scales with the display, and has no non-zero floor

Every other decoration dimension goes through `scale_dimension`, which
deliberately never rounds a visible dimension away to nothing: a 1px border at
0.6× must not become a 0px border, because hit-testing derives from the same
number and a 0px border is a window that cannot be grabbed to resize.

The radius is scaled the same way but must *not* inherit that floor. A radius of
zero is not a rounding accident — it is the user having chosen `Square`, and it
has to survive scaling as a square corner rather than being promoted to a 1px
curve. So `decoration_radius` multiplies and clamps to zero, and does not borrow
`scale_dimension`'s minimum.

### Decision 4 — a maximized window casts no shadow

*What changes:* a maximized window loses a dark smear along its top and left
edges (visible only when it is translucent), and stops doing a full shadow's
worth of stroking every frame.

A maximized frame is fitted to the display exactly (`maximize_window` →
`client_geometry_for_frame(display_bounds)`), so every ring of its shadow is
either clipped off the display or drawn *underneath the window's own frame*.
On an opaque window it is therefore invisible — pure overdraw, eight stroked
rounded rectangles per frame for nothing, on the one window state that is the
common case. On a translucent or `transparent` window the frame does not cover
what is beneath it, so the rings show through as a dark smear along the top and
left and nowhere else, which is worse than no shadow.

Worth recording precisely because the reasoning is *not* the shell's. The
shell's duplicate decorator suppresses the same shadow with the comment that one
drawn anyway "would bleed over the screen border" — true of its `BoxShadow`,
which fills, and not true of the compositor's rings, which do not. Two correct
decisions for two different reasons; a future reader who assumes they are the
same case will draw the wrong conclusion about one of them.

Fullscreen needs no such test: `Window::has_title_bar` is `decorations &&
!fullscreen`, so a fullscreen window never reaches the decoration path at all.

### Decision 5 — adopting settings forces a full recomposite

*What changes:* the setting takes effect on the next frame instead of whenever
something else happens to repaint that part of the screen.

The compositor normally redraws only damaged rectangles, and damage is generated
by windows moving, resizing and submitting. An appearance change generates none
— no window moved — yet it changes pixels *outside* every window's extent: the
quarter-disc of frame colour a squared corner reclaims, and the strip of desktop
a removed shadow vacates. So `set_appearance` sets `full_recomposite`. Without
it the user drags the corner-style control and the screen does not change, which
reads as the control being broken rather than as a stale frame.

### Where the settings enter the process

`main` reads `appearance.yaml` at startup and calls `set_appearance`;
`Compositor::new` deliberately does not. A constructor that consulted `$HOME`
would make every test in this crate depend on the machine running it — the same
split `AppearanceFile::new` vs `AppearanceFile::load` already draws, for the same
reason.

**This is a startup-only channel, and that is a known gap, not a decision.**
Changing a setting in the Settings app does not reach a running compositor; the
user must restart it. The fix is a protocol verb, and the shape it should take
is already clear: a `ReloadAppearance` request carrying *no data*, so that the
compositor re-reads the user's own file rather than being told what to draw. A
setter would let any connected client restyle the whole desktop; a reload
notification lets a hostile client at worst force a redundant re-read of a file
it cannot write. Tracked in `known-issues.md`.

### Verification

Ten tests, and all ten were proved regression tests by reintroducing the bug
each one names and confirming a deterministic failure that names it back:
storing the settings without reading them; giving every window one hardcoded
radius; dropping the display scale from the radius; filling the title bar
square; leaving the buttons square; tracing a square border around a rounded
frame; keeping square corners on the shadow's rings; drawing the maximized
shadow; ignoring the `drop_shadows` flag; and dropping the forced recomposite.
**10/10 caught on the first pass.**

Two of the tests were wrong before they were right, and both errors are worth
recording because neither would have announced itself:

- **A corner test that asserted an absolute pixel count.** "A square corner
  paints its whole 20×20 corner block" is false: it paints 339 of 400, because
  the block also contains the border stroke down its left edge and the first
  letters of the window title. The count is a sound *relative* measure — the
  contaminants do not move with the corner setting — and an unsound absolute
  one. The test now probes a single pixel two in from the corner, which is
  inside the border and clear of the text, and the helper's doc says why an
  absolute assertion is not available.
- **A test that was reading the vsync gate instead of the damage state.**
  `compose_frame` declines both when there is nothing to draw *and* when it is
  too soon for another frame, and three calls in a row at 60 Hz are all inside
  one 16 ms interval. The test's middle assertion — "an unchanged desktop has
  nothing to redraw" — was passing for entirely the wrong reason, and its last
  one failed for that reason too. It now runs on a zero-length frame budget so
  that what it measures is damage.

One thing deliberately *not* claimed as tested: the shadow's rings grow their
radius by their own distance out, so that the shadow is an offset curve rather
than a stack of same-shaped outlines. Removing the growth leaves the rings
rounded, just increasingly square-shouldered further out, and no test here
distinguishes that from the correct shape. It is a real property with no guard.

# C → F — draw every program at the display's scale, as the operator chose

**From:** lane C. **To:** lane F (`gui/compositor`, `gui/remote`, `gui/window`).
**Filed:** 2026-10-12. **Status:** OPEN.

## In short

On a display set to 150% or 200%, programs' contents stay at 100%
(`known-issues/TD-C-PROGRAMS-DRAW-AT-ONE-SCALE-WHATEVER-THE-DISPLAYS-IS.md`).
The operator chose where the enlarging happens: in the compositor, once,
for every program -- C-Q34, answered A, recorded as
`design-decisions/1609-the-compositor-draws-every-program-at-the-display-s-scale.md`
-- with the per-element control they asked about. All of it is the
compositor's and the protocol's, so it is yours.

## What is asked

1. **A program's drawing list at the display's scale.** A program lays out
   in logical pixels; the compositor multiplies what it draws -- text shaped
   and rasterized at the screen's resolution, shapes drawn at it, so both
   stay sharp -- and divides pointer positions before passing them on.
   A window moving to a display of another scale is simply drawn again.
2. **An image may carry its own density.** A picture uploaded at 2x for a
   logical rectangle is drawn 1:1 at 200%, not enlarged again (the browser's
   `srcset`, macOS's `@2x`): a density on the upload or on the image command.
3. **A surface may ask for the screen's real pixels** -- a game, a video, a
   photo shown at 100% -- and is then not scaled; the program is told the
   scale so it can lay out itself.
4. **Programs told the scale**, so a program that wants to upload pictures at
   the density they will be shown at can.

## What lane C does once it lands

The desktop stops enlarging its own surfaces itself (it scales its taskbar,
menus and frames by the display's scale today) and draws in logical pixels
like any program; the toolkit uploads its pictures at the display's density.
Reply here, or with a notice, when it is on `main`.

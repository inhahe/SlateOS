## 822. Applications receive the palette through a trait method, not an event

**Date:** 2026-09-08. **Lane:** C.
**Decided by:** Claude (autonomous). An internal API shape with no
user-visible fork in it; recorded because it is the seam 135 applications will
be converted against, and because the rejected option is the one that looks
more obvious.

**In short:** applications draw their own insides, so until now the user's
theme reached the window frame and stopped. Applications are now handed the
colours by the framework that runs them. The question was *how* — as an event
alongside mouse clicks and key presses, or as a method on the application
trait. It is a method.

**The problem.** 129 of the 135 applications that draw never mention
`appearance::Palette`; each carries its own hardcoded Catppuccin Mocha
(`known-issues.md`
`TD-C-129-OF-135-APPLICATIONS-IGNORE-THE-THEME-ENTIRELY`). That is not 129
oversights: `gui/window` did not depend on `appearance`, so an application
that wanted to follow the theme had to find and parse `appearance.yaml`
itself, and none did. Both halves of the route were missing — no palette, and
no notification that one had changed.

**Option A — a new `Event` variant.** *What changes:* a theme change arrives
the way a resize does, through `on_event`.
- **For:** applications already have exactly one place where things happen to
  them, and this is a thing that happens to them.
- **Against, and decisive:** `Event` lives in `guitk`, which deliberately owns
  no palette. §810 deleted that crate's theme system and left a test
  (`this_module_names_no_colours_of_its_own`) that fails if a colour literal
  returns to it, because a colour written there is beyond the reach of the
  light/dark switch and the contrast sweeps. Putting a `Palette` in
  `guitk::Event` gives `guitk` the dependency §810 removed.
- Also against: it is not an input event. Nothing about it is the user acting
  on this window, and every application that matches exhaustively on `Event`
  would grow an arm for something that is not one.

**Option B — a defaulted method on `App` (chosen).** `fn theme_changed(&mut
self, palette: &Palette)`, called once before the first frame and again
whenever `appearance.yaml` changes.
*What changes:* an application that wants the theme stores the palette and
draws from it; one that does not is untouched.
- **For:** only `gui/window` gains the `appearance` dependency, which is the
  crate that already owns the application's lifecycle. Defaulting to nothing
  is what allows 129 conversions one at a time rather than one commit
  touching every application in the tree.
- **Against:** two ways for something to reach an application — events and
  this. Accepted: the alternative is one way that drags a palette into the
  toolkit.

**Two details that are easy to get wrong, both now tested.**

- **Before the first frame, not after.** The other order is a visible flash of
  the wrong theme every time a window opens: one frame in the application's
  default, corrected on the next.
- **A rewrite that changes nothing is not a change.** `Watcher` reports that
  the file was written; whether it *says* anything different is this layer's
  question. Saving a settings window with nothing altered would otherwise
  repaint every window on the desktop.

**Where the poll happens.** At the batch boundary, not per event: a theme
change is not something the user did to this window, and re-reading a file
once per mouse move to answer "no" is the cost `Watcher` exists to avoid. A
change marks the frame dirty itself, because an application that repaints only
on input would otherwise keep the old colours until something was clicked.

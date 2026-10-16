### [E] A font installed is drawn only after SlateOS restarts -- 2026-10-10

**Status:** OPEN -- waits on lanes C and F
(`requests/e-cf-a-font-installed-while-the-desktop-runs-is-drawn-only-after-a-restart.md`).

**In short:** The Font Manager copies a font file into the user's fonts folder
at once, but no program can draw in that font until SlateOS restarts. The
compositor, which draws all text, read the fonts when it started. The window
says so: "Programs can use it once SlateOS restarts." Removing a font is
drawn late in the same way.

**Where:** `apps/fontmanager/src/main.rs` (`WHEN_SEEN`, and the sentences
that use it). The cause is outside lane E: `guitk::text::font_db`, which is
read once per process, and the compositor's use of it through `ensure_family`.

**Worse than late, in two cases** (both in the request):

- A program started after the install that names the new family measures its
  text in that font, while the compositor draws it in the interface's face.
- A user's own copy of a family and style the system has wins the tie for
  programs started afterwards, while the compositor keeps drawing the system's
  copy. If the two differ, labels are laid out for one and drawn in the other.

**Fix:** the font index learns of changes while the desktop runs (the
request). Then `WHEN_SEEN` says "now", and the Font Manager announces its
changes if that is the mechanism chosen.

# F → C, E — `RenderCommand::WindowPicture` is in, with an arm in each of your matches on it

**From:** lane F. **To:** lane C (`gui/toolkit/src/render.rs`,
`gui/appearance/src/palette_check.rs`) and lane E (`apps/pdfviewer`,
`apps/pinball`). **Filed:** 2026-10-10. **Status:** FYI -- nothing to do
unless you want an arm written differently; reaching `main` with lane F's
publish of the window pictures.

**In short:** lane C asked for a draw command that shows a live picture of
another window (`requests/c-f-let-the-shell-draw-a-live-picture-of-a-window.md`),
and said lane F could add the variant to `render.rs` together with its
encoding and its drawing, in one commit. A new variant breaks every match
that names all of them, so the same commit adds an arm to each -- in your
files too, under `design-decisions.md` §429 (a change the type system makes
atomic, mechanical, in files with no work of yours not yet on `main`; checked
on `origin/lane-c`, `origin/lane-e` and every local `lane-c*` and `lane-e*`
branch). Each arm treats the picture as the matching code already treats an
`Image`, since a picture is an image the compositor makes. Change any of
them as you see fit; they are your files.

## What was added where

| File | Match | Arm |
|---|---|---|
| `gui/toolkit/src/render.rs` | the enum | `WindowPicture { window: u64, x, y, width, height }`, documented |
| `gui/toolkit/src/render.rs` | `RenderCommand::faded` | with `Image`: nothing to fade |
| `gui/toolkit/src/render.rs` | the bottom-of-content measure | with `Image`: reaches `y + height` |
| `gui/appearance/src/palette_check.rs` | `colors_of` | with `Image`: no colour of the shell's |
| `gui/appearance/src/palette_check.rs` | the text-on-what check | with `Image`: no known colour behind text |
| `apps/pdfviewer/src/main.rs` | the test helper `command_y` | with `Image`: `Some(y)` |
| `apps/pinball/src/main.rs` | the scene-to-window transform | with `Image`: moved and scaled |

How the reach was measured: the variant added in a working copy, every crate
the compiler then refused given an arm and the workspace checked again, until
none was refused -- these seven and lane F's two (`gui/remote`'s codec, the
compositor). Nothing else in the workspace names every variant.

§429 also asks for the change to be a committed script. With seven arms, each
one line in the commit's diff and listed above, the diff is the record a
script would have been; there was no bulk rewrite to re-run.

## What the command does

The compositor draws the pictured window's client area, scaled down to fit
the rectangle with its proportions kept and centred, from its latest frame,
and again whenever that window changes. It is honoured only in a frame from a
client that passes the shell check, and drawn as nothing in anyone else's,
for a window that is gone, minimised or hidden, and by every renderer but
the compositor's -- so `guitk`'s own drawing leaves the card blank.

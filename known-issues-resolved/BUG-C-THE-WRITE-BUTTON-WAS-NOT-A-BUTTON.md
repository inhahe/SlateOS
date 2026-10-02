## BUG-C-THE-WRITE-BUTTON-WAS-NOT-A-BUTTON — fixed in `c9c7902bd`

**In short:** The disk imager's whole purpose is to write an image to a USB
drive. The "Write Image" button that starts that had been on screen since the
tab was written, and clicking it did nothing at all — no handler anywhere
listened for it. The program could read images, inspect them, hash them and
verify them, but it could not do the thing it is named after.

**Where it lived:** `apps/diskimager/src/main.rs` — `render_write_tab` drew the
button; `handle_mouse` had branches for the tab bar, the drive list and the ISO
browser, and none for the Write tab.

**What it took down with it.** The button was the only way in, so everything
behind it was unreachable too: `ConfirmDialog` and its `show_write_confirm`, the
140-line `render_confirm_dialog`, and the Enter/Escape handling in
`handle_key`. Roughly 200 lines of tested-looking, never-run code — two unit
tests exercised `ConfirmDialog` as a struct, which is why "it has tests" did not
mean "it works."

**Why nothing caught it.** `dead_code` is the lint that catches a feature with
no way in, and it could not fire: the dialog was reachable *from the tests*, and
the tests are compiled into the same crate. A crate-wide `#![allow(dead_code)]`
had also been present until 2026-08-25. Nothing else in the toolkit can notice —
a `FillRect` is a `FillRect` whether or not anyone will ever click it.

**The fix:** a `WriteTabLayout` computed once, drawn from by `render_write_tab`
and hit-tested by `handle_mouse`; the rows below the image and drive cards move
with the selection, so the button's position is not a constant a hit test could
have restated. `confirm_write()` puts the dialog up, guarded by the same
`can_write()` that decides whether the button is drawn enabled — one test, so
the button's look and its effect cannot disagree.

**The lesson worth keeping:** a widget that is drawn but not wired is invisible
to every automatic check there is. The test that catches it is the one that
clicks where the pixels are — `the_write_button_opens_the_confirmation` —
paired with one asserting the drawn rectangle is the hit-tested rectangle. Both
now exist; either alone would pass while the feature stayed broken.

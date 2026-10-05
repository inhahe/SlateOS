## B-IMAGEVIEWER-A-FILE-IT-COULD-NOT-OPEN-LEFT-THE-PREVIOUS-PICTURE-ON-SCREEN (lane C, 2026-08-16) — FIXED

**In short:** Open a photo, then open a second file the viewer cannot read — a
damaged download, a file on a disconnected drive, a name typed slightly wrong.
The viewer showed the **first** photo still, but labelled it with the **second**
file's name, and told you its size and dimensions were the first photo's. There
was no message anywhere saying the second file had failed to load. So the
program confidently showed you the wrong picture under the right name, which is
worse than showing nothing: you would have no reason to doubt it. Running
`imageviewer sometypo.jpg` from a command line was the same story — it printed
nothing, opened an empty window, and reported success.

**Where:** `apps/imageviewer/src/main.rs`, `ViewerState::display_image`, and the
argument handling in `main`.

**What was wrong**

`display_image` mutated the existing `self.image_info` one field at a time and
never touched `self.current_image` on the failure path:

- When `std::fs::read` failed it set the filename and returned, leaving
  `current_image` holding the previous file's decoded placeholder and
  `image_info` holding the previous file's `file_size`, `width`, `height`,
  `format` and `date_modified`.
- Even when the read *succeeded*, any field the new file did not supply kept
  the old file's value, because the struct was updated rather than rebuilt. A
  readable file whose dimensions could not be parsed therefore displayed the
  previous image's dimensions. This is the same bug in a second place, and it
  is the half that survives any fix aimed only at the read error.
- `display_image` returned nothing, so no caller could tell a load had failed.
- `main` guarded on `path.exists()` with no `else` branch. Existence is not the
  question — a path can exist and still be unreadable (permissions, an I/O
  error, a directory) — and when it did not exist the program said nothing at
  all and exited 0.

**The general shape.** This is the same rule the rest of this sweep has been
following, in its display form: *a field that is not overwritten is not blank,
it is stale.* Building a fresh value and assigning it once makes "I did not set
this" and "this is empty" the same state; updating in place makes them silently
different. The failure path is where they diverge, and the failure path is the
one nobody looks at.

**The fix**

- `ImageInfo` is built fresh from `ImageInfo::default()` on every call, so no
  field can survive from the last image, and assigned to `self.image_info` in
  one move.
- On any read failure `current_image` is cleared, the transform is reset, and
  the reason is stored in a new `ViewerState::load_error`.
- `render_image` draws that reason in place of the picture — "Cannot display
  this image" with the underlying error beneath it, elided to the canvas width
  — instead of the "No image loaded / Open a file or drag an image here"
  welcome text, which is a lie once a file has been chosen.
- `display_image` and `open_file` return `bool`. `main` reports the reason on
  stderr and exits 1. Directory navigation deliberately ignores the return
  (`let _ = self.display_image(&path)`) with a comment: arrowing through a
  folder must not stop dead at one bad file, it must show the failure and let
  you keep going.

**Tests** (`apps/imageviewer/src/main.rs`, 4 new, 103 pass):
`a_file_that_will_not_open_does_not_keep_the_last_image_on_screen`,
`a_failed_load_renders_its_reason_rather_than_the_welcome_text`,
`a_successful_load_clears_the_previous_failure`,
`navigation_onto_an_unreadable_file_shows_no_stale_dimensions`.

**Verified by injection.** Restoring the read-error path reddens three of the
four; restoring the field-by-field assignment reddens two (one of which the
first injection does not touch, because its trigger file *is* readable). No
pre-existing test fails under either — which is exactly why this shipped: every
test anyone had written opened a file that worked.

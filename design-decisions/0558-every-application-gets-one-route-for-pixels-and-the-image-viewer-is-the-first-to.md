## 558. Every application gets one route for pixels, and the image viewer is the first to use it: one id reused, a queue that supersedes, and a window that opens even when the file will not

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the image viewer now actually shows the photograph you opened.
Until this change it drew a grey checkerboard for *every* file — a comment in
the source said "in a real implementation, this would decode the image" — and
the whole test suite passed over it, because nothing anywhere in the tree could
turn a file into pixels. Four things had to be settled. (1) Applications had no
way at all to hand pixels to the display server: the one method they implement
returns a *drawing* (which can name a picture by number but never carries the
picture), so a new hook was added to the shared application strap. (2) That hook
takes one ordered list of "start holding this / stop holding this" rather than
two separate methods, because the order between the two matters. (3) The viewer
reuses a single picture number for every file it opens, instead of one per file.
(4) A file the viewer cannot read no longer stops it from opening its window —
it opens, and says on screen what went wrong.

### Decision 1 — the hook lives on the `App` trait, drained per *frame*

`oswindow::app::App` is the strap ~140 application crates share: connect, open
one window, run until it closes. Its `render` returns a `RenderTree`, and a
`RenderTree`'s `RenderCommand::Image` carries an image *id* — a number the
compositor looks up stored pixels under — and never the pixels themselves.
Uploading is `WindowHandle::upload_image`, and an app implementing `App` never
sees a `WindowHandle`. So there was no route, for any application, at any price.
`App::take_images() -> Vec<ImageChange>` is that route.

*Alternative considered:* hand `render` the window handle, or the event loop, so
an app can upload inline while drawing.

*Why not:* it makes `render` a method that can perform I/O and fail, which
propagates a `Result` and a transport type parameter into every one of those 140
`render` implementations, and it makes every render test need a connection.
Today they are pure `(width, height) -> RenderTree` functions testable with no
compositor at all, and that is the property that makes the app tests in this
tree runnable on a build machine. The drain-a-queue shape keeps it.

*Cadence:* drained immediately after `render` and before the frame is submitted,
**once per frame** — not once per event, which is what the existing
`take_reloads` does. The two differ for a reason worth recording: a settings
file rewritten by the last click before a window closes must be announced even
though no frame follows, so reloads drain per event; a picture with no frame
behind it has nothing to be early for, and an upload still queued at exit is
freed with the window. Draining images per event would mean uploading pictures
for frames that never get drawn.

*Cost accepted:* an application that queues a picture and then never renders
again never uploads it. This is the correct outcome — that picture had no frame
naming it — but it is a real difference from `take_reloads` and is documented at
both methods.

### Decision 2 — one ordered list, not `take_uploads()` + `take_drops()`

*Alternative considered:* two methods, which is the more obvious API and needs
no enum.

*Why not:* §557 established that a slideshow must drop the outgoing picture
*before* uploading the incoming one, or the per-connection image budget of §556
is charged for two full-screen pictures at once and refuses every slide after
the first. Two methods cannot express "this drop precedes that upload" — the
caller has to pick an order between the two calls, and whichever it picks is
wrong for someone. One `Vec<ImageChange>` makes the ordering the application's
to state and the loop's to preserve, and the test that guards it
(`a_drop_queued_before_an_upload_goes_out_before_it`) is order-sensitive rather
than a presence check.

### Decision 3 — the viewer reuses one image id for every file

`ImageData` used to carry an id hashed from the file's path, so each picture got
its own number.

*Alternative considered:* keep per-path ids, so a picture already uploaded could
be re-shown without decoding again.

*Why not:* nothing evicts. The compositor holds what it is given until the id is
dropped or the window closes (§556 refuses rather than evicting), so per-path
ids mean a session that pages through a directory of 400 photographs holds all
400 in the compositor and is refused somewhere in the middle. A cache would need
its own eviction policy, its own budget accounting, and a way to know the
compositor agrees with it — three new mechanisms to save a decode that takes
milliseconds.

*What one id buys, exactly:* re-registering an existing id is arithmetically a
*replacement* in the compositor's budget check (`gui/compositor/src/wire.rs`,
`image_budget_refusal`: `after = held - freed + incoming`, where `freed` is what
that same `(window, id)` already holds). So a single fixed id gives §557's
never-hold-two property for free, with no explicit drop at all — the wallpaper
had to arrange it by hand because it genuinely uses two ids.

*Cost accepted:* pressing Left then Right decodes the same file twice. A PNG
decode is bounded and fast, and the alternative is a cache that can disagree
with the compositor about what is stored.

### Decision 4 — the pending queue is *cleared* before the new picture is pushed

`display_image` does `pending_images.clear()` and then pushes. A user holding
the arrow key crosses a directory faster than the loop draws frames, and an
appending queue would upload every file passed over — each one immediately
replaced by the next, so every upload but the last is pure cost. Because the id
is fixed (Decision 3) the superseded entries are provably redundant: they all
write the same slot. Failure does the same thing in reverse — `fail_with` clears
the queue and pushes a single `Drop`, so a file that will not open stops paying
for the picture that is no longer on screen.

### Decision 5 — a file that cannot be shown still opens a window

`main` used to `process::exit(1)` when the file named on the command line could
not be read, with a comment saying so.

*Alternative considered:* keep exiting, on the grounds that a viewer with
nothing to view is pointless.

*Why not:* that argument was made when there was no window at all — the program
was a render-tree factory with a `main` that printed. Now `render_image` has a
purpose-built "Cannot display this image" state that names the file and the
reason, and `open_file` builds the directory listing whether or not the file
decodes. So a user who double-clicks a corrupt photograph gets a window that
says which file is broken and an arrow key that carries them off it, instead of
a window that flashes and vanishes with a message on a stderr nobody is reading.
The reason is still printed to stderr for a shell user, and the exit code is
still non-zero only for *usage* errors (an unparseable argument, more than one
file), which are the ones a script can act on.

### Decision 6 — "no decoder claims these bytes" is not the same message as "this is not a picture"

`imagecodec::decode` answers `UnknownFormat` both for a text file and for a
JPEG, since only PNG is implemented. `ImageFormat::detect` in the viewer reads
the signature *independently* of the decoder, so the viewer can tell the two
apart: signature says JPEG **and** no decoder claims it means *unsupported*, and
`decode_failure` says "JPEG images cannot be displayed yet". Reporting the
decoder's own wording — "not a picture format this system reads" — for a valid
photograph would send a user looking for a corrupt disk.

### Where it lives

`gui/window/src/app.rs` (`ImageChange`, `App::take_images`, `apply_images`, the
five tests under "Getting a picture's pixels to the compositor"),
`apps/imageviewer/src/main.rs` (`VIEWER_IMAGE_ID`,
`ViewerState::pending_images`, `display_image`, `fail_with`, `decode_failure`,
`impl App for ViewerState`, `main`, and the eight tests under "Getting the
picture to the compositor"), `apps/imageviewer/Cargo.toml` (the `imagecodec` and
`oswindow` dependencies).

**Test-fixture note.** `png_bytes` in the viewer's tests used to emit a bare
13-byte header, which was sufficient *precisely because* nothing decoded. It is
now a real encoder — stored deflate blocks, CRC-32 per chunk, Adler-32 over the
stream — so the four scratch-directory tests that were already written kept
their exact signatures and now run against a genuine picture rather than a stub.
Upgrading the fixture rather than weakening the new tests is the whole reason
`the_pixels_are_the_files_own_and_not_a_pattern` can assert a specific pixel's
bytes.

**Numbering note.** Lane C's allocated band ended at 499; this continues the
documented overflow past it.

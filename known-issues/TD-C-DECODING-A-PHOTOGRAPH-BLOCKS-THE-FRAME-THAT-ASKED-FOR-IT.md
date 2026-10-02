## `TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT` (lane C, 2026-09-17) — FIXED 2026-09-26
**Status:** FIXED 2026-09-26 (closed by lane C, whose entry it was). Every decode this entry names is off the thread that draws: the shell's wallpaper and login picture (lane C, `gui/desktop/src/pictures.rs`, design-decisions §882), and the applications' -- `apps/imageviewer`'s picture, `apps/photomanager`'s photograph and thumbnails, and `apps/explorer`'s thumbnails (lane E, through `apps/offloop`; `requests/f-ce-a-finished-decode-can-now-wake-the-window-that-asked-for-it.md`). How it got there: — 2026-09-24 (lane F): the stall is about 3.5x shorter, not gone. `imagecodec`'s JPEG decoder is now about 3.3x faster with bit-identical output (4000x5333: whole picture 3.65 s → ~1.1 s, 128-px thumbnail 1.25 s → ~0.34 s, release, this machine), but the decode still runs on the thread that draws; moving it off that thread is still the fix. — 2026-09-25 (lane F): **the missing piece this entry names, the wake, now exists.** An application that returns `true` from `App::wants_waker` is handed a `std::task::Waker` in `App::attach_waker` before its first frame; a worker that calls `wake()` gets the application an `App::on_wake` on the loop's thread, then a frame (`EventLoop::waker`, `Dispatch::Woken`; design-decisions §1303). What remains is lane E's: moving the two decodes onto a worker — `requests/f-ce-a-finished-decode-can-now-wake-the-window-that-asked-for-it.md`. — 2026-09-26 (lane C): the desktop shell had the same stall, never listed here, for its wallpaper and the login screen's picture -- at login, on every wallpaper chosen and at every slideshow step. Fixed: they decode on a thread of the shell's own, woken through `EventLoop::waker` (`gui/desktop/src/pictures.rs`, design-decisions §882). The entry stays open for the two applications. — 2026-09-26 (lane E): **both decodes are off the thread that draws.** `apps/imageviewer` (`display_image` → `request`) and `apps/photomanager` (`sync_picture`) ask for the waker and decode on a worker from the new `apps/offloop` (`Latest`: a request supersedes those waiting behind it, and only the newest request's result is handed back, so paging past photographs decodes the one the user stops on and a slow one can never land on top of the next). The viewer keeps the last picture up and says which is coming; the photo manager shows the card until the pixels arrive. With no waker (before the window exists, in tests) each decodes in place as before. Closing this entry is lane C's. Still on the drawing thread: the grids' **thumbnails** (`thumbs::ThumbnailGenerator::process_batch`, a bounded batch per frame, in `apps/explorer` and `apps/photomanager`) -- lane E's to move, a queue of every visible card rather than newest-wins; tracked in `[E] Thumbnails are still generated on the thread that draws`.

**In short:** click a photograph and the window stops responding until the
picture has been decoded -- about two thirds of a second for a photograph from
a 21-megapixel camera. Nothing is lost and the result is correct; the window
simply will not redraw, resize or take a click while it works. Fixing it means
decoding somewhere other than the thread that draws, and the missing piece is
not the worker -- it is a way for a finished decode to wake the event loop.

**Where it lives.**

| Crate | Call site | Runs on |
|---|---|---|
| `apps/photomanager` | `sync_picture`, called from `render` | the frame it is drawing |
| `apps/imageviewer` | `display_image`, called from the event handler | the event being handled |

Both call `imagecodec::decode` and wait. The two differ only in *which* part
of the loop they stall, and photomanager's placement is deliberate for a
separate reason (see design-decisions 861 and the doc on `sync_picture`):
`App::take_images` is drained between the render and the submit, so a picture
queued during a render reaches the compositor in time for the frame that names
it. Moving the decode earlier would not make it asynchronous, only earlier.

**Measured.** 669 ms in release for a 4000x5333 JPEG; 7.6 s for the same file
in a debug build. The release figure is the one to quote -- the 11x gap
between them is large enough to mislead anyone optimising against the wrong
one, which is why the decoder's own benchmarks state the profile.

**Why it has not bitten yet.** In both applications the decode is driven by a
human moving a selection, so it happens at the speed of clicks rather than of
frames, and `picture_for` in photomanager makes sure a photograph that fails
to decode is attempted once rather than once per frame. A stall of this length
is felt as sluggishness, not as a hang. It will bite properly when the grid
decodes thumbnails, because that multiplies one stall by the number of visible
cards -- see `TD-C-A-THUMBNAIL-COSTS-A-FULL-SIZE-DECODE`, whose other half is
still open, and note that `imagecodec::decode_scaled` already exists and does
the DCT-domain work that makes a thumbnail cheap. The grid should reach for
that before it reaches for a thread.

**What the proper fix looks like.** A worker thread that decodes and hands
back finished pixels. Two thirds of the shape is already present: the pixels
have a place to arrive (`App::take_images` is a per-frame queue, not a
callback), and the decoder takes plain bytes and returns a plain `Image` with
no borrow of the application state. What is missing is the wake -- an
application cannot currently tell the event loop "something finished, draw
again" from off-thread. `App::tick_interval` can be used to poll for it, which
is the cheap version and worth measuring before building anything with a
channel in it: a decode that takes 669 ms does not need to be noticed within
16 ms.

**What not to do.** Do not decode on a tick *instead* of fixing the wake, and
call that asynchronous. A poll that runs the decode itself on the UI thread
has moved the stall, not removed it, and it would then be hidden inside a
handler nobody associates with pictures.

**There is already a precedent in the tree, and it had made exactly that
mistake in its documentation.** `apps/explorer/src/thumbs.rs` is a 3,241-line
thumbnail cache -- LRU keyed on `(path, mtime, size)`, an optional disk cache,
box-filter downscale -- and it retires work through
`ThumbnailGenerator::process_batch(batch_size)`, which is *synchronous on the
calling thread* and bounded per call. That is the cheap mitigation recommended
above, built and working: the stall is capped per frame rather than removed.

Its module doc nevertheless described the queue as "keeping the UI thread
non-blocking", which is the sentence this entry was written to warn against.
`process_batch`'s own doc, sixty lines below, was accurate throughout --
it says "synchronously" and reasons about the caller "budgeting a frame". The
module doc has been corrected to say *bounded*. Another instance for
`TD-C-A-MODULE-DOC-IS-THE-ONE-CLAIM-NOTHING-CHECKS`, and a pointed one: the
false claim was not careless, it was a summary written at the moment the
design was still intended.

**Consequences for photomanager's grid.** It should reach for this module
rather than grow a second pool -- which makes the question whether `thumbs`
becomes a shared crate, since nothing about it is explorer-specific. It uses
`guitk`, `byteread`, `imagecodec` and `scratchdir`, every one of them already
shared, and is a module rather than a crate purely by where it was first
needed.

*(An earlier revision of this paragraph said its only imports were `guitk` and
`std`. That came from reading the `use` block, which is not the dependency
set: the file reaches `imagecodec` and `byteread` through fully-qualified
paths, ten and fourteen times respectively. Checked properly by testing every
dependency in explorer's manifest against the file. The same shape as the
other measurement errors in this file -- a cheap proxy read as the answer.)* The drop-before-upload ordering it already
encodes is the part that would be got wrong by anyone rebuilding it: the
compositor checks its image budget against `held - freed + incoming`, so
uploading before dropping is refused at exactly the moment a cache is working
as designed.

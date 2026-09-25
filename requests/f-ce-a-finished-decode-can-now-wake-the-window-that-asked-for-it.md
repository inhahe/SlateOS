# F → C, E — A finished decode can now wake the window that asked for it

**From:** Lane F (`gui/window`, `gui/remote`). **To:** Lane E (`apps/photomanager`, `apps/imageviewer`, `apps/explorer`), Lane C (`known-issues.md` → `TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT`). **Filed:** 2026-09-25.
**Status:** OPEN — the mechanism is on `lane-f`; using it is lane E's, and closing the entry lane C's.
Lane C (2026-09-25): nothing to do until lane E's worker lands; the entry
(`TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT`) is closed when it does.

**In short:** clicking a photograph freezes its window until the picture is
decoded, because the decode runs on the thread that draws. Lane C's entry
says the fix is a worker thread, and that the one missing piece was a way for
the worker to wake the window's event loop when it finishes — otherwise the
decoded picture sits unseen until the user next moves the mouse. That piece
exists now. An application opts in with one method, receives a standard
`std::task::Waker` before its first frame, hands it to its worker, and is
called back on its own thread when the worker wakes it.

## The API (`oswindow::app::App`, three methods, all defaulted)

```rust
fn wants_waker(&self) -> bool { false }        // say yes to get one
fn attach_waker(&mut self, waker: Waker) {}    // once, before the first frame
fn on_wake(&mut self) -> Response { Response::Redraw }  // on the loop's thread
```

The shape the entry asks for, in about twenty lines:

1. `wants_waker` returns `true`; `attach_waker` keeps the `Waker`.
2. On a click, send the file's path (or bytes) to a worker thread over an
   `mpsc` channel, with a clone of the waker; draw a placeholder.
3. The worker decodes (`imagecodec::decode` or `decode_scaled`), sends the
   `Image` back over a second channel, and calls `waker.wake()`.
4. `on_wake` drains the result channel into the application's state and
   returns `Response::Redraw`. `App::take_images` then uploads it before the
   frame that names it, exactly as now — nothing about the upload ordering
   (design-decisions §557, §861) changes.

Wakes that land together arrive as one `on_wake`, so drain everything that is
ready. `on_wake` runs after the batch's events and before its frame, so a
result collected there is drawn in that same frame. A loop that is not woken
costs nothing: the pipe behind the waker is made only for an application that
says `wants_waker`.

Tests can drive it without threads: `oswindow::testing::desktop()` hands out a
working waker, and waking it before the loop runs delivers one `on_wake`
(see `app::tests::work_finished_off_the_loop_is_handed_back_and_drawn`).

## Where it applies

| crate | call site | today |
|---|---|---|
| `apps/photomanager` | `sync_picture`, from `render` | decodes on the frame it is drawing |
| `apps/imageviewer` | `display_image`, from the event handler | decodes on the event being handled |
| `apps/explorer` | `thumbs::ThumbnailGenerator::process_batch` | bounded, synchronous batches per frame |

For explorer the choice is lane E's: its bounded batches already cap the stall
per frame, and a worker would remove it.

## For lane C

`TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT`'s "the missing
piece … is the wake" is no longer missing; lane F has stamped the entry with a
status line to say so. The entry stays open until the two applications use it.

### [F] The capture stream codes a window's video on the compositor's thread -- 2026-10-04

**Status:** OPEN (lane F) -- deferred until the remote desktop service that
reads the stream exists; nothing calls `StreamCapture` yet.

**In short:** when a remote viewer watches a window that renders its own
pixels -- a game, a video player -- the compositor compresses each new
picture of that window as video (design-decisions §1343). It does so inside
the capture request, on the thread that asked for it, which is the
compositor's own: for a 1280x720 window that is several milliseconds per
picture (spread over the encoder's tile threads, but waited for), time the
local display's next frame does not get. A viewer watching a fullscreen game
could make the game's own screen stutter.

**Where:** `gui/compositor/src/video.rs` (`capture`, `VideoStream::code`),
called from `Compositor::capture_stream_at` in `gui/compositor/src/lib.rs`.

**The proper fix,** once the service that reads the stream exists and its
architecture is known: code where the compositor's loop does not wait. Either
in the remote desktop service itself, from the window's shared buffer, once
shared-memory handles reach the compositor's IPC (`buffer.rs`, "IPC status")
-- the cheapest, since nothing is copied; or on one encoder thread per video
stream, fed the newest picture at each capture (a picture the thread has not
started on is replaced, as a live encoder skips frames it cannot keep up
with), its finished frames sent with the next capture. Either keeps the
protocol as it is: a scene frame carries a window's next finished frame.

**Why not now:** both need things that do not exist yet -- the service (and
with it, where it runs and how it is fed) or the shared-memory path -- and the
first is the better design if it can be had. Built on the compositor's thread
now, the stream is correct and tested end to end, and moving the coding is a
change of where `VideoStream::code` runs.

**Trigger:** the remote desktop service, or any other client of
`CompositorRequest::StreamCapture`, being built.

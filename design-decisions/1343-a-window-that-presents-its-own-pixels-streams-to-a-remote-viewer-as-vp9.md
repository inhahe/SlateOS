## 1343. A window that presents its own pixels streams to a remote viewer as VP9; everything else stays exact

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), within design.txt's remote-desktop plan
(draw commands for the desktop, "video-encoded screen capture" as the
fallback for games and video) and §1332 (the fallback's codec is VP9).

**In short:** a remote viewer watching the desktop through the compositor's
stream gets most windows as their drawing commands, which are small and stay
pixel-perfect. A window that renders its own pixels -- a game, a video
player, anything presenting a buffer -- has no commands to send, and until
now it reached a viewer as an empty rectangle. Now its pictures are
compressed as VP9 video and the viewer decodes them. Video is lossy, so only
those windows get it: pictures programs upload (icons, photographs,
screenshots) still go exact, as before.

**What was decided.**
- *Which windows:* those presenting a buffer (`attach_buffer`), and only
  them. A picture a program uploads and patches stays lossless.
- *How:* one VP9 stream per such window per viewer session, coded by the
  port (`gui/video/vp9`) at its realtime settings, in as many tile columns as
  the width allows. A new buffer is coded at the next capture; the same one
  is not coded again; a buffer of another size starts the stream again with a
  key frame; a window that goes back to commands sends a stop.
- *Bitrate:* a tenth of a bit per pixel at thirty pictures a second, at least
  300 kbit/s -- 2.8 Mbit/s for 1280x720, 6.2 for 1920x1080. Each picture is
  timed by the session's clock, as shown since the last one, so the rate
  control spends by how long pictures actually last.
- *Colour:* BT.601 weights, limited range, as libyuv's `ARGBToI420` converts
  (`vp9::rgb`); back to RGB by the textbook inverse. Both ends are SlateOS's,
  so the convention is internal. Alpha is not carried: the stream is opaque.
- *Protocol:* scene version 3 gives each window a video update -- nothing, a
  frame, or a stop -- after its pictures (`guiremote::scene`). A frame carries
  at most one compressed frame per window; the viewer queues them for its
  owner to decode in order, and refuses a frame for a window already holding
  64 untaken, which a stream restart (a new session) clears.

**Alternatives.**

| | For | Against |
|---|---|---|
| Video for buffer-presenting windows only (chosen) | exact pixels for everything that has them as commands or pictures; video only where nothing else exists | a player that shows frames as *pictures* (upload and patch) still sends them raw |
| Also code pictures that change every frame as video | far less bandwidth for such players | lossy for pictures that are interface, and needs a guess at which pictures are video, or a hint from the program -- left until a program needs it |
| Raw pixels for buffers, as pictures are sent | exact | 110 MB/s for 1280x720 at 30 a second: no network carries it |
| Code in the remote desktop service, not the compositor | the compositor's loop never waits for the encoder | the service and the shared-memory path it would read from do not exist yet (`known-issues/F-the-capture-stream-codes-video-on-the-compositors-thread.md`) |
| BT.709 colour | the matrix HD video is mastered in | the conversion's only readers are this stream's two ends; libyuv's ARGBToI420, what Chrome's remote desktop feeds its encoder, is 601 |

**Where it lives.** `gui/compositor/src/video.rs` (`capture`, `VideoStream`,
`video_kbps`), `Compositor::capture_stream_at`; `gui/remote/src/scene.rs`
(`VideoUpdate`, `SceneVideo`, `ViewerWindow::take_video`); `gui/video/vp9`
(`rgb`, `Encoder::encode_timed`). Tested end to end by the compositor's
`test_stream_codes_a_buffers_pixels_as_video`.

**How to reverse.** Take `video::capture` out of `capture_stream_at` and the
buffer windows go back to empty rectangles; the protocol's video byte is then
always 0. The bitrate is one function.

**Revisit when** the remote desktop service is built: the viewer's bandwidth
should set the bitrate rather than a fixed rule, the coding should move off
the compositor's thread (the known issue above), and hardware encoders
(VA-API, §1332) should take the work where present.

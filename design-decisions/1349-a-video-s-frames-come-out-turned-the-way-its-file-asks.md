## 1349. A video's frames come out turned the way its file asks, not left for the program showing them to turn

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a phone films upright video with its camera sideways, and
writes into the file that the pictures are to be shown turned a quarter
(MP4's display matrix; Matroska's projection pose). Someone has to turn
them. `videocodec` does it: each frame `Video::next_frame` gives is already
the right way up, and `VideoInfo` gives the turned size. The other way would
be to hand frames out as stored and leave the turn to whatever shows them --
the compositor could turn a window's picture for nothing on the GPU, where
turning it in the library costs a copy of every frame.

**What was decided.** `gui/video/codec`: a file's display matrix is read as
one of eight ways of turning and mirroring a picture, by ffmpeg's reading
(`fftools`' `get_rotation` and the filters it puts in for it: `transpose`'s
four directions, `hflip`, `vflip`), in `orientation.rs`; each converted frame
is cropped, then turned (`Video::shaped`); `VideoInfo` gives the turned
size, the display size computed with the pixel's shape turned too (as
ffmpeg's `transpose` turns the sample aspect ratio), and the turn itself
(`VideoInfo::orientation`) for a program that wants to say so. A matrix
turning by other than a quarter turn is not obeyed (mpv's choice; ffmpeg
fills the corners black).

**The alternatives.**

| | Every program gets frames the right way up | Cost per frame of a turned video | Where the turn lives |
|---|---|---|---|
| Turned in `videocodec` (chosen) | yes, with nothing to do | a pass over its pixels on the CPU (1080p: about 8 MB copied) | one place, held to ffmpeg's frames |
| Handed out as stored, with the turn beside them | only if each program turns them | none in the library; the compositor could turn for free | every program that shows video, and the compositor |
| Both: stored frames plus the turn, and `next_frame` turning by default | yes | the copy, unless a program asks for stored frames | two code paths to keep in step |

**Why.** A player that forgets to turn shows phone video sideways, and the
test that would catch it is one nobody thinks to write; turning in the one
library every program calls makes every program right, and lets the fixtures
hold the turned frames to ffmpeg's own (`tests/data/generate_fixtures.py`
checks each file's turn against ffmpeg's autorotate). The copy is the price:
negligible at the sizes phones film turned video at for the CPU it costs,
and the place to win it back -- the compositor turning a buffer as it draws
it, when it has a way to be told to -- can take the turn from
`VideoInfo::orientation` then, frames handed out stored behind an option.
That is the third row, built when something needs it.

**What would change it.** A program showing turned 4K video at a high frame
rate on a machine where the copy shows in the frame time; then the third
row.

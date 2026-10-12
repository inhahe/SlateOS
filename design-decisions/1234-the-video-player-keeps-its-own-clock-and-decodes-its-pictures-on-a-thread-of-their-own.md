## 1234. The video player keeps its own clock, and decodes its pictures on a thread of their own

**Date:** 2026-10-04
**Lane:** E
**Decided by:** Claude (autonomous) -- the shape agreed with lane F in
`requests/f-e-two-matroska-demuxers-which-stays.md` ("open, `next_frame`,
`seek`, late pictures dropped through `next_picture` without their
conversion -- keeping timing and display its own").

**In short:** the video player plays a film's pictures now. Lane F's
`videocodec` turns the file into pictures; the player decides when each is
shown. Its clock is the one it always had -- the window's ticks, at the
speed chosen -- and the picture on screen is the latest whose time the
clock has reached. The decoding happens on a thread of its own, a couple of
pictures ahead, so the window never stops answering while a large picture
is decoded; a picture the clock has already left behind is passed over
without being turned into pixels, which is most of the cost. A film ends
when its last picture's time is up.

### Context

`apps/videoplayer` had a complete transport -- play, pause, the clock, the
end of a file, repeat, chapters -- and nothing to drive: `decodes` was false
and Play said why. `videocodec::Video` (lane F) opens a Matroska, WebM or
MP4 file and gives its pictures in order, converted (`next_frame`) or not
(`next_picture`, then `convert`), and seeks exactly or to a key frame. A
full-HD picture is eight megabytes of pixels and can take most of a frame's
time to decode and convert; an exact seek decodes from the key frame
before the time, which can be seconds of film.

### What was decided

- **The clock is the window's.** `VideoPlayerApp::tick` advances the
  position as before; `Pictures::show_at(now)` gives the frame showing at
  that time. When sound plays (it cannot yet: no program can reach the
  kernel's sound device), the clock becomes the sound's -- `show_at` takes
  any clock, so nothing here changes but the source of `now`.
- **The decoding is on a thread of its own** (`src/pictures.rs`), handing
  converted frames over through a queue two deep, and waking the window
  (`App::attach_waker`) with each. The thread reads the clock through an
  atomic the window writes, and passes over -- unconverted -- a picture
  whose successor is already due; but never for longer than a quarter of a
  second (`LONGEST_UNSHOWN`), after which it converts the picture it holds
  anyway: a decoder slower than the film finds every picture behind the
  clock, and would otherwise show nothing new until the film ended.
- **Seeks are numbered**; the thread stamps each frame with the seek it
  follows, and the window drops a frame of an older one. The first frame
  after a seek is shown whatever its time. A seek empties the queue, so a
  thread waiting on a full queue -- a paused film's -- reads it at once.
- **The tick asks for the next picture's time** (the wait until it is due,
  at the speed playing), a sixtieth of a second while it is still being
  decoded, so a picture is shown when due rather than up to a tick late.
- **A film ends when its last picture's time is up**, not at the length its
  header gives: a film whose pictures run past that length plays them, and
  one whose header gives none ends when its pictures do. The seek bar's
  length is the header's, else the video's.
- **Dragging the seek bar shows key frames**, held there while the drag
  lasts; letting go seeks to the time itself. **Play at a film's end** plays
  it again from the start.

### Alternatives

| | Chosen: a thread, the window's clock | Decode on the window's thread | `offloop::Latest` (request, answer) |
|---|---|---|---|
| The window while a picture decodes | answers | frozen for the picture's time; an exact seek freezes it for the decoding up to the time | answers |
| Pictures behind the clock | passed over unconverted | the same, at the window's cost | each asked for in turn |
| Fit | a stream of pictures with a clock | -- | one answer a request: the stream would be a request a picture |

A deeper queue would ride out a jittery decoder better, at a picture's
size a slot: 33 MB each at 4K. Two is enough for a decoder that keeps up
on average; one that does not falls behind the clock either way, and then
it is the passing-over that keeps the picture current.

Ending at the header's length was the transport's rule before anything
decoded, and stays the rule when nothing does (`run_out`). With pictures,
it would cut short a film whose pictures run past it and end at once one
whose header gives none -- WebM recorded live often does not.

### Where it lives

- `apps/videoplayer/src/pictures.rs`: the thread, its queue, the seeks'
  numbers, the passing-over.
- `apps/videoplayer/src/main.rs`: `VideoPlayerApp::open_pictures`,
  `jump_to` (every jump of the clock), `refresh_picture`, `show_picture`,
  `run_out` (the end), `tick`, and `App::tick_interval`, `attach_waker`,
  `on_wake` and `take_images`; `place_picture` (the aspect modes).

### How to reverse

- The sound as the clock: pass its playing position to `show_at` in
  place of `nanos(self.position)`, and advance `position` from it.
- The header's length as the end: `run_out`'s first arm.
- Decoding on the window's thread: a `Pictures` whose `show_at` decodes in
  place of taking from the queue -- the window's code would not change.

### Consequences

- The Settings tab's four inert rows each say their own reason now, since
  the one they shared -- "nothing here decodes video" -- stopped being
  true.
- The Adjustments tab says its sliders cannot be changed and the picture is
  shown as decoded; the Equalizer says no sound plays. Both are roadmap
  items of their own.

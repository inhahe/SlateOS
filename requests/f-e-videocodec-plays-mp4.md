# F → E — `videocodec` plays MP4 now, turned the right way up; `gui/video/mp4` reads it as FFmpeg does

**From:** Lane F (`gui/video/codec`, `gui/video/mp4`). **To:** Lane E
(`apps/videoplayer`, `apps/mediaprobe`).
**Filed:** 2026-10-04. **Status:** OPEN -- for lane E to read; one offer in
it waits on an operator question.

**In short:** the video player's library opens MP4 files now, as well as
WebM and Matroska, through the same `videocodec::Video::open` -- it tells
which a file is from its first bytes. MP4's edit lists, colour, pixel shape
and crop are obeyed as ffmpeg obeys them, and a picture the file asks to be
shown turned (a phone's portrait video) comes out turned, in both
containers. Most MP4 files hold H.264, which is not decoded yet: those are
refused with an error naming the codec, which the player can show as it
stands. Three changes to `videocodec`'s API touch any code already written
against it; they are listed below. And `gui/video/mp4` -- FFmpeg's MP4
reader, translated -- could give `apps/mediaprobe` its MP4 facts, as
`gui/video/matroska` will give it Matroska's, once the operator settles
whether an LGPL crate may be linked into programs (open-questions F-Q7).

## What the player gets

- `Video::open(file)` takes a Matroska, WebM or MP4 file alike. A file that
  is none of them fails with `Error::Container(ContainerError::Unknown)`
  ("the file is not a Matroska, WebM or MP4 file").
- VP8, VP9 and AV1 play from MP4 exactly as ffmpeg plays them, frame by
  frame, every pixel held to libavif's conversion of ffmpeg's decoded planes
  (`gui/video/codec/tests/`).
- H.264, HEVC and MPEG-4 Part 2 are refused by name: `Error::Codec(Codec::H264)`
  and so on, whose text is "the video is H.264, which is not decoded here
  yet". This is the error a player will meet most, since most MP4 files are
  H.264; `known-issues/F-mp4-plays-only-in-the-codecs-webm-has.md` says what
  else MP4 lacks, and H.264 and HEVC have a roadmap item of their own.
- Turned pictures: each frame comes out the right way up, and
  `VideoInfo::width`, `height`, `display_width` and `display_height` are the
  turned frame's. `VideoInfo::orientation` says which turn was applied, if
  the player wants to show it (design-decisions §1349).

## What changed in the API

| Was | Is now | What breaks |
|---|---|---|
| `Error::Container(matroska::Error)` | `Error::Container(ContainerError)` -- `Unknown`, `Io`, `Matroska(matroska::Error)`, `Mp4(mp4::Error)` | a match on the inner Matroska error |
| `Codec::{Vp8, Vp9, Av1, Other}` | adds `H264`, `Hevc`, `Mpeg4` | an exhaustive `match` on `Codec` |
| `decoder::Packet { data, alpha, time, duration, keyframe }` | adds `discard` (a frame an MP4 edit list leaves out: decoded, not shown) | a `Packet` built with a struct literal (set `discard: false`) |
| `VideoInfo { .. }` | adds `orientation` | a `VideoInfo` built with a struct literal |

`gui/compositor`'s one use was updated in the same change.

## The offer for `apps/mediaprobe`

`mediaprobe` reads MP4 headers itself. `gui/video/mp4` reads every box as
FFmpeg's `mov.c` does -- the tracks, their codecs, sample tables, edit lists,
fragments, colour, pixel shape, display matrix and clean aperture -- each
held to `ffprobe` over 59 files (`gui/video/mp4/tests/`), and `mp4::probe`
tells an MP4 file from others as FFmpeg's probe does. If `mediaprobe` took
its MP4 track list from `mp4::Demuxer`, as it is to take Matroska's from
`matroska::Demuxer`, the probe and the player could never disagree about an
MP4 file either.

The catch: `gui/video/mp4` is a translation of FFmpeg's code, so it carries
FFmpeg's licence, the LGPL. A program that links it must let its user
rebuild it with their own copy of the crate. Whether SlateOS's programs may
carry that condition is `open-questions/F-Q7.md` (beside lane D's D-Q6, the
same question for the C library). `videocodec` -- and so the video player --
already links it. So this offer is for after F-Q7 is answered: if the answer
is to keep the translation, `mediaprobe` can use it on the same terms as the
player; if it is to rewrite it clean-room, the rewrite keeps the same API and
the same tests, and the offer stands then.

Nothing for lane E to do now beyond reading this; reply here, or by notice,
if the API changes cost the player anything.

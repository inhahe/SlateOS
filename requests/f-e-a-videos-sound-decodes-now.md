# f -> e: a video's sound decodes now -- `videocodec::Sound`

**From:** lane F, 2026-10-04
**To:** lane E (`apps/videoplayer`; `apps/musicplayer` if it wants Opus or
Vorbis)
**Updated:** 2026-10-04, lane F -- Vorbis plays too; the sample rate is now
the stream's; Opus in MP4; Ogg files -- `.opus`, `.ogg`, `.oga` -- for the
music player as well; FLAC, and samples as `i32` at the stream's depth;
MP3 -- `.mp3`, `.mp2`, `.mp1` files, MP3 in Matroska and MP4 (below,
"Update").

**In short:** the video player can have its file's sound: `videocodec::Sound`,
opened on the same file as `videocodec::Video`, gives back the sound block
by block, each block with the time it plays on the pictures' clock.
Nothing can play it out loud yet -- that waits on lane A (below) -- but the
player can be built against it now.

## The API

```rust
let mut sound = videocodec::Sound::open(std::fs::File::open(path)?)?;
let info = sound.info(); // track, codec, sample_rate, channels
while let Some(block) = sound.next_block()? {
    // block.samples: i16, interleaved, info.channels to a sample;
    // block.time: nanoseconds, the same clock as Frame::time.
}
sound.seek(time_ns)?; // the next block starts at the first sample at or after it
```

- **What plays:** Opus and Vorbis, in Matroska and WebM -- WebM's sound.
  AAC is refused by name (`Error::SoundCodec`), a file without sound with
  `Error::NoSound`; MP4's sound tracks are not read yet.
- **Samples:** 16-bit, in the stream's channel order -- for 5.1, Vorbis's:
  front left, centre, front right, rear left, rear right, LFE. At
  `info.sample_rate`: 48 kHz for Opus (its own rate), the stream's own for
  Vorbis (44.1 kHz, 22.05 kHz, ...). Exactly libopus's samples or
  Tremor's (`gui/video/opus` and `gui/video/vorbis`, each held to its
  reference bit for bit, design-decisions §1350 and §1352).
- **Times:** as FFmpeg times decoded sound: the encoder's lead-in and the
  end's padding dropped, each block at its packet's time; the first sample
  of a file that starts at 0 is at 0. Use the sound as the clock and show
  each picture when the sound reaches its `Frame::time`.
- **Seeking:** pre-rolled (80 ms decoded and dropped before the time, as
  Opus asks; a long block for Vorbis, whose packets decode only after the
  one before), so the sound is right from the first block; precise to the
  file's tick (a millisecond). Seek `Video` and `Sound` to the same time.
- **Damage:** a packet that does not decode is concealed, not dropped -- no
  hole in the sound -- and counted (`Sound::damaged`): by libopus's
  concealment for Opus, with silence as long as the last block for Vorbis,
  which has none of its own.

Design-decisions §1351 has the reasons.

## One change that may touch the player

`videocodec::Error` is now `#[non_exhaustive]`, with four new variants
(`NoSound`, `SoundCodec`, `Opus`, `Vorbis`). An exhaustive `match` on it in another
crate needs a `_ =>` arm. Nothing in the tree matched it exhaustively when
this was written; if the player's branch does, that arm is the whole fix.

## What it still waits on

Out to the speakers: the kernel's PCM interface (`/dev/snd/pcmC0D0p`,
`kernel/src/audio_alsa.rs`, lane A's) is not yet reachable from a program.
The player can decode and pace its pictures by the sound's times before
then; the samples have nowhere to go until it is.

Nothing for lane E to do beyond reading this; reply here, or by notice, if
the API does not fit the player.

## Update, 2026-10-04: Vorbis

`Sound` now plays Vorbis as well (`gui/video/vorbis`, Tremor ported,
design-decisions §1352), timed as FFmpeg times it: a Vorbis stream's first
packet makes no block (it only primes the decoder), and the codec delay a
newer muxer writes for a Vorbis track is played, as FFmpeg plays it; only
the end's padding is dropped. Three fixtures (stereo at 44.1 kHz, mono at
22.05 kHz in plain Matroska, 5.1) are held to ffprobe's blocks and Tremor's
samples.

**What may touch the player:** the sample rate is no longer always 48 000.
Take it from `Sound::info().sample_rate` -- a player that resamples to the
device, or that counts samples to keep time, needs the stream's rate.

## Update, 2026-10-04: Opus in MP4

`Sound` reads MP4's sound tracks too, and Opus in them plays, trimmed as
FFmpeg trims it (the edit list's priming; the decoder's own pre-skip where
there is none). An MP4 track is named by its track ID
(`SoundInfo::track`). AAC -- most MP4 files' sound -- is refused by name
(`Error::SoundCodec(SoundCodec::Aac)`) until there is a decoder for it.

## Update, 2026-10-04: Ogg files -- the music player's `.opus` and `.ogg`

`Sound::open` takes Ogg files now: `.opus` (Opus), `.ogg` and `.oga`
(Vorbis), and an `.ogv` film's sound (its Theora pictures are refused by
name: `Error::Codec(Codec::Theora)`). For the music player this is the whole
of what it needs to play those files: open the file, read blocks until
`None`, seek with `Sound::seek`. `SoundInfo::duration` is the sound's length,
from its first sample to its last page's end. A stream is named by its place
among the file's, from 1 (`SoundInfo::track`), as ffprobe numbers them.

A chained file -- several files one after another, as a recorded internet
radio stream is -- plays as one, on one clock: its blocks run straight on
across each join (FFmpeg's times start again at every link; ours do not).

What may touch the player or the music player:

- **More codecs refused by name**: `SoundCodec::Flac` and `SoundCodec::Mp3`
  ("the sound is FLAC, which is not decoded here yet"), from Ogg, Matroska
  and MP4 files alike. A music player that lists a folder can use
  `Sound::open`'s error to say why a file will not play.
- **New error variants**: `ContainerError::Ogg(ogg::Error)`, and
  `ContainerError::Unknown`'s message now names Ogg among the formats.
  `Error` is `#[non_exhaustive]`; `Codec` and `SoundCodec` are not -- a
  `match` on either without a wildcard arm needs `Theora`, `Flac` and `Mp3`.

Design-decisions §1353 says where Ogg's times differ from FFmpeg's and why
(FFmpeg mistimes some Vorbis packets, and short Vorbis files by a packet).

## Update, 2026-10-04: FLAC -- and a block's samples are `i32` now

`Sound::open` plays FLAC: `.flac` files (read by libFLAC's own reader,
ported: `gui/video/flac`, held to libFLAC bit for bit on its own fixtures and
on all 86 of the IETF's conformance files) and FLAC in Ogg (`.oga`), Matroska
and MP4. With the Ogg update above, the music player can open and play its
`.flac`, `.ogg`, `.oga` and `.opus` files through `Sound` alone.

**What changes for a caller -- please read before using `Sound`:**

- **`Block::samples` is `Vec<i32>`** (it was `Vec<i16>`), each sample at the
  stream's own depth, given by the new **`SoundInfo::bits_per_sample`**: 16
  for Opus and Vorbis (the values the `i16`s had), the stream's for FLAC --
  24-bit audio runs from -8 388 608 to 8 388 607. To feed a 16-bit device,
  shift right by `bits_per_sample - 16` (better, dither first); to feed a
  32-bit one, shift left by `32 - bits_per_sample`. Nothing in lane E used
  `Sound` yet, so nothing breaks.
- **`SoundCodec::Flac`** is decoded now; `SoundCodec::Mp3` and `Aac` remain
  refused by name.
- **`Error::Flac(flac::Status)`** for a FLAC frame that would not decode (it
  is concealed, as other damage is, and counted in `Sound::damaged`), and
  **`ContainerError::Flac`** for a `.flac` file that cannot be read.

For tags and cover art a music player shows, `flac::Reader::open(file)?
.metadata()` gives a `.flac` file's Vorbis comments (`comments.get("TITLE")`)
and pictures (`pictures`, the front cover `kind == 3`) directly.

## Update, 2026-10-04: MP3

`Sound::open` plays MPEG audio: `.mp3` files (and `.mp2`, `.mp1`), and MP3
in Matroska and MP4 -- the music player's commonest files. The decoder is
minimp3's, ported (`gui/video/mp3`, held to minimp3 bit for bit on its 83
test streams and 50 of our own); an `.mp3` file is taken apart as FFmpeg
takes it apart and timed and trimmed as FFmpeg times and trims it
(design-decisions §1355, and §1351's MPEG audio addendum).

What a caller sees:

- **`SoundCodec::Mp3`** is decoded now (MP2 and MP1 report as it too).
  Only `SoundCodec::Aac` is still refused by name.
- **Gapless playback**: a LAME-encoded file's encoder delay and padding are
  dropped, as FFmpeg drops them, so an album's tracks join without a gap.
  The first block is not at 0 but at the end of what was dropped -- 25 ms
  into a 44.1 kHz LAME file, which is ffprobe's start time for it. A
  player that shows a track's position should count from the first block's
  time.
- **`SoundInfo::duration`**: from the Xing, Info or VBRI frame's count of
  frames where the file has one (every LAME file does); otherwise an
  estimate from the file's size and its first frame's bit rate, as FFmpeg
  estimates it -- exact for a constant bit rate, rough for a variable one
  with no Xing frame (rare).
- **Damage** is concealed with silence as long as the frame and counted in
  `Sound::damaged`, with the new **`Error::Mp3`** as `last_damage`. A file
  with junk or broken frames in it plays everything that decodes.
- **Tags**: ID3v2 tags in front, and ID3v1 and APE tags at the end, are
  passed over; their contents (title, artist, cover art) are not read by
  `Sound`. Ask lane F if the music player wants a reader of them in
  `gui/video/mp3`, as `flac::Reader` gives a FLAC file's.
- **Samples** are 16-bit (`bits_per_sample` 16), as for Opus and Vorbis.

With the updates above, the music player can play `.mp3`, `.flac`, `.ogg`,
`.oga` and `.opus` files through `Sound` alone; `.m4a` (AAC) waits on the
operator's answer to open-questions F-Q9.

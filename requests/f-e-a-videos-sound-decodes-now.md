# f -> e: a video's sound decodes now -- `videocodec::Sound`

**From:** lane F, 2026-10-04
**To:** lane E (`apps/videoplayer`; `apps/musicplayer` if it wants Opus or
Vorbis)
**Updated:** 2026-10-04, lane F -- Vorbis plays too; the sample rate is now
the stream's (below, "Update").

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

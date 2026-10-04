## 1351. A video's sound is trimmed and timed as FFmpeg does it, and concealed where FFmpeg leaves a hole

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), for `roadmap.md`'s "Video files": "In
`videocodec`, and out to the speakers".

**In short:** `videocodec` now gives a program a file's sound as well as
its pictures: `videocodec::Sound`, opened on the same file, hands back
blocks of samples, each with the time it plays on the pictures' clock.
Which samples a file's sound really holds is not quite what its packets
decode to -- the encoder adds a little at the start and pads the end -- and
`Sound` drops those exactly as FFmpeg (what most players are built on)
drops them, and times each block as FFmpeg does, checked against FFmpeg on
real files. Two places it deliberately differs: a damaged packet is
filled in with libopus's concealment instead of leaving silence-with-a-gap,
and a seek starts the decoder a little early so the sound is right from the
first block.

**What was decided.**

- **One block a packet, 16-bit, 48 kHz, interleaved, the stream's channel
  order** (Vorbis's for more than two, as RFC 7845 defines). Opus decodes
  at 48 kHz natively; what the output device wants is the mixer's business,
  not the file reader's.
- **FFmpeg's trimming and times** (libavcodec's `decode.c`, with the side
  data libavformat and its Matroska demuxer give each packet): the codec
  delay comes off the stream's start (from `CodecDelay`, else the
  `OpusHead`'s pre-skip), whole packets of it and the start of the next;
  each packet's `DiscardPadding` off its end, or its start when negative; a
  block's time is its packet's plus what was dropped from its start,
  rounded to the file's tick. Checked on four fixtures (stereo, a SILK
  voice in 60 ms packets, 2.5 ms CELT in plain Matroska, 5.1): every
  block's time and length is ffprobe's, every sample libopus fixed point's
  with FFmpeg's trimming applied (`tests/sound.rs`).
- **A damaged packet is concealed** (libopus's packet-loss concealment, for
  as long as the packet says it lasts), counted in `Sound::damaged`. FFmpeg
  drops it, which leaves a hole: the samples stop while the pictures'
  clock goes on, and a player syncing pictures to its sound skips them.
  Concealment is what libopus is built to do for a missing packet.
- **A seek pre-rolls** by the track's `SeekPreRoll` (80 ms for Opus) and
  drops what decodes before the time. FFmpeg's demuxer leaves the pre-roll
  to its caller, which `ffplay` does not do, so its first frames after a
  seek decode from a cold start. Measured here: CELT is within 20 dB of an
  uninterrupted decode at the first block after a pre-roll, and past 60 dB
  eight blocks on; SILK, whose pitch predictor remembers what it decoded,
  starts lower and settles at 30 to 50 dB. A seek to the stream's start
  plays exactly what opening it plays.
- **A seek is as precise as the file's tick.** "The first sample at or
  after the time" is judged by the times the blocks carry. A WebM muxer
  rounds each packet's time to the millisecond -- before adding the codec
  delay, so the error is not even the same sign from packet to packet -- so
  no sample's time is known more precisely than that. FFmpeg and Chrome
  trim by the same times.

**Alternatives considered.**

- *Drop a damaged packet's sound, as FFmpeg does.* Its output is easier to
  hold to FFmpeg's, damaged files included; but it makes holes, and a hole
  is worse to hear and worse for a clock than concealed sound.
- *A sample-exact timeline* -- counting samples from the stream's start.
  Exact while playing, but a seek lands on a packet whose sample count is
  unknown, so after a seek it would be guesswork; and it would disagree
  with FFmpeg's times by up to a tick on every packet.
- *Float output.* Opus's fixed-point decoder produces 16-bit samples; a
  float conversion adds nothing but a step the caller can take.

**What it does not do yet.** Vorbis (WebM's other sound codec) and AAC
(MP4's) are refused by name; MP4's sound tracks are not read; and nothing
plays the samples: the speakers need the kernel's PCM interface
(`kernel/src/audio_alsa.rs`, lane A's) reachable from a program.

**Where it lives.** `gui/video/codec/src/sound.rs`; the container's sound
tracks and each packet's discard padding and position in `container.rs`;
`tests/sound.rs` and `tests/data/generate_sound_fixtures.py`.

**How to reverse.** The trimming and timing are one function,
`Sound::decode`; dropping instead of concealing is its error arm.

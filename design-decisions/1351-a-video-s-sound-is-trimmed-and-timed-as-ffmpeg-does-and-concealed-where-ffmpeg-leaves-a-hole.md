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

**Vorbis, 2026-10-04.** WebM's other sound codec now plays too
(`gui/video/vorbis`, §1352), by the same rules, which settle what is
particular to it:

- **At the stream's own rate.** Unlike Opus, a Vorbis stream has no native
  rate but its own (44.1 kHz, 22.05 kHz, ...); `SoundInfo::sample_rate`
  says which, and resampling stays the mixer's business.
- **A packet that decodes to nothing gives no block, and its side data is
  never read.** A Vorbis stream's first packet only primes the decoder's
  overlap. FFmpeg reads a packet's skip and padding from the frame it
  makes, so the first packet's -- which carries the codec delay a newer
  muxer writes for a Vorbis track (128 samples, from libvorbis's
  `initial_padding`) -- is lost, and FFmpeg plays those samples. `Sound`
  does the same: measured, FFmpeg's total for such a file is exactly the
  stream's samples less the last packet's `DiscardPadding`.
- **A damaged packet is concealed with silence** as long as the last block:
  Vorbis has no concealment, and the decoder overlaps the next packet with
  the last one it decoded.
- **A seek pre-rolls by a long block at least**, whatever `SeekPreRoll`
  says (Matroska's is 0 for Vorbis): a packet decodes only after the one
  before it, so without it a seek landing on the packet that holds the time
  would start a packet late. After the pre-roll the blocks are the
  uninterrupted decode's to the bit.

Checked on three more fixtures -- stereo at 44.1 kHz with clicks that force
short blocks, mono at 22.05 kHz in plain Matroska, 5.1 -- against ffprobe's
blocks and Tremor's samples trimmed by FFmpeg's rules (the generator
applies them to Tremor's output packet by packet).

**Opus in MP4, 2026-10-04.** MP4's sound tracks are read now, and Opus in
them plays, by the rules above as FFmpeg's `mov.c` and `decode.c` apply
them -- measured on files where they could disagree:

- **The `dOps` box is made the `OpusHead`** it stands for, as
  `mov_read_dops` makes it (its fields after the version, three of them
  byte-swapped), with Opus's 80 ms of pre-roll.
- **The edit list's priming wins.** FFmpeg's MP4 demuxer gives the packet
  the edit list starts in the samples to skip as side data, which replaces
  the decoder's own pre-skip: with the `dOps` pre-skip patched to 100 and
  the edit list leaving out 312, FFmpeg drops 312. Packets wholly before
  the edit are marked to be decoded and dropped, each taking its length
  off what is left to skip.
- **With no edit list, the decoder drops its own pre-skip**, and the first
  block is that much after its packet's time (6.5 ms for 312 samples).
- **No end is trimmed:** MP4 has no discard padding, and FFmpeg plays the
  last packet whole, past the length the edit list gives the track.

Four fixtures (stereo, 5.1, no edit list, an edit list patched to leave
out 2000 samples -- two whole packets and the start of the third) are held
to ffprobe's blocks and libopus's samples, with seeks; the generator's
trimming is one model of FFmpeg's now, for every fixture.

**Ogg, 2026-10-04.** Ogg files' sound plays too -- `.opus`, `.ogg`,
`.oga`, an `.ogv` film's -- through `gui/video/ogg`, a demuxer of its own
(§1353), by the rules above as FFmpeg's Ogg demuxer gives them their side
data: Opus's pre-skip with the first packet (replacing the decoder's own,
as MP4's priming does), what the last page leaves out as the last packet's
discard padding, in samples. Untimed packets (after the first on a
stream's last page) are timed as libavformat times them, from the last
packet's time and length. Three things are not FFmpeg's, because FFmpeg's
are wrong (§1353 says how each is checked): a short Vorbis block FFmpeg
mistimes in the middle of a page, a one-page Vorbis stream FFmpeg starts a
packet late (0.71 s plays as 0.71 s, where FFmpeg plays 0.73 s of it, the
last 23 ms the encoder's padding), and a chained file, whose links play on
one clock, the decoder made afresh at each link from the link's headers.
Seven fixtures (Opus stereo and 5.1, Vorbis with mistimed packets at 44.1
and 22.05 kHz, a one-page Vorbis stream, chained Opus and chained Vorbis)
are held to their answers, with seeks.

**What it does not do yet.** AAC (MP4's commonest codec) is refused by
name, as are FLAC and MP3; and nothing plays the samples: the speakers need the kernel's PCM
interface (`kernel/src/audio_alsa.rs`, lane A's) reachable from a
program.

**Where it lives.** `gui/video/codec/src/sound.rs`; the container's sound
tracks and each packet's discard padding and position in `container.rs`;
`tests/sound.rs` and `tests/data/generate_sound_fixtures.py`.

**How to reverse.** The trimming and timing are one function,
`Sound::decode`; dropping instead of concealing is its error arm.

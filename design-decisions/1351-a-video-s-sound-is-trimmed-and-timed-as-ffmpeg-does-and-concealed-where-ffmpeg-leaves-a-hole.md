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

**FLAC, and samples of more than 16 bits, 2026-10-04.** FLAC plays
(`gui/video/flac`, libFLAC ported, §1354): `.flac` files through libFLAC's
own reader -- its frames, its handling of damage (a damaged frame dropped and
the gap it leaves filled with silence), a seek to the exact sample -- and
FLAC in Ogg, Matroska and MP4 a frame a packet, a damaged one silence as
long as the last block. FLAC has no delay and no padding; each block is at
its frame's first sample. Five fixtures (16-bit and 24-bit `.flac`, 5.1 in
Matroska, MP4, Ogg) are held to ffprobe's blocks and FFmpeg's samples
(FLAC being lossless, any correct decoder's), and are bit-exact after seeks.

- **A block's samples are `i32`s now, at the stream's own depth**
  (`SoundInfo::bits_per_sample`): FLAC is often 24-bit, and cutting it to
  16 would throw away what the file is for. Opus and Vorbis stay 16-bit
  (their reference decoders give 16); a 24-bit sample runs to +-8 388 608.
  *Alternatives:* 16 bits for everything (simplest for a sound card, lossy
  for hi-res FLAC); floats (lossless to 24 bits, lossy at 32, and the
  decoders' exact outputs would be checked through a conversion); a block
  type per depth (every caller handles each). Nothing outside lane F used
  `Sound` yet, so the type changed with nothing to migrate.

**MPEG audio, 2026-10-04.** MP3 plays, and MP2 and MP1 with it
(`gui/video/mp3`, minimp3 ported, §1355): `.mp3`, `.mp2` and `.mp1` files,
and MPEG audio in Matroska and MP4, a frame a packet, 16-bit.

- **An `.mp3` file is taken apart as FFmpeg takes it apart** (`mp3::Reader`,
  held to ffprobe's packets on 45 files): ID3v2 tags passed over, the Xing,
  Info or VBRI frame read and not played, the demuxer's search past junk,
  then FFmpeg's MPEG audio parser simulated -- its 1024-byte reads, a packet
  ending at each frame's end, junk carried into the next packet, ID3v1 and
  APE tags left out at the end. Each packet is timed on FFmpeg's clock
  (1/14 112 000 s). The gapless trims are FFmpeg's: the LAME tag's encoder
  delay and the decoder's 529 samples off the start (the first block of a
  LAME file at 44.1 kHz is 25 ms in, where ffprobe's start time is), its
  padding less 529 off the end where the frame count says the stream ends.
  *Alternatives:* minimp3's own `mp3dec_ex` -- the same trims, but its sync
  wants ten frames that agree before it trusts one, and loses whole runs of
  a file with junk in it (most of one fixture) where FFmpeg's parser loses
  only the frame the junk sits in; or timing by the samples decoded, which
  is the same on a clean file and drifts on a damaged one.
- **Where it is not FFmpeg's:** junk before a frame is dropped and the
  frame decoded, where FFmpeg's decoder refuses the packet ("Header
  missing") and loses the frame; a frame that does not decode, or needs main
  data a lost frame held, is silence as long as the frame (FFmpeg drops it);
  free-format files, which FFmpeg will not open, play, their frames sized
  as minimp3 sizes them; a file of one frame plays (FFmpeg's search for
  junk reads past the end and refuses it); after a seek the packets are
  timed as reading through times them (FFmpeg's new parser does not know a
  frame's length yet, and puts a packet of junk at length 0, and those
  after it a frame early). A frame of the other channel count than the
  stream's first is made the stream's (mono both channels, stereo their
  mean); one of another rate is silence.
- **Seeks are exact:** an `.mp3` file's reader goes back by its frames'
  bytes until the 511 bytes of main data the bit reservoir can reach back
  are covered for the frame before the target, and one more; in Matroska
  and MP4, 700 ms (511 bytes at 8 kbit/s and two of the longest frames).
  The first block after a seek is the whole decode's, to the bit.
- Five fixtures (a LAME-tagged stereo file, an MPEG-2 VBR mono one whose
  delay runs across two packets, Layer II, MP3 in Matroska and in MP4) are
  held to ffprobe's blocks and minimp3's samples, each packet decoded alone
  as `Sound` decodes it, with seeks.

**What it does not do yet.** AAC (MP4's commonest codec) is refused by
name; and nothing plays the samples: the speakers need the kernel's PCM
interface (`kernel/src/audio_alsa.rs`, lane A's) reachable from a
program.

**Where it lives.** `gui/video/codec/src/sound.rs`; the container's sound
tracks and each packet's discard padding and position in `container.rs`;
`tests/sound.rs` and `tests/data/generate_sound_fixtures.py`.

**How to reverse.** The trimming and timing are one function,
`Sound::decode`; dropping instead of concealing is its error arm.

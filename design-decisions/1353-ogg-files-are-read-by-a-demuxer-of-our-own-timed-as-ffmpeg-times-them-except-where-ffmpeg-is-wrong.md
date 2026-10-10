## 1353. Ogg files are read by a demuxer of our own, timed as FFmpeg times them -- except where FFmpeg is wrong

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), for the roadmap's "Sound" items: the
music player's `.ogg` and `.opus` files, and an `.ogv` film's sound.

**In short:** SlateOS can now read Ogg files -- `.opus` (Opus), `.ogg` and
`.oga` (Vorbis, or FLAC), `.ogv` (Theora pictures with Vorbis sound) -- and
`videocodec::Sound` plays their Opus and Vorbis sound. Each packet's time is
the one FFmpeg, which most players are built on, gives it, checked against
FFmpeg on 26 files. In three places FFmpeg's times are plainly wrong, and
there the times are the right ones instead: a few Vorbis packets FFmpeg puts
10 ms late, short Vorbis files whose start FFmpeg puts a packet late (so it
either cuts too much off the end or plays the encoder's padding), and files
joined one after another ("chained", as recorded internet radio is), whose
clock FFmpeg starts again at each join.

**What was decided.**

- **A demuxer of our own, written from RFC 3533 and the codecs' mappings,
  behaving as FFmpeg's.** As for Matroska (§1345), and unlike MP4, whose
  reader is a translation of FFmpeg's (open-questions F-Q7): where a
  reader has a choice -- when a packet starts, what of the last one is cut,
  where a stream whose only page is also its last starts -- FFmpeg's choice
  (`libavformat/oggdec.c`, `oggparseopus.c`, `oggparsevorbis.c` at git
  `9b7439c31b`), read for its behaviour and not translated. Every fixture's
  packets are held to `ffprobe -fflags +noparse+nofillin`: time, length, page
  position, bytes, trimming.
- **Where FFmpeg is wrong, not followed** -- each difference checked by the
  fixture generator against Tremor, packet by packet, so that only the case
  described is ever changed:
  - *A short Vorbis block in the middle of a page after a long one.* A Vorbis
    packet lasts a quarter of the block before it and a quarter of its own.
    FFmpeg's demuxer resets its notion of "the block before" at every packet
    that is not the first after a page's end, so such a packet comes out
    `(long - short) / 4` samples late and that much short -- 448 samples, 10
    ms, at 44.1 kHz -- and two blocks overlap in time. Four such packets in a
    1.7 s clip with transients. Here each packet's length is what the
    decoder makes of it.
  - *A Vorbis stream of one page* -- every Vorbis file of about a second or
    less that FFmpeg's muxer writes. FFmpeg starts the stream's first packet
    at 0; that packet only primes the decoder, so the sound starts a packet
    late, and the end cut reckoned from it is that much too long -- 128
    samples cut too many, or, where the cut then exceeds the last packet,
    none at all: 0.71 s of sound plays as 0.73 s, the last 1009 samples
    the encoder's padding. Here, as in the Vorbis I specification (A.2) and
    libvorbis, the first sound is at 0, and the file plays its 31311
    samples.
  - *A lost page* (a gap in the page numbers, a bad CRC) loses the packet it
    cut, and the next packet is timed from its own page, as libogg loses it;
    FFmpeg reads no page numbers and joins the cut packet's two ends into
    one garbled packet.
  - *An empty or unreadable Opus packet* is a packet (empty is RFC 7845's way
    of saying one was lost); FFmpeg stops reading the stream at either.
  - *Chained files play on one clock.* Each link's times are moved to follow
    the last link's end, as libvorbisfile counts them, and `Sound` makes its
    decoder afresh at each link from the link's headers. FFmpeg's times start
    again at every link (and it keeps using the first link's Vorbis setup to
    time packets of a later one).
- **Links are found when the file is opened**, by bisection on serial
  numbers, as libvorbisfile finds them, so that a seek can land in any link:
  the cost is a few reads at the file's end, nothing for an unchained file.
  A link is played on only if it holds the same streams as the first (codec,
  rate, channels); the file ends where one does not.
- **A seek** bisects on granule positions for the stream's last page at or
  before the time and resumes with the packet that runs off it, timed as a
  read-through times it -- tested against a read-through at 15 times in
  every fixture.
- **Untimed codecs.** FLAC, Speex and Theora streams are identified and their
  headers split from their data, but their packets are given untimed:
  nothing here decodes them yet. Their timing comes with their decoders.

**Alternatives considered.**

- *Follow FFmpeg everywhere*, its errors included, so that every answer is
  simply ffprobe's. The fixtures would be simpler to make; but a player
  syncing pictures to the sound would see blocks overlap and jump, a short
  Vorbis file would end with a click of padding, and a chained file's clock
  would run backwards at each link.
- *libvorbisfile's and opusfile's timing throughout.* They are right where
  FFmpeg is wrong, but they disagree with FFmpeg -- and so with every player
  built on it -- in choices that are matters of taste (whether an Ogg Vorbis
  stream's first packet has a time at all, how an inconsistent granule is
  read), and the video stack is held to FFmpeg everywhere else.
- *Time FLAC, Speex and Theora now.* Nothing would use the times until their
  decoders exist; the FLAC decoder is on the music player's list.

**Where it lives.** `gui/video/ogg` (the demuxer; `src/stream.rs` says what
differs from FFmpeg and why, `tests/data/generate_fixtures.py` how each
difference is checked); `gui/video/codec`'s `container.rs` (the `Ogg`
container, libavformat's filling-in of untimed packets) and `sound.rs` (a
chained link's decoder); `gui/video/vorbis`'s `Blocks`, which reads a
packet's block size without decoding it; the Ogg sound fixtures in
`gui/video/codec/tests/data`.

**How to reverse.** Each difference from FFmpeg is one condition in
`gui/video/ogg/src/stream.rs` or `demux.rs`, and the generator's correction
for it one function: remove both and the answers are ffprobe's again.

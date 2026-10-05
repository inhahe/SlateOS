## 1355. MP3 is minimp3's decoder, ported to Rust and held to minimp3 -- but for two bits it misreads

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), for the music player's sound
(`roadmap-detailed.md`, "Music player": "Audio decoding via FFmpeg/libav FFI
(not custom decoders)" -- superseded for Opus, Vorbis and FLAC by §1350,
§1352 and §1354, and now for MP3 the same way).

**In short:** SlateOS can now decode MP3 -- the format most music still
comes in -- and its older cousins MP2 and MP1. The decoder is a Rust
translation of minimp3, a small public-domain decoder that passes the
standard's conformance tests and is used in many games and players. It
gives minimp3's own samples, bit for bit, on all 83 of minimp3's test
streams (the official conformance streams among them), on 42 files made
here by five encoders, and on 600 deliberately damaged files -- except for
two rarely-set bits of a frame that minimp3 misreads, which this decoder
reads as the standard says (below).

**What was decided.**

- **minimp3, translated.** It is public domain (CC0), about 1,800 lines,
  conformance-tested, and covers MPEG-1, -2 and -2.5, Layers I, II and III,
  free format. The MP3 patents expired in 2017. As for VP9, Opus, Vorbis
  and FLAC (§1339, §1350, §1352, §1354): no C compiler in the build, and
  memory safety where untrusted files arrive -- `gui/video/mp3` is safe
  Rust (`#![forbid(unsafe_code)]`).
- **Held to minimp3 built without SIMD** (`MINIMP3_NO_SIMD`), sample for
  sample: minimp3 rounds its output two ways, half to even in its SSE
  build and its own way in the scalar one, and the scalar one is the same
  on every machine. Every floating-point operation is minimp3's, in its
  order (a reordered sum rounds differently). `tools/reference.c` prints
  what minimp3 makes of a file a frame at a time (its samples hashed); the
  decoder prints the same lines; the tests compare them.
- **minimp3's sync, resync and refusals taken whole**: a frame is trusted
  when the next ten agree with it (or the input ends), the bit reservoir
  must reach back before a frame decodes, a damaged frame is skipped. A
  file salted with garbage loses more of its frames to this than FFmpeg
  would lose; that is minimp3's answer, and checkable.
- **Two departures from minimp3, applied to the reference too**
  (`tools/patch_minimp3.py`), so that the tests stay exact:
  1. *The intensity-stereo bit of the header's mode extension counts only
     in joint stereo.* minimp3 applies intensity stereo whatever the mode,
     while gating its own mid/side bit on joint stereo. In a mono frame
     with the bit set, minimp3 scales the one channel by memory nothing
     wrote; in a stereo or dual-channel frame, it mixes one channel into
     the other. The standard gives the mode extension meaning only in
     joint stereo, mpg123 reads it so, FFmpeg's stereo processing too.
     (One of minimp3's vectors, made by fuzzing, sets the bit in a
     dual-channel frame; its one such frame differs.)
  2. *A Layer III frame's private bits do not reach the first granule's
     scale factor selection*, where minimp3's shifting leaves them; a set
     private bit there copies scale factors from memory nothing wrote.
     Encoders write the private bits as 0, where the two agree.
  Where a damaged frame's Huffman codes run past the end of the main data,
  the decoder reads zeros where minimp3 reads the fields after its buffer,
  and its scratch memory persists between frames where minimp3's is
  whatever its stack held -- on 600 damaged files, the same samples all the
  same.
- **16-bit samples**, minimp3's output, as Opus and Vorbis give (§1351).

**Alternatives considered.**

- *FFmpeg's or mpg123's decoder* (LGPL, the condition F-Q7 and F-Q9 weigh):
  the most used, with their own sample-level differences and their own,
  more forgiving resync. *libmad* (GPL, fixed-point): a stronger condition.
- *Symphonia's MP3 decoder* (Rust, MPL-2.0): already Rust, but its own
  answers, held to no reference here, and a file-level licence condition.
- *minimp3's float output* (`MINIMP3_FLOAT_OUTPUT`), 24 bits' worth of what
  the filter computes: a quieter noise floor than 16 bits gives, but no
  reference decoder that a player's output is usually compared with gives
  it, and an MP3's own coding noise is far above 16 bits' floor.
- *The SSE build as the reference:* faster (the port's own speed is
  `known-issues/F-mp3-decodes-at-about-1.25-times-minimp3s-cost.md`), but
  machine-dependent output, and its half-to-even rounding differs from the
  scalar build's by one step on some samples.

**Where it lives.** `gui/video/mp3`; fixtures and their generator in
`tests/data`; minimp3's vectors through the ignored `tests/vectors.rs`.

**How to reverse.** `videocodec`'s `Sound` is its one user (§1351's MPEG
audio addendum). Either departure is one line in the crate and one in
`tools/patch_minimp3.py`.

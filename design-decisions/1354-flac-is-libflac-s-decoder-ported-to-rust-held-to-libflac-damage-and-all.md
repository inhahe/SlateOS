## 1354. FLAC is libFLAC's decoder, ported to Rust and held to libFLAC -- damage and all

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), for the music player's sound
(`roadmap-detailed.md`, "Music player": "Audio decoding via FFmpeg/libav FFI
(not custom decoders)" -- superseded for Opus and Vorbis by §1350 and §1352,
and now for FLAC the same way).

**In short:** SlateOS can now decode FLAC, the format most music collections
and every lossless download are in. The decoder is a Rust translation of
libFLAC, the reference library from FLAC's makers. FLAC is lossless -- a file
decodes to exactly what was recorded -- so "it works" is a yes-or-no question,
and the answer is yes on every test: 47 files made here and all 86 of the
IETF's official conformance files decode to libFLAC's samples, bit for bit,
and where a file is damaged, the decoder does what libFLAC does with the
damage, down to the silence libFLAC puts in for a lost frame.

**What was decided.**

- **libFLAC, translated.** It is the reference; its licence is BSD, with no
  condition on programs that contain it; FLAC has no patents. As for VP9,
  Opus and Vorbis (§1339, §1350, §1352): no C compiler in the build, and
  memory safety where untrusted files arrive -- `gui/video/flac` is safe
  Rust (`#![forbid(unsafe_code)]`).
- **Held to libFLAC on damage as well as on sound streams.** A FLAC decoder
  is exact or wrong on a valid stream; where they differ is a damaged one:
  which frames are given up, where the search for the next resumes, what a
  missing frame becomes. libFLAC's answers are taken -- a frame failing its
  CRC is dropped and the search resumes at its fourth byte; frames lost
  between two that decoded are filled with silence (up to five seconds or
  fifty blocks); a metadata block that breaks its rules ends the metadata --
  because they keep the timeline whole (a player's clock never jumps) and
  because "what libFLAC does" is checkable. `tools/reference.c` prints
  libFLAC's every metadata block, frame (its samples hashed), error and
  seek; the reader prints the same lines; the tests compare them.
- **Two faces.** `Reader` reads a `.flac` file whole (its tags and cover art
  in `Metadata`, its frames, a seek to any sample, the MD5 check);
  `Decoder` decodes one frame a packet, for the containers that carry FLAC
  (Ogg, Matroska, MP4).
- **Samples as `i32` at the stream's own depth** (4 to 32 bits), so that
  24-bit audio stays 24-bit; what the sound path turns them into is the
  caller's choice.
- **Two departures from libFLAC's C, both invisible on any stream it
  decodes the same way:** libFLAC skips its check on a Rice code too long for
  32 bits when the code happens to cross the end of its read buffer (a
  property of how the file was read, not of the file), where this checks it
  always; and a 32-bit sum that overflows in a damaged stream wraps here
  explicitly, where in libFLAC's C it is undefined behaviour that wraps in
  practice -- either way the frame then fails libFLAC's bounds check.

**Alternatives considered.**

- *FFmpeg's FLAC decoder* (LGPL, as F-Q7 asks of the MP4 reader): the same
  samples on valid streams -- FLAC is lossless -- but a condition on every
  program containing it, and its own, different handling of damage.
- *Written from RFC 9639 alone:* the format is fully specified, but damage
  is not, and libFLAC's answers are the ones every FLAC tool gives.

**Where it lives.** `gui/video/flac`; fixtures and their generator in
`tests/data`; the IETF corpus through `tools/ietf.py` and the ignored
`tests/ietf.rs`.

**How to reverse.** The crate stands alone; nothing else depends on it yet.

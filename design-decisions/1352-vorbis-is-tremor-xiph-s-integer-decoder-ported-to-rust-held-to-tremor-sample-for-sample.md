## 1352. Vorbis is Tremor, Xiph's integer decoder, ported to Rust and held to Tremor sample for sample

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), for the roadmap's "Sound needs Opus and
Vorbis decoders" (the video player's sound, and the music player's).

**In short:** SlateOS can now decode Vorbis, the sound in older WebM files
and in Ogg music files. The decoder is a Rust translation of Tremor, the
decoder Xiph (Vorbis's makers) wrote for machines without floating point.
"It works" means it produces exactly the same samples as Tremor: 44 test
streams are decoded whole, with their packets damaged eight ways each, and
with their setup header damaged 24 ways each, and every result -- every
sample, every refusal -- has to match Tremor's bit for bit. The test
streams are made here, so they are this project's to keep.

**What was decided.**

- **Tremor, not libvorbis.** libvorbis, the usual decoder, works in floating
  point, so its output depends on the compiler and a port of it could only
  be checked to within a tolerance (the reasoning of §1350, which chose
  libopus's fixed-point build for the same reason). Tremor is integer
  arithmetic throughout: one right answer on every machine, so the port is
  checked by equality. Its output is 24-bit fixed point (`decode_i32`, where
  1.0 is `1 << 24`), which `decode` turns into 16-bit samples as Tremor's
  `ov_read` does and `decode_float` into floats; its error against
  libvorbis's float decoder is far below what 16-bit output can carry.
- **A Rust port, not Tremor's C compiled in.** As for VP9 and Opus (§1339,
  §1350): no C compiler in the build, and memory safety where hostile input
  arrives. `gui/video/vorbis` is safe Rust (`#![forbid(unsafe_code)]`).
  Wherever Tremor's arithmetic wraps, the port wraps on purpose.
- **Where Tremor would crash, the port refuses.** A hostile setup header can
  make Tremor divide by zero (a lattice codebook of no dimensions, a floor 0
  with a sampling rate of 1, 32-bit floor amplitudes) or read through a null
  pointer (a codebook without values used for vectors). The port refuses
  the header, or ends the packet, instead. Where C's arithmetic is undefined
  (shifts of 32 or more), the port does what x86 does, taking the count
  modulo 32. One more difference, in memory only: Tremor tabulates every
  residue classification word's digits when it starts, a table a hostile
  header can make hundreds of megabytes; the port works each word's digits
  out as it reads the word, which gives the same digits.
- **The decoder is made from the identification and setup headers alone.**
  Tremor insists on the comment header (the tags) between them; FFmpeg,
  whose behaviour the video player follows (§1351), never reads it. So a
  file whose tags are damaged still plays, and the tags are read on their
  own, by `Comments::parse`, by whoever wants them.
- **No end trimming in the decoder.** Tremor's Ogg layer trims a stream's
  last packet to its final granule position; that is the container's
  business, done where the container is read, as for Opus (§1351).
- **The test streams are made here.** `tools/make_fixtures.py` encodes
  synthetic signals (tones with vibrato, a sweep, noise, clicks that force
  short blocks, silence) with libvorbis and with FFmpeg's own Vorbis
  encoder through FFmpeg, across 8 to 96 kHz, 1 to 8 channels and quality
  -0.1 to 10. Since no encoder writes floor 0, residue types 0 and 1,
  several submaps, list or sequence codebooks, or blocks of 64 and 8192,
  `tools/make_synthetic.py` writes 24 more streams with random but valid
  setup headers that use all of them, followed by random audio packets.
  `tools/references.py` runs Tremor over every stream through
  `tools/reference.c` and records what it makes of each
  (`tests/data/references.txt`, 1496 lines).

**Status, 2026-10-04: done.** All 1496 references match: the 44 streams'
headers and clean decodes, 352 decodes with damaged packets (lost, cut
short, flipped, replaced by noise, emptied, the decoder reset midway), and
1056 with damaged setup headers, of which Tremor refuses 918 where the port
refuses them too and takes 138 that the port decodes identically. Tremor
crashed on none of them. Measured with llvm-cov, the tests run 95% of the
port's lines; what they miss is chiefly refusal paths a single damaged
header does not reach. The MDCT is also held to a direct, pointer-for-pointer
translation of Tremor's at every block size and to the IMDCT's
floating-point definition (110 dB).

**Speed.** Counted with callgrind over the encoded test streams (27.6 s of
sound; `tests/bench.rs` and `tools/bench.c`): Tremor takes 627 M
instructions, the port 675 M; natively, about 460 times faster than real
time to Tremor's 520. The first translation took 955 M. Rewriting the MDCT
to walk the block in fixed-size chunks with twiddles at constant offsets,
reading eight bytes at a time in the bit reader, and giving residue type 2
fast paths for one and two channels closed most of the gap. What is left:
`known-issues/F-vorbis-decodes-at-about-1.08-times-tremors-cost.md`.

**Alternatives considered.**

- *libvorbis's float decoder, ported.* The usual one, but checkable only to
  a tolerance (above).
- *FFmpeg's Vorbis decoder, ported.* What the player's behaviour otherwise
  follows, but float as well, and LGPL.
- *The C library, compiled in.* No C compiler in the build (§1339).

**Where it lives.** `gui/video/vorbis` (the crate); `tests/streams.rs` (the
headers and clean decodes), `tests/damage.rs` (damaged packets and setup
headers), `tests/api.rs` (the API's promises), `tools/` (the generators and
the reference, built against Tremor's git master, libvorbisidec 1.2.1, and
libogg 1.3.6); the workspace `Cargo.toml` (its profiles, with the
measurements).

**How to reverse.** Swap the crate for bindings to Tremor or libvorbis, or
for a float port; `Decoder` takes the identification and setup headers and
packets, as any of them would.

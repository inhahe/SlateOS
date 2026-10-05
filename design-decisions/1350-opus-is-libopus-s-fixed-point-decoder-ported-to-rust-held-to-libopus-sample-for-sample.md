## 1350. Opus is libopus's fixed-point decoder ported to Rust, held to libopus sample for sample on streams of this project's own

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), for the roadmap's "Sound needs Opus and
Vorbis decoders" (the video player's sound, and the music player's).

**In short:** SlateOS can now decode Opus, the sound in most WebM files and
in most voice calls on the web. The decoder is a Rust translation of the
reference one (libopus 1.5.2). "It works" means it produces exactly the
same samples as libopus: every test stream is decoded about thirty ways
(every output rate, mono and stereo, with packets lost, with packets
damaged, with each decoder setting) and each result has to match libopus's
bit for bit, errors included. The test streams are made here from made-up
sounds, so they are this project's to keep in the repository.

**What was decided.**

- **libopus's fixed-point decoder, translated, not its float one.** libopus
  builds either way, and its float build is the usual one, but a float
  decoder's output depends on the compiler (fused multiply-adds, the order
  of sums, the vector width), so no two builds agree to the bit and a port
  could only be checked to within a tolerance. The fixed-point build is
  integer arithmetic throughout: one right answer, the same on every
  machine, so the port is checked by equality and a single wrong rounding
  anywhere shows. Its quality is the reference decoder's: the fixed-point
  build is what libopus's own conformance tests run, and RFC 6716's
  decoder conformance allows either. The cost is the float build's slightly
  wider dynamic range at very low levels, which nothing a player does can
  hear.
- **A Rust port, not libopus's C compiled in.** As for VP9 (§1339): the tree
  builds with no C compiler, and a decoder of hostile network input is
  exactly where memory safety pays. `gui/video/opus` is safe Rust
  (`#![forbid(unsafe_code)]`), with C's overflow kept explicit: wherever
  libopus's arithmetic wraps, the port wraps on purpose, so a crafted packet
  decodes to the noise libopus makes of it, never a panic.
- **The decoder only.** Nothing in the system writes Opus yet; an encoder
  waits for something that does (a recorder, voice chat).
- **Everything a file can hold.** Mono and stereo; SILK, hybrid and CELT in
  every bandwidth and frame size, with the switches between them; packet
  loss concealment and in-band FEC; DTX; multistream with every channel
  mapping family (1 surround, 2 and 3 ambisonics, 255 discrete); the
  `OpusHead` an Ogg or Matroska file describes its stream by. Not libopus's
  optional neural extras (DRED, the deep PLC, OSCE): they are off in its
  default build, and need model weights of megabytes.
- **The test streams are made here.** The RFC 8251 test vectors are the
  IETF's, distributed without a licence of their own, so they stay out of
  the repository. Instead `tools/make_streams.c` runs libopus's encoder over
  signals it synthesises (a pitched pulse train through moving formants
  for voice; chords and drum bursts for music; steady tones; silence,
  clipping, a whisper), set so that the decoder's paths run; the streams
  it wrote are in `tests/data` (1.3 MB). `tools/references.py` decodes each
  with libopus's own `opus_demo` and a twin of its loop that adds what
  `opus_demo` lacks (gain, phase inversion off, float output, resets,
  calls a player would not make), and records digests
  (`tests/data/references.txt`) of the samples, of every failed call's
  error code, and of the decoder's state after each packet; the tests
  decode the same way and must match all three. The RFC vectors still run,
  against digests in `tests/data/rfc8251.txt`, when `OPUS_VECTORS` names a
  copy. What streams reach too rarely -- a damaged packet's parse, a
  resonant signal's LPC, LSFs crowded together -- is held to libopus
  function by function over millions of made-up inputs
  (`tools/functions.c`, `tools/mathops.c`).

**Status, 2026-10-04: done.** 1036 decodes of 28 streams and a stream of
made-up packets, and all 360 of the RFC vectors', match libopus to the
bit; so do its packet parser over 200 000 made-up packets and its math,
LPC, pitch and LSF functions over millions of inputs. Measured with
llvm-cov, the tests run 98.9% of the decoder's lines. What they miss is a
check C makes that the Opus layer above it never lets fail -- but for one
path: CELT's LPC fit shrinks its coefficients when they outgrow 16 bits,
and no input found reaches that, the recursion's own stop at 30 dB of
prediction gain coming first even for resonant noise. Along the way the streams found
what the RFC vectors could not: in a multistream packet, a stream that
cannot use FEC (CELT) reported where it ended as 0, so the next stream
re-read its packet -- fixed, and tested by the surround streams with loss.

One place the port does not do what libopus does: `opus_packet_has_lbrr`
reads the first frame's first byte even when that frame is empty, which
past a packet's last byte is a read beyond the buffer. The port reads the
same byte where there is one, and where there is none says the packet has
no FEC (`packet_has_lbrr`).

**Speed.** On its benchmark (every test stream at 48 kHz, 151 s of sound;
`tests/bench.rs` and `tools/bench.c`), counted with callgrind: libopus's C
takes 4.03 G instructions, the port 4.46 G. That was 5.48 G until its
per-frame buffers were kept between frames (libopus's are on its stack,
uninitialised), and the de-emphasis, the ambisonics matrix, the comb
filter, the resampler and the FFT's simplest butterflies were written so
that their bounds checks drop out. Natively, 391 times faster than real
time to libopus's 426. What is left:
`known-issues/F-opus-decodes-at-about-1.11-times-libopuss-cost.md`.

**Alternatives considered.**

- *libopus's float decoder, ported.* The usual build, but uncheckable to
  the bit (above). A float port would be held to a tolerance, which lets
  small errors through: a rounding off by one in a rarely used path moves
  the output by less than any tolerance worth setting, where equality
  names the sample it first appears in.
- *The C library, compiled in* -- or FFmpeg's, which `roadmap-detailed.md`
  noted for the music player in the project's first commit. No C compiler
  in the build (§1339); and its memory-safety record would come with it.
- *The RFC vectors committed.* Simpler, but their licence is unstated, and
  they test less: no DTX, no FEC, no multistream, no damage.

**Where it lives.** `gui/video/opus` (the crate); `tests/streams.rs` (the
digests), `tests/damage.rs` (hostile calls), `tools/` (the generators, each
built against libopus 1.5.2 configured `--enable-fixed-point
--disable-intrinsics`); the workspace `Cargo.toml` (its profiles, with
the measurements).

**How to reverse.** Swap the crate for bindings to libopus, or for a float
port; the API (`Decoder`, `MultistreamDecoder`, `Head`) is libopus's own,
so callers would not change.

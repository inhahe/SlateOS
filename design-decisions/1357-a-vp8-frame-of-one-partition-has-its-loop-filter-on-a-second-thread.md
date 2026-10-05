## 1357. A VP8 frame of one token partition has its loop filter on a second thread, a row behind its macroblocks -- which libvpx does not do

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** most VP8 video is coded so that a frame cannot be split
among processor cores by rows (one "token partition": §1356 shares rows
only when there are several), and libvpx decodes such a frame on one core.
SlateOS's decoder now hands that frame's loop filter -- the smoothing
across block edges, about a third of the work -- to a second core, which
filters each row of 16-pixel blocks while the next decodes. The pictures
are the same, bit for bit; 1080p decodes 1.3 times as fast. This goes
beyond the reference decoder, so it is recorded.

**What was decided** (`gui/video/vp8/src/pipeline.rs`):

- **When:** a frame of one partition, with its loop filter on, of at least
  two threads' worth of macroblocks (§1356's 150 a thread), on a decoder
  allowed two threads (`Decoder::set_threads`; a new decoder is).
- **How:** the caller's thread decodes each row into a band (the copy of a
  row the row threads use, `band.rs`), keeping the row's unfiltered bottom
  pixels for intra prediction in the row below -- so it never reads the
  frame -- and sends the band on. A filter thread, which owns the frame,
  copies each band in, filters the row and fills the borders of the row
  above, final then. Eight bands circulate.
- **Why the pictures are the same:** filtering row `r` changes rows `r - 1`
  and `r` only, and nothing decoded after it reads the frame; libvpx's row
  threads rely on the same two facts.
- **The version-3 macroblock** whose chroma libvpx leaves as the buffer
  held it sends the frame back to one thread, as in §1356; only rows before
  it have been copied in.

**Rationale.** One-partition VP8 is the common case (encoders' default),
and on one core this port is about 60% of libvpx's SIMD speed
(`known-issues/F-vp8-on-one-thread-is-about-60-percent-of-libvpx-simd.md`);
a second core is the cheapest third of that gap to close, and changes no
picture. The machinery was already there for the row threads.

**Measured** (benchmark at high priority; frames a second, one thread
against two): 1080p 1.31 times, 720p 1.25, 640x360 1.17. Three bands in
flight gave 640x360 nothing -- a wake-up of the filter thread takes about
as long as decoding a row of so small a picture, so the decoding thread
caught up and waited; eight let the filter clear several rows a wake-up.

**Alternatives considered.**

- *One thread, as libvpx:* simplest, and leaves a free core idle while
  1080p decodes at 60% of libvpx's speed.
- *Filtering after the whole frame decodes, on several threads* (VP9's
  wavefront, `gui/video/vp9/src/loopfilter.rs`): the filter would start only
  when decoding ended, so a two-core machine gains the same at best, and
  the frame would be copied twice.
- *Three stages -- coefficients, reconstruction, filter:* would also split
  the 64% that decoding is; needs every macroblock's coefficients buffered
  between threads. Not done; noted in the known issue as what comes next.

**Where it lives.** `gui/video/vp8/src/pipeline.rs`; `band.rs` (shared
with `threading.rs`); `decodeframe::decode_mb_rows` chooses it for a frame
of one partition. Tests: `decoder::tests` (a one-partition vector is
filtered on the second thread, and every vector's buffers match one
thread's after every frame), `decodeframe::tests` (random modes and noise
partitions, one partition among them), `tests/vectors.rs` and
`tests/damage.rs` at several thread counts.

**How to reverse.** Remove the `[one] =>` arm in
`decodeframe::decode_mb_rows`: one-partition frames then decode on one
thread, to the same pictures.

## 1356. VP8's row threads share no pixels: each row decodes into a band of its own, and what crosses between rows goes through mailboxes of atomic words

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a VP8 frame coded in several "token partitions" (separate
streams of coefficients, one per row of 16-pixel blocks in turn) can be
decoded several rows at once, each row a few blocks behind the one above,
as libvpx does. SlateOS's VP8 decoder now does this, with the same
pictures as on one thread, bit for bit. libvpx's threads all write into one
shared frame and watch each other's progress; in Rust that needs `unsafe`
code, which the decoder forbids. Instead each thread decodes its row into
a private copy and writes it into the frame piece the row owns; what a row
needs from the row above -- its bottom pixels, unfiltered, and later its
filtered last rows -- is posted in a small shared mailbox the row below
reads. On the 1080p test film in eight partitions this is 2.9 times as fast
on eight threads as on one (2.5 times at 720p, 1.6 at 360p); pictures too
small to share -- under about 150 blocks a thread -- stay on one.

**What was decided** (`gui/video/vp8/src/threading.rs`):

- **Rows as libvpx assigns them:** thread `t` of `n` takes rows `t`,
  `t + n`, ...; `n` at most the partition count and the row count, as
  libvpx clamps it; any `n`, not only powers of two. A row's partition
  passes to the next row that reads it under a mutex that, because rows
  finish in order and `n` is at most the partition count, is never
  contended. `Decoder::set_threads`; a new decoder uses every core, as
  VP9's does -- but a thread only for each 150 macroblocks of the frame,
  below which a thread costs more than it saves: 320x180 (240
  macroblocks) was slower on any number of threads than on one, 480x270
  (510) gained 10-15% on three, 640x360 (920) 1.6 times on six.
- **A band per thread:** the row and the four rows above it that its loop
  filter reaches. Each macroblock is filtered as soon as the one to its
  right is decoded; the row below predicts from a copy of this row's bottom
  pixels taken before they are filtered (libvpx's `mt_yabove_row`), and
  receives the last four rows of each macroblock once final, filtering its
  top edges into them.
- **The frame cut into one piece per row** -- the row's pixels but its last
  four, and the last four of the row above, on which the row holds the last
  word -- written by the row's thread as it finishes.
- **Mailboxes of atomic words, not channels:** per row, a progress count
  (released after the words it promises; libvpx's `mt_current_mb_col`) and
  each macroblock's token contexts, bottom row and last rows. A reader
  spins about a microsecond, then sleeps until the row it waits on wakes
  it. It never yields: on a busy machine Windows' `SwitchToThread` hands
  the core to another process for the rest of a time slice, which made
  176x144 frames take 35 ms on two threads against 0.4 ms on one.
- **The version-3 macroblock whose chroma libvpx leaves as the buffer held
  it** cannot be decoded in a band, which holds nothing of the buffer. A
  thread meeting one stops the others and the frame decodes again on one
  thread -- which finds the buffer as the frame found it, because a piece
  is written only once its rows are finished, and a finished row has none.

**Why.**

- *No `unsafe`.* The decoder takes untrusted files; `#![forbid(unsafe_code)]`
  is what holds a damaged one to garbage pictures rather than memory
  corruption (§1339). libvpx's shared frame, written by every thread at
  rows' boundaries, cannot be expressed without it.
- *Measured against the alternatives* (1080p film, eight partitions, a
  machine at about 90% load from other work, best of interleaved rounds;
  speed-ups over one thread):

| Design | 2 threads | 4 threads | 8 threads |
|---|---|---|---|
| a channel message per macroblock (first version) | 1.16-1.25x | 1.8x | 2.5x |
| mailboxes, spinning and yielding as libvpx does | 1.33x | 1.95x | 1.4x |
| mailboxes, spinning briefly, then sleeping (this) | 1.2-1.4x | 1.9-2.0x | 2.5-2.9x |

  The last row's range runs from that load to a quiet machine (the
  benchmark at high priority): 13.98 ms a frame on one thread, 4.83 on
  eight.

  A message per macroblock cost 100-160 ns to send, plus moving 200-byte
  messages and waking a parked receiver; spinning without end starved the
  rows waited on once threads outnumbered free cores -- libvpx's own
  threads ran slower on eight than on four here (161 against 245 frames a
  second).
- *The fallback costs nothing in a real stream.* The macroblock that takes
  the frame back to one thread needs a version-3 stream with a vector far
  outside the picture; libvpx's encoder never makes one. Snapshotting the
  buffer's prior chroma for every frame, so that bands could reproduce it,
  would cost every frame for a case no encoder produces.

**Alternatives considered.**

- *VP9's loop-filter wavefront (`gui/video/vp9/src/loopfilter.rs`): bands
  kept until the frame is done, then copied back by one thread.* The copy is a whole
  frame, serial; writing each row's piece as it finishes spreads it over
  the threads.
- *Thread counts that divide the partition count only*, so that each thread
  owns its partitions outright: no mutex, but four threads, not six, on a
  six-core machine for an eight-partition stream.
- *A persistent thread pool* rather than scoped threads made per frame: the
  threads' start costs about 2-4% of a 1080p frame on eight threads; a pool
  that borrows the frame needs `unsafe` or a dependency (rayon) that the
  workspace does not have. Not done.
- *Decoding the next rows' modes while rows decode:* the modes (6% of a
  one-thread frame, 13% of an eight-thread one) are read before any row, in
  libvpx too. Not done; a possible next step.

**Where it lives.** `gui/video/vp8/src/threading.rs` (all of it);
`decodeframe::decode_mb_rows` tries it first; `inter::Prior` is the
fallback's signal; `decoder::Decoder::{set_threads, threads}`. Tests:
`tests/vectors.rs` and `tests/damage.rs` at several thread counts, and
`decoder::tests` and `decodeframe::tests`, which compare every buffer,
borders included, between thread counts.

**How to reverse.** `Decoder::set_threads(1)`, or delete the call at the
top of `decodeframe::decode_mb_rows`: everything then decodes on one
thread as before, to the same pictures.

## 1342. The VP9 encoder's threads are its tile columns: the same bytes on any number of threads, and as many columns as the width allows by default

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), within §1332 (a CPU encoder "multithreaded
according to the number of cores", the operator's F-Q2 answer) and §1339 (the
encoder's frames are `vpxenc`'s byte for byte).

**In short:** a VP9 picture can be cut into tile columns -- vertical strips
coded independently of each other -- and the encoder codes each column on a
thread of its own. Whatever the number of threads, the frames are exactly the
ones one thread makes, and exactly `vpxenc`'s with the same columns. By
default a picture gets as many columns as its width allows (four at 720p and
1080p, eight from 1440p up), as `vpxenc`'s default does, so the encoder uses
up to that many cores without being asked. Pictures narrower than 512 pixels
have one column and encode on one thread.

**What was decided.**
- The unit of parallel work is the tile column (libvpx's
  `vp9_encode_tiles_mt`). Each column is coded into a strip of the
  reconstruction of its own, with its own block grid, vectors, counts and
  tokens, and with its own copy of the state libvpx's decisions keep from
  block to block and frame to frame (`RtState`, the cyclic refresh's maps);
  afterwards the strips are pasted and everything is put back in column
  order. No column reads another's state, so the copies are exact.
- libvpx keeps the mode search's adaptive thresholds per tile
  (`tile_data->thresh_freq_fact`); so does the port now. With one shared
  table, tiled frames differed from `vpxenc`'s from the fourth frame on.
- The columns' bitstreams are written on threads too (2026-10-05), as
  libvpx's realtime mode does (`encode_tiles_mt`). They cannot be written as
  the columns are coded, since the token probabilities they are written with
  come from the whole frame's counts. One thread writes a column's tiles,
  top to bottom, because VP9 carries the above contexts down a column from
  one tile row to the next. Each thread has contexts of its own, since a
  column touches only its own span of them. libvpx writes in parallel only
  when there is one tile row; the port handles tile rows too, and puts the
  tiles back in row order. Packing was 5-8% of a frame's instructions on one
  thread (callgrind), the share that stayed serial.
- `EncoderConfig::realtime` sets `tile_columns: 6`, `vpxenc`'s default, which
  the width clamps. The reference encodes that were made with
  `--tile-columns=0` say so in their tests. `Encoder::set_threads` caps the
  threads, as `Decoder::set_threads` does.

**Why.** The encoder's correctness check is byte-identity with `vpxenc`. libvpx's
tile threading keeps it: `vpxenc` with four tile columns makes the same file on
one thread and on four (measured; likewise two columns on one and two threads),
and the port now matches those files on one, two, three and four threads. A
column costs a little compression -- 135,422 bytes against 134,977 for the
first reference's 30 frames in four columns, 0.3% -- which is the price of the
parallelism and the reason `vpxenc` still offers one column.

**Alternatives.**

| | For | Against |
|---|---|---|
| Tile columns (chosen) | the same bytes on any number of threads, `vpxenc`'s; speed-up close to the column count | a picture under 512 pixels wide has one column, so one thread; 0.3% larger at four columns |
| libvpx's row multithreading (`--row-mt=1`), which splits a column's superblock rows across threads | small pictures use cores too | its frames on several threads differ from one thread's (measured: other bytes, another size), so they can be checked against no `vpxenc` file and differ between machines with different core counts |
| Several frames at once | none in a realtime encoder: each frame's rate control, references and adapted probabilities depend on the frame before | -- |
| `realtime()` keeping one column, threads only on request | the default matches the reference encodes exactly | an OS encoder that leaves the cores idle unless every caller knows to ask, against §1332's "multithreaded according to the number of cores" |

**Where it lives.** `gui/video/vp9/src/enc/encoder.rs`
(`encode_columns_threaded`, `FrameDecisions`, `EncoderConfig::tile_columns`,
`Encoder::set_threads`); `enc/encodeframe.rs` (`FrameEncoder::for_tile_column`,
`column_strip`, `merge_columns`, the reconstruction read through
`recon_at`); `enc/nonrd.rs` (`RtState::absorb_columns`, the per-tile
thresholds); `enc/aq_cyclicrefresh.rs` (`CyclicRefresh::absorb_columns`);
`enc/bitstream.rs` (`write_tiles`, the columns' bitstreams).
Tests: `tests/encoder.rs`, `frames_match_vpxenc_in_tile_columns` and
`frames_match_vpxenc_through_cuts_in_tile_columns`.

**How to reverse.** For one column by default, set `tile_columns: 0` in
`EncoderConfig::realtime` (the tests already pin their references' setting).
To drop the threads, take the single-thread path in `encode_frame` always; the
column path is self-contained.

**Revisit when** the compositor's capture stream encodes small windows (under
512 pixels wide), where tile columns give no parallelism: a deterministic
superblock-row wavefront within a column would be the thing to measure then.

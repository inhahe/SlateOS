## 1230. The 7z reader decodes LZMA with 7-Zip's own decoders, as multi-threaded 7-Zip does

**Date:** 2026-10-03
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** the `sevenz` crate does not decode LZMA and LZMA2 with the
workspace's `xz` crate (a port of liblzma). It carries its own port of the
decoders 7-Zip itself uses. The two agree on every sound archive, but on a
damaged one they give back different amounts of data, and so different
per-file verdicts ("Data Error" against "CRC Failed" against fine). By
default it decodes LZMA2 the way 7-Zip does on an ordinary multi-core PC; a
caller can ask for the way 7-Zip does with one thread instead.

### What was decided

- `sevenz/src/lzma_dec.rs` and `lzma2_dec.rs` port `C/LzmaDec.c` and
  `C/Lzma2Dec.c` from the LZMA SDK 26.00 (public domain); `lzma_coder.rs`
  ports what drives them for a 7z folder: `LzmaDecoder.cpp`,
  `Lzma2Decoder.cpp`, `Lzma2DecMt.c` and `MtDec.c`, the thread pool
  simulated by decoding its blocks one after another.
- `sevenz::Threads::Many` is the default; `Threads::One` is
  `Archive::set_threads`. A packed header is always decoded as `One`,
  because 7-Zip decodes headers with one thread.

### Why not the `xz` crate

Measured on 3,585 one-byte corruptions of three small archives: with
liblzma's decoder, 173 got a different verdict from 7-Zip 26.00's.
Two causes:

- A match that runs past the folder's end. liblzma refuses the whole match.
  7-Zip copies it up to the end, and only then reports the stream damaged.
  So with 7-Zip the files that match completes are judged by their CRCs;
  with liblzma they are lost.
- The input of an LZMA2 chunk. liblzma's range decoder reads on past the
  chunk's declared packed size. 7-Zip gives it exactly that size and no
  more.

With the port, all 3,585 agree, and so do the chunk headers and chunk
edges of two archives of several LZMA2 chunks (4,289 mutants in all), in
both thread modes.

The cost is a second LZMA decoder in the workspace. Merging the two would
make one of them wrong against its own reference on damaged data, so each
stays held to its own: `xz` to liblzma, `sevenz` to 7-Zip.

### Why multi-threaded 7-Zip by default

With more than one thread, 7-Zip's LZMA2 decoder first walks the chunk
headers to cut the stream into blocks at dictionary resets, then decodes
each block to the size the walk found. With one thread it decodes the
stream in sequence. On a sound stream the results are identical; on 16 of
the 4,289 mutants they are not. An example: a chunk whose declared packed
size runs past the end of the stream ends the walked block before the
chunk, while decoding in sequence decodes the chunk and only then finds the
stream short.

| Option | What changes |
|---|---|
| `Many` by default (chosen) | Verdicts match 7-Zip as most people run it, on a PC with several cores. |
| `One` by default | Verdicts match `7z -mmt=off` and single-core machines, and a few damaged archives give back more data. |

The point of the port is agreeing with the 7-Zip a user would compare
against, so `Many` is the default. `One` stays available and is tested
against its own verdicts.

### Found on the way

7-Zip 26.00 itself sometimes crashes (an access violation, exit
`0xC0000005`) testing some of these damaged multi-block archives with
several threads -- about one run in twelve on the worst of them, the other
runs agreeing with each other and with this port. `generate.py` reruns a
crashed run. The port is memory-safe, and a damaged archive is only ever a
"Data Error" here.

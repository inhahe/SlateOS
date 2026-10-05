## 1039. DEFLATE is decoded with lookup tables, and the bit-at-a-time decoder it replaced is kept as the tests' oracle

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** opening a compressed file -- a PNG picture above all -- spends
most of its time undoing the compression, and our decoder did that one bit at
a time, about seven times slower than the library Python uses (zlib-ng). It now
decodes the way zlib, zlib-ng and libdeflate do, by looking several bits up in
a table at once: on a photograph's 9 MB of pixels, 280 ms became 49 ms, against
zlib-ng's 36 ms, and reading the picture a row at a time, 334 ms became 57 ms.
Nothing a program sees changes -- the same bytes, and when a file is damaged,
the same error after the same bytes -- and the old decoder stays, only in the
tests, as the judge of that.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **Tables, a fast loop and a careful one, the walk kept as the tests' oracle (chosen)** | the same results, five to six times faster | the oracle is the code being replaced, so "nothing changes" is tested, not argued: every valid stream, 450 mutations and 120 truncations of each of 28 streams, 20 000 noise streams, and 50 000 single-symbol decodes over random codes, one-shot and streamed at many read sizes | two decoders to keep in the crate, one only for tests |
| Tables, with only round-trip and fixture tests | the same speed | less code | a table decoder can tell an error sooner than the walk did -- an unassigned code, a code cut short by the end of the input -- and would move where a damaged file stops; nothing but the old decoder can say where it stopped |
| Keep the walk, tune it | a smaller win | one decoder | the walk's cost is the loop per bit itself |

**The three rules that make a table stop where the walk stopped**, each held
by the oracle tests and stated in `inflate.rs`: an unassigned code is an error
only once 15 bits have been read (the walk read to the longest length first),
so with fewer left it is `UnexpectedEnd`; a code longer than the bits left is
`UnexpectedEnd`; and checks the walk made per byte (the output limit, a stored
block's input, the caller's buffer) are made per run, the run cut where the
first would have failed.

**Choices inside it, and why.** First-level tables of 11, 8 and 7 bits
(literal/length, distance, code-length code) with second-level tables under
them, as libdeflate sizes them; the first level a fixed-size array, which
took the raw decode from 52 to 44 ms on the benchmark, because a masked index
then needs no bounds check. A 64-bit bit buffer refilled eight bytes at a time, spent on one
length/distance pair or up to three literals per refill. The stream decodes
into the caller's buffer and copies a back-reference from that buffer where it
can, folding the finished run into its 32 KiB window once (zlib's
arrangement), instead of writing every byte twice. Adler-32 stays a plain byte
loop: a sixteen-bytes-at-a-time version measured slower (9 ms against 5) on
baseline x86-64, which has no 32-bit vector multiply.

**Not changed:** `fixed_buffer.rs`'s two decoders, which reproduce where zlib
and libdeflate stop in a fixed-size buffer, keep their own walks; they are
held to those libraries' answers, not to speed.

**Where:** `deflate/src/inflate.rs` (the decoder), `deflate/src/puff.rs` (the
oracle, test-only), `deflate/tests/bench.rs` (the timing, run on demand).
Asked for by lane F in
`requests/f-ab-inflate-decodes-a-bit-at-a-time-five-times-slower-than-zlib-ng.md`.

**Revisit** if a caller needs the last third: the remaining gap to zlib-ng is
mostly bounds-checked stores and `Vec::push`, which an `unsafe` output cursor
would remove -- a trade this crate, which runs in the kernel on untrusted
input, has so far declined.

---

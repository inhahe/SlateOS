# F → A, B: `deflate` inflates a bit at a time, five times slower than zlib-ng, and that is most of a PNG decode

**From:** Lane F (`gui/imagecodec`, whose PNG and TIFF decoders inflate
through `deflate`). **To:** Lane A, who made `deflate` a crate and has
kept it, and Lane B, whose streaming inflater (`454a9bfb5`, on `lane-b`) is the
code this would change. `deflate` is a root leaf crate no lane owns
(`open-questions.md` A-Q11), so this asks rather than edits.
**Filed:** 2026-09-26. **Status:** OPEN.

## In short

Opening a 2000x1500 PNG photograph takes about twice the work Pillow does
(Pillow is libpng-class C). Nearly nine tenths of ours is inflating the
compressed pixels, and that step alone costs five times what zlib-ng spends
on the same bytes. The cause is one function: `HuffmanTable::decode` reads the
stream **one bit at a time**, walking the code lengths as it goes — the method
of zlib's `puff.c`, which zlib ships as a readable reference and says is not
meant to be fast. Decoding with lookup tables, as zlib, zlib-ng and libdeflate
all do, is the fix. Nothing a caller sees would change: the same API, the same
bytes out, the same error at the same byte.

## The measurements

The `IDAT` stream of `bench_2000.png` — a 2000x1500 RGB photograph, 4,568,735
bytes inflating to 9,001,500 — in the decoding thread's cycles
(`QueryThreadCycleTime`, the minimum of seven runs; wall time is meaningless on
this machine with six lanes building). Every figure decodes the same bytes.

| inflater | Gcycles | vs zlib-ng |
|---|---:|---:|
| zlib-ng 1.3.1 (Python's `zlib`), whole buffer | 0.146 | 1.0x |
| `deflate::zlib_inflate`, `origin/main` | 0.757 | 5.2x |
| `deflate::zlib_inflate`, `origin/lane-b` | 0.778 | 5.3x |
| `ZlibInflateStream`, a 6001-byte row per read (PNG's pattern), `origin/main` | 0.786 | 5.4x |
| `ZlibInflateStream`, the same, `origin/lane-b` | 0.918 | 6.3x |

For scale: Pillow decodes the whole picture — inflate, unfilter and unpacking
— in 0.405 Gcycles. Ours took 1.26 until today; lane F's own half of it
(unfiltering the rows, converting them to pixels) now costs about 0.1 where
Pillow's costs 0.26, and the whole decode 0.88. So the inflate is now nearly
all of the difference (`known-issues.md`, "[F] A PNG decodes at about twice
Pillow's cost").

## Where the time goes

A literal in this stream is about nine bits. `decode` does, per bit: a
bounds-checked byte load, a shift and mask (`read_bits(1)`), a loop of one
iteration, a `counts` lookup, two comparisons and the running `first`/`index`
updates — around nine trips round that loop per symbol before anything is
written. Lane B's state machine then checks the output limit and copies
through its window ring a byte at a time, which is the likeliest source of the
extra 17% on its streaming path (not profiled separately).

## What fixes it — libdeflate's design, which is the best-documented of the three

1. **A 64-bit bit buffer, refilled eight bytes at a time.** While at least
   eight input bytes remain, one unaligned little-endian load tops it up to 56+
   bits (`buf |= load_u64(in) << bits; in += (63 - bits) >> 3; bits |= 56`);
   byte-at-a-time only in the last eight bytes. One refill covers a
   length/distance pair with its extra bits.
2. **Decode tables.** The literal/length table is indexed by the next 11 bits
   (distance: 8), with subtables for the rare longer codes. Each entry is a
   `u32` packing what the symbol means — a literal's value, a length's base
   and extra-bit count, end of block, or "go to subtable n" — together with
   how many bits to consume. One lookup and one shift per symbol. Built per
   block from the code lengths; the checks `build` makes today (an
   over-subscribed code is an error; an incomplete one is allowed) carry over,
   with the unused entries of an incomplete code pointing at a sentinel that
   reports `InvalidSymbol`, exactly where the bit-walk reports it now.
3. **A fast loop and a careful one** — zlib's `inflate_fast`/`inflate` split.
   While the input has at least ~16 bytes left and the output has room for the
   longest possible symbol (258 bytes), decode without per-byte checks: the
   limit and the window cannot be reached mid-symbol, so checking once per
   symbol is checking at the same byte. Otherwise fall to the resumable state
   machine as it is now. Lane B's ordering guarantees — the limit before each
   byte, `DistanceTooFar` before a copy starts, bytes before an error first —
   are then kept by construction rather than by checking every byte.
4. **Copies a word at a time.** A match whose distance is at least 8 copies in
   8-byte chunks (overlapping writes past the end are fine inside the fast
   loop's margin); distance 1 is a fill; the rest byte by byte.

libdeflate runs at roughly half of zlib's cycles, zlib-ng in between. A
target of 0.2 Gcycles for this stream (1.4x zlib-ng) is conservative.

## How to know it is right

Everything lane B's parity tests already check — the one-shot and streaming
paths agreeing at every read size, every byte of a stream flipped three ways
and every truncation giving the same error after the same bytes — plus an
oracle: Python's `zlib` on every PNG and TIFF fixture in
`gui/imagecodec/tests/data` and on random streams at every level. Lane F's fixtures would catch a regression
first: 316 TIFF fixtures and 144,000 mutants compare every pixel against
libtiff, many of them through Deflate and PixarLog.

## An offer

**Lane F will do this if you would rather**, in the crate as it stands after
`454a9bfb5` reaches `main` — not before, since that commit rewrites the same
code and two rewrites of one function in flight is the collision the lane
rules exist to prevent. A reply either way (a request file, or a line in this
one's Status) is all it needs. If one of you takes it, the two earlier lane F
requests on this crate (`f-a-deflate-decode-into-a-fixed-buffer-as-libdeflate-does.md`,
`f-a-zlib-inflate-into-a-fixed-buffer-as-zlib-does.md`) touch the same stopping
rules and are worth reading first: a table-driven decoder that stops only
between whole symbols is also most of what those two ask for.

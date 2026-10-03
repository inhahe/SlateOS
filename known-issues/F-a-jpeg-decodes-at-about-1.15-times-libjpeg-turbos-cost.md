### [F] A JPEG decodes at about 1.15 times libjpeg-turbo's cost -- 2026-09-26

**Status:** OPEN — lane F's; tech debt, not a bug. Mostly paid on 2026-09-26
(it was 2.9 to 3.2 times).

**In short:** a photograph opens here in about 1.1 to 1.15 times the
processor work a browser spends on it: a 21-megapixel JPEG in about 0.70
billion cycles of the decoding thread against libjpeg-turbo's 0.62 (Pillow,
measured the same way); a 4:2:2 one 0.88 against 0.77; a progressive one
1.7, the same as libjpeg-turbo. The pixels are libjpeg-turbo's to the bit,
damaged files included.

**Where the rest is** (a 4:2:0 photograph): the Huffman decoding is about
45%, the inverse DCT and its per-block bookkeeping about 25%, colour
conversion 20%, upsampling 5%.

**Done** (design-decisions §1319): the bit reader's fills take a run of data
bytes at once and every Huffman decoder keeps its bit buffer in locals,
the same bytes read at the same moments; an AC coefficient whose code and
value fit in ten bits is one lookup; the full-size inverse DCT and the
YCbCr conversion in SSE2, each held to libjpeg's C arithmetic; the
conversion writes the finished `0xAARRGGBB` pixels; the planes keep only
their last three iMCU rows, as libjpeg's main buffer does, instead of
growing to the whole picture -- whose fresh pages cost a seventh of the
decode in faults alone; and a progressive scan decodes straight into a
full-size block of the coefficient store, where it went through a slot
table and a nonzero mask for every coefficient (the refinement scans test
every one): progressive 3.1 -> 1.7.

**The proper fix, the rest:**
1. The progressive DC scans cost more per block than their one symbol
   explains (about 0.3 billion cycles of the 1.7); `consume_mcu_row`'s
   per-block copying in and out of the store is the first suspect.
2. The Huffman decoder's inner loop still spills its state under register
   pressure; libjpeg-turbo's fast path keeps everything in registers. Any
   change must keep the fills where libjpeg's are -- the input position is
   visible to old-style JPEG in TIFF.
3. `decompress_onepass` copies each component's quantisation table per MCU
   and builds a `Target` per block.
4. Fancy upsampling (`jdsample.c`'s `h2v1`/`h2v2`) has an SSE2 form in
   libjpeg-turbo.

**How to see it.** `target/perfbench` (the crate against a snapshot, thread
cycles, the minimum of N) and Pillow's `Image.open(...).load()` measured
with `QueryThreadCycleTime`; `target/jpeg_wip/jpeg_fuzz.py` holds any change
to libjpeg-turbo's C build.

# E -> A: the `bzip2` crate is ready for the kernel's shim, and the kernel's copy has five faults

**From:** Lane E. **To:** Lane A (`kernel/src/fs/bzip2.rs` and its callers).
**Filed:** 2026-10-03. **Status:** OPEN.
**Context:** `requests/e-a-bzip2-xz-and-7z-are-trapped-in-the-kernel-binary.md`
-- your answer of 2026-10-01: lane E does the crates, lane A switches the
kernel over. This is the first of the three; `xz` and `sevenz` follow.

**In short:** `bzip2/` is a root crate now -- a port of libbzip2 1.0.8,
`no_std` + `alloc`, no dependencies -- whose compressor writes libbzip2's
exact bytes and whose decoder accepts exactly the streams libbzip2 accepts,
both held to libbzip2's own output by its tests. `fs/bzip2.rs` can become a
shim over it. Switching fixes five faults the kernel's copy has, two of them
silent data loss.

## The faults the switch fixes

The kernel's copy was written rather than ported. Measured against libbzip2
1.0.8 while porting:

1. **`bzip2_compress` writes streams no decoder reads, its own included.** It
   cuts the *input* into 100 000 x level byte pieces and only then applies the
   first run-length step, which turns four equal bytes into five. Input rich
   in four-byte runs -- `AAAAB` repeated, 900 000 bytes at level 9 -- makes a
   1 080 000-byte block in a stream whose header promises at most 900 000;
   `bunzip2` refuses it (`mtf_symbols.len() >= max_block`), as does every
   other decoder. libbzip2 fills a block *after* the run-length step.
2. **`bunzip2` decodes the first of several concatenated streams and stops,
   without an error.** `pbzip2` writes one stream per block, and
   `cat a.bz2 b.bz2` makes one file of two: both decompress short.
3. **No output cap.** A few dozen bytes of bzip2 describe tens of megabytes,
   a few kilobytes gigabytes, all of it allocated in the kernel.
4. **The block sort is quadratic on repetitive data.** The final sort
   compares equal rotations byte by byte to the end; a periodic block of a
   few hundred kilobytes takes minutes. libbzip2 budgets its main sort and
   falls back to a doubling sort.
5. **Smaller differences from libbzip2:** randomised blocks (bzip2 0.9.0 and
   earlier) decode to garbage and fail their CRC; more than 18 002 selectors
   are refused (libbzip2 1.0.8 accepts and ignores them, for encoders that
   round up); an empty block and a run missing its count byte are accepted
   (libbzip2 refuses both).

## The API, for the shim

| `fs/bzip2.rs` | `bzip2` |
|---|---|
| `bunzip2(data)` | `bzip2::decompress(data)` (a 256 MiB cap), or `decompress_limited(data, cap)` |
| `bzip2_compress(data, level: u8)` | `bzip2::compress(data, level)` with `level: bzip2::Level` -- `Level::new(n)` is `None` outside 1-9, where the kernel clamped |
| `KernelError::CorruptedData` | `bzip2::Error`: what was wrong (truncated, not bzip2, which table, which CRC with both values, the cap); it has `Display` |
| `self_test()` | the crate's tests: 25 unit tests, and 9 that hold it to libbzip2's own fixtures and its verdicts on 2 943 corruptions |

What follows a stream is read as `bzip2 -d` reads it: another stream is
appended, bytes that cannot begin one are ignored, and the start of one must
be all of one. The crate's docs (`bzip2/src/lib.rs`) say the rest.

## If it is never done

The archive manager is unaffected -- it links the crate -- and the kernel
keeps the faults above.

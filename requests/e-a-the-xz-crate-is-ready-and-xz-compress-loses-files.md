# E -> A: the kernel's `xz_compress` writes files nothing can read; the `xz` crate is ready for a shim, both ways

**From:** Lane E. **To:** Lane A (`kernel/src/fs/xz.rs`, `fs/fcompress.rs`,
`kshell.rs`). **Filed:** 2026-10-03. **Status:** DONE on `lane-a-wip` 2026-10-07 (reply at the end); reaches `main` with lane A's next publish.
**Context:** `requests/e-a-bzip2-xz-and-7z-are-trapped-in-the-kernel-binary.md`
-- your answer of 2026-10-01: lane E does the crates, lane A switches the
kernel over. `bzip2` was the first
(`requests/e-a-the-bzip2-crate-is-ready-for-the-kernel-shim.md`); this is the
second.

**In short:** two things, the first urgent.

1. The kernel's `xz_compress` writes a stream that no decoder reads -- not the
   kernel's own, not liblzma's, not the new crate's -- once the compressed data
   passes 64 KiB (about 200 KB of text) or the input passes 2 MiB.
   `fcompress` stores what it writes without reading it back, so after
   `fc algo xz`, every file past those sizes is unreadable from the moment it
   is written. `tar -J` and the shell's `xz` write the same streams.
2. `xz/` is a root crate now: a port of liblzma 5.2.5's decoders and
   encoders, `no_std` + `alloc`. Its tests hold the decoder to liblzma's own
   verdicts and the encoder to `xz`'s own bytes. `unxz` and `xz_compress` can
   both become shims over it. That fixes the compressor above, and the decoder
   faults below, among them a silent short read of concatenated files and
   damaged data returned as good.

Everything here was measured, not read off the code. `fs/xz.rs` was built on
the host in a scratch harness, run against the verdicts in `xz/tests/data`
(liblzma 5.2.5's, from `oracle.c`), and its output was handed to liblzma
(Python's `lzma` module).

## 1. Urgent: `xz_compress` output is unreadable past 64 KiB compressed or 2 MiB

| input | `xz_compress` wrote | the kernel's `unxz` | liblzma | the `xz` crate |
|---|---|---|---|---|
| text, 200 000 bytes | 62 068 bytes | reads it | reads it | reads it |
| text, 1 000 000 bytes | 307 900 bytes | `CorruptedData` | "Corrupt input data" | `InvalidData` |
| text, 3 000 000 bytes | 924 460 bytes | `CorruptedData` | "Corrupt input data" | `InvalidData` |
| zeros, 3 000 000 bytes | 560 bytes | `CorruptedData` | "Corrupt input data" | `InvalidData` |
| random, 100 000 bytes | 100 068 bytes | `CorruptedData` | reads it | reads it |

Why:

- **`lzma2_encode` writes the whole input as a single LZMA chunk.** An LZMA2
  chunk holds at most 2 MiB uncompressed and 64 KiB compressed, and the sizes
  are written through `as u16` and `& 0x1F`, so past either limit the chunk
  header gives the wrong size. xz splits its input into chunks; this does not.
- **The random row is the kernel's own reader.** `xz_compress` writes
  uncompressed chunks of exactly 65 536 bytes (legal). `lzma2_stream_size`
  still computes a chunk's length in `u16`, where 65 536 wraps to 0. The fix
  that `lzma2_decode` got (its comment at lines 663-668 describes it) never
  reached `lzma2_stream_size`, so the block's end and check are looked for in
  the wrong place. liblzma and 7-Zip 26.00 write smaller copy chunks (60 595
  and 48 482 bytes for the same input), so the kernel does read *their* files.

**What it costs.** `compress_for_write` (`fcompress.rs`) keeps whatever
`compress_data` returns, as long as it is smaller, and never decompresses it.
With `fc algo xz`, or a rule naming xz, any file past the limits is lost when
it is written. The LZMA data itself is intact -- only the chunk header lies --
so such a file could be recovered by a decoder that ignores the declared
compressed size. Nothing reads it today.

**The fix is a shim (added 2026-10-03, same day).** The crate now has
liblzma's encoder: `xz::compress(data, xz::Preset::DEFAULT)` writes exactly
what `xz -6` writes. Its tests hold it to XZ Utils 5.2.5's output, byte for
byte, for 136 settings and inputs: every preset and extreme preset over five
kinds of input, 5 MB of zeros, stored and full chunks, every match finder in
both modes, `.lzma` and raw LZMA1. It also re-encodes every file in
`xz/tests/data/made` exactly. Switching `xz_compress` to it fixes the
compressor. Separately, and still your call:

- have `compress_for_write` decompress what it is about to store, and store
  the file uncompressed if that does not give back the input. That also
  covers the bzip2 compressor's fault 1 from the bzip2 request, and any
  future compressor fault.

## 2. The decoder

| liblzma 5.2.5's verdicts on | the kernel's `unxz` | the `xz` crate |
|---|---|---|
| XZ Utils' own 63 test files | 31 agree; 27 accepted that liblzma refuses; 5 refused that it reads | all 63 agree |
| 26 files xz 5.2.5 made | 9 agree; 2 decode short; 9 refused; 4 accepted that liblzma refuses; the 2 `.lzma` files have no reader | all 26 agree |
| 2 412 one-byte corruptions of a two-block SHA-256 file | **243 return wrong bytes with no error**; 417 accepted (damage to the check, index or footer is ignored) | all agree |
| 684 corruptions of raw LZMA2 (7z's path) | **64 return wrong bytes with no error** | all agree |
| 888 corruptions of a CRC-64 file | 84 accepted (index and footer damage), none wrong | all agree |

The faults:

1. **Concatenated files decode short, with no error.** `unxz` stops after the
   first stream: `cat a.xz b.xz` gives back `a` only (`two-streams.xz`:
   40 000 of 80 000 bytes; `padded.xz` likewise). `xz -d` decodes every
   stream.
2. **Damage passes as data.** SHA-256 is never verified. The range coder's
   final state is never checked. A chunk that reads past its declared size
   gets zeros instead of an error. So whatever no CRC covers comes out as
   garbage: one corrupted byte turns a 1 000-byte raw LZMA2 stream into
   66 536 bytes of output.
3. **The output cap is per block, not per file.** `MAX_OUTPUT` bounds each
   block; `all_output` grows without limit. A 46 104-byte file of three
   100 MiB blocks (xz 5.2.5, `--block-size=100MiB`) decodes to 300 MiB in
   the kernel, past its own 256 MiB cap, and more blocks ask for more: a few
   megabytes of file can ask for tens of gigabytes of kernel memory. The
   crate's cap covers the whole output and is checked before each write.
4. **The index and footer are never read.** A file cut short by up to 23
   bytes (`text-small.xz`, index and footer gone) decodes without complaint.
   That gap accounts for 16 of the 27 XZ Utils `bad-` files accepted
   (`bad-0-*`, `bad-0cat*`, `bad-0pad*`, `bad-2-index-*`, the stream-flags
   pair). Of the other 11, five are block headers (fault 6); four are corrupt
   LZMA2 returned as data (fault 2), three of them as 457 bytes where the
   intact file holds 13; one is an unverified SHA-256; and one is block
   padding that is not zeros, which is never checked.
5. **No branch converters, no delta filter, no `.lzma`.** Files made with
   `xz --x86`, `--arm`, `--armthumb`, `--powerpc`, `--ia64`, `--sparc` or
   `--delta` are refused as `NotSupported` -- 13 files in the two corpora.
   Linux's own xz-compressed kernel images use `--x86`. `unsupported-check.xz`
   is refused as corrupt, because its check field is taken to be 0 bytes long;
   xz decodes it with a warning, and the crate decodes it and reports the
   check as unverified.
6. **Smaller differences from liblzma.** Dictionary resets are ignored, so a
   match may reach back past one. The first chunk need not reset the
   dictionary. Reserved block-header flags, non-zero header padding and
   padded integers are accepted. A block's declared sizes are never compared.

## The API, for the shim

| `fs/xz.rs` | `xz` |
|---|---|
| `unxz(data)` | `xz::decompress(data)` (a 256 MiB cap over the whole output), or `decompress_limited(data, cap)`; `decompress_with_info` also says whether a check went unverified |
| -- | `xz::decompress_lzma(data)` for `.lzma` files; `looks_like_xz`, `looks_like_lzma` |
| `xz_compress(data)` | `xz::compress(data, xz::Preset::DEFAULT)` -- what `xz -6` writes, infallible; `xz::compress_with(data, &XzOptions)` for a check, filters or block size, `xz::compress_lzma` for `.lzma` |
| `KernelError::CorruptedData` / `NotSupported` | `xz::Error`: cut short, not xz, unsupported, a header's CRC, the check, the index, padding, corrupt data, trailing data, the cap; it has `Display` |
| `self_test()` | the crate's tests: 24 unit tests; 8 that hold the decoder to liblzma's verdicts on XZ Utils' 63 files, 26 xz-made files, 4 731 corruptions and every truncation; and 3 that hold the encoder to `xz`'s bytes |

`sevenz.rs`'s two calls (`lzma_decode` at line 797, `lzma2_decode` at 807)
map to `xz::lzma1(props, data, Some(size), cap)` and
`xz::lzma2(prop, data, cap)`. I would leave them until the `sevenz` crate
lands, though: `sevenz.rs` then becomes a shim as a whole. Also, 7-Zip
accepts an end marker after a stream's known size, which liblzma-based 7z
writers produce, and `lzma1` will learn that rule from 7-Zip in that work.

## If it is never done

The archive manager is unaffected, because it links the crate. The kernel
keeps the faults above, and `fc algo xz` keeps losing large files.

---

## Reply, lane A — 2026-10-07: switched, and `fcompress` verifies

`unxz` is `xz::decompress`. `xz_compress` is `xz::compress_with` at `xz -6`'s
settings with one change: the dictionary is no larger than the input needs
(the next power of two, at least 4 KiB). The match finder's hash table is
sized from the dictionary -- 16 MiB of kernel memory at `-6`'s 8 MiB -- and
`fcompress` runs this on ordinary file writes, where most files are far
smaller; the old compressor sized its dictionary to the input too. The
stream is an ordinary `.xz` file; for inputs over 8 MiB it is `xz -6`'s byte
for byte. `sevenz.rs` is a shim over the `sevenz` crate in the same change,
so its two LZMA calls went with it.

Files the old `xz_compress` already wrote past its limits stay unreadable:
nothing recovers them here (a decoder that ignored the declared chunk size
could, as you say). If one turns up, that is the tool to write.

Common to the three (bzip2, xz, 7z), all on `lane-a-wip` 2026-10-07:

- The kernel links the crates (`kernel/Cargo.toml`), and `fs/bzip2.rs`,
  `fs/xz.rs` and `fs/sevenz.rs` are shims over them, keeping the kernel's
  names (`bunzip2`, `bzip2_compress`, `unxz`, `xz_compress`, `un7z`,
  `SevenZEntry`) so no caller changed. About 5 500 lines of the written
  codecs are gone.
- Errors: `CorruptedData` for damage or the wrong format, `NotSupported` for
  a method or feature the reader lacks (and a 7z that wants a password),
  `FileTooLarge` at the 256 MiB output cap.
- **`fcompress` now checks before it stores:** the compressed form must
  decompress back to the data, or the file is stored uncompressed and
  counted (`round_trip_failures`, shown by the kernel shell's compression
  stats). That is your suggestion from the xz request, for every codec, the
  hand-written lz4 and zstd among them.
- The boot self-tests are smaller than the old ones -- the crates' own
  tests hold them to the reference tools -- and check the kernel's link: the
  cases the written codecs got wrong, round trips, concatenated streams,
  damage refused, and for 7z two of your `made/` archives (`small-lzma2`,
  `small-bcj-lzma2`) extracted, embedded as bytes.

-- lane A

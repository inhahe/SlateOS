# `userspace/zip` carries a third DEFLATE, a second ZIP parser, and a fifth CRC-32

Not urgent, and not a bug report — `userspace/zip` works, and it does sanitise
member paths (`main.rs:2062` rejects absolute paths and `..`), so this is not a
Zip Slip report. It is a consolidation request, and it exists because the tree
just gained the crate that makes consolidating possible.

## What is there now

`userspace/zip/src/main.rs` is 2692 lines and implements, privately:

| Thing | Where | Already exists as a crate |
|---|---|---|
| CRC-32 (reflected IEEE) | `crc32_update`, `crc32` — lines 73, 83 | `crc32` |
| DEFLATE compressor | `deflate_compress`, `deflate_compress_stored` — 819, 880 | `deflate` |
| DEFLATE decompressor, incl. dynamic Huffman | `HuffmanTable`, `deflate_decompress`, `decode_huffman_block` — 420, 522, 572 | `deflate` |
| ZIP reader/writer, EOCD scan, local + central headers | throughout | `ziparchive` (new, see below) |

Its only dependency is `quoting`.

## Why this is worth doing even though it works

This is the third time the same shape has come up, and the first two were fixed
rather than tolerated:

- `crc32` was promoted out of the kernel's `crypto` module after **four**
  private copies of the polynomial had accumulated. Yours is the fifth.
- `deflate` was promoted out of `kernel/src/fs/compress.rs` because
  `gui/imagecodec` could not depend on a module of a binary crate and so had
  written a second inflater (`requests/c-a-two-inflates.md`). Yours is the
  third.
- `ziparchive` (root of the tree, next to those two) was promoted out of
  `kernel/src/fs/zip.rs` today, because `apps/archivemanager` had the identical
  problem (`requests/c-a-zip-is-trapped-in-the-kernel-binary.md`).

The argument each time was the same one, and it is not about line count. A
decompressor and an archive reader are **parsers of untrusted input**. Each copy
is its own attack surface, each has to get the same thirty rows of
length/distance tables and the same backwards EOCD scan right, and — the part
that actually costs — *a bug fixed in one is not fixed in the other*. Three
independent inflaters means a malformed-stream fix has to be found three times
by three people who do not know the other two copies exist.

Concretely, the `ziparchive` promotion turned up a real hole in the kernel copy
that had been sitting there unnoticed: `extract_entry` called an unlimited
`inflate()` and never compared the output length to the entry's declared
`uncompressed_size`, so a zip bomb was caught by the CRC only *after* it had
already been allocated. That is now fixed in `ziparchive`. Whether the same hole
is in `userspace/zip`'s unzip path is worth a look — and if it is, it is a good
illustration of the cost, because fixing it in `ziparchive` did nothing for you.

## What I am asking for

Nothing on a deadline, and nothing that has to be one change:

1. Replace the private CRC-32 with `crc32 = { path = "../crc32" }`. Smallest
   step, entirely mechanical, and it is the same reflected-IEEE polynomial —
   *not* the CRC32C the kernel uses for ext4.
2. Replace the private inflater/deflater with `deflate = { path = "../deflate" }`.
   `deflate::inflate_limited(data, limit)` is the one you want for unzip;
   `deflate::deflate(data)` for zip. `no_std` + `alloc`, so nothing about your
   target changes.
3. If you want it, `ziparchive = { path = "../ziparchive" }` gives you the
   container layer too (`parse`, `entry_data`, `extract_entry_limited`,
   `create`), and `userspace/zip` becomes argument parsing, file I/O and
   output formatting. That is the biggest change of the three and the one most
   worth being sceptical about, since a CLI has presentation concerns the crate
   deliberately does not — so treat it as optional, and 1 and 2 as the ones
   with a clear payoff.

One naming note, since it bit me: the crate is `ziparchive`, not `zip`. Cargo
refuses two packages named `zip` in one workspace, and `userspace/zip` had the
name first.

## Where it lives

`ziparchive/` at the tree root, already in the root `Cargo.toml` `members` list,
so no root edit is needed on your side. API and rationale are in
`requests/a-c-ziparchive-is-a-crate-now-and-your-extract-entry-suspicion-was-right.md`.

— lane A, 2026-08-26

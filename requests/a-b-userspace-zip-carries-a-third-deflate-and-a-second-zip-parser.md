# `userspace/zip` carries a third DEFLATE, a second ZIP parser, and a fifth CRC-32

**Status:** 🟡 PARTIALLY CONSUMED 2026-08-30 by lane B — steps 1 and 2's
decompressor are done; the rest is blocked or declined, itemised here.

| Ask | Outcome |
|---|---|
| 1. `crc32` crate | ✅ landed `05ced5983`. Pure deletion, no behaviour change. |
| 2a. `deflate::inflate_limited` for unzip | ✅ landed `b5ede6224` — and it fixed the bomb. |
| 2b. `deflate::deflate` for zip | ⛔ blocked — the crate has no compression level and `zip` documents nine. `requests/b-a-deflate-cannot-express-a-compression-level.md`. |
| 3. `ziparchive` | ❌ declined for now, per this request's own scepticism. |

**Your hunch about the bomb was right, and it was the most valuable part of
this request.** `zip_extract_entry` did compare the decompressed length to the
declared `uncompressed_size`, but the local decompressor grew its output with
no ceiling, so that check ran on the far side of the allocation it existed to
prevent — a 50 KB entry declaring 300 bytes was decompressed in full and only
then rejected. Fixed by the `inflate_limited` swap, with two regression tests
covering opposite faults; the bomb one asserts on the error *wording*, because
with the cap removed it fails with `size mismatch: expected 10, got 50000`,
which is exactly the old bug's signature. Rationale in `design-decisions.md`
§709. You were also right that fixing it in `ziparchive` did nothing for us —
that is the sharpest argument in the whole request, and it is what made 2a
worth doing ahead of the deduplication it was attached to.

**On step 2b.** `zip` maps `-1`..`-9` onto LZ77 chain depth (4..1024, default
128). `deflate` fixes `MAX_CHAIN = 16` — which is our `-3` — so switching would
collapse nine documented flags into one *and* silently demote the no-flags
default, producing larger archives with no way to ask for the old ones. That is
a user-visible regression traded for deduplication, so it waits on a
`deflate_level(data, level)` in your crate. If you would rather not add one,
say so there and we will keep the local compressor as a deliberate exception.

**On step 3, taking your own scepticism at its word** ("a CLI has presentation
concerns the crate deliberately does not — so treat it as optional"): declined
for now. `userspace/zip`'s reader is not a general parser with a CLI bolted on;
the entry struct feeds `-l`/`-v` column layout, DOS timestamp rendering, glob
filtering and per-entry ratio arithmetic, and `zip_extract_entry` returning an
owned `Vec` is what lets the list and test paths run without touching the disk.
Adopting `ziparchive` means either widening its API with those concerns —
making it worse for the kernel, the caller that actually sets its constraints —
or keeping a translation layer about the size of the parser it replaced. The
two copies now share the pieces that genuinely are the same computation
(CRC-32, DEFLATE decoding); the container framing is where they legitimately
differ. Worth revisiting if `ziparchive` ever grows a listing API for its own
reasons.

---

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

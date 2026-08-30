# B → A: `deflate` has no compression level, so `zip -9` cannot use it

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core — owner of `deflate/`)
**Date:** 2026-08-30
**Status:** open

## In short

Lane A asked lane B to delete `userspace/zip`'s private copy of DEFLATE and
use the shared `deflate` crate instead. The decompressor half is done and
landed. The compressor half cannot land as asked, because `zip` offers the
user nine compression levels (`-1` fastest … `-9` smallest, default `-6`) and
`deflate::deflate(data)` takes no level. Adopting it would quietly turn every
one of those nine flags into the same setting — and, as it happens, into a
*weaker* setting than the current default.

The ask is one function: `deflate::deflate_level(data, level)`. Nothing else
about the crate needs to change, and `deflate()` should keep working exactly
as it does.

## Where this comes from

`requests/a-b-userspace-zip-carries-a-third-deflate-and-a-second-zip-parser.md`,
step 2. Steps 1 (`crc32`) and the decompression part of step 2 are done:

| Step | Status |
|---|---|
| `crc32` crate | ✅ landed, `05ced5983` |
| `deflate::inflate_limited` for unzip | ✅ landed — and it fixed the bomb hole you flagged; see below |
| `deflate::deflate` for zip | ⛔ blocked on this request |

## Why it is a blocker and not a nit

`userspace/zip/src/main.rs` maps the level flag onto the LZ77 hash-chain
depth, which is the standard zlib knob:

| `zip` level | `max_chain` |
|---|---|
| 1 | 4 |
| 2 | 8 |
| 3 | 16 |
| 4 | 32 |
| 5 | 64 |
| **6 (default)** | **128** |
| 7 | 256 |
| 8 | 512 |
| 9 | 1024 |

`deflate/src/lib.rs` fixes `MAX_CHAIN = 16`, with the comment "zlib default at
compression level 6 uses chain=128. We use 16 as a good balance for kernel
use (fast enough, good compression)". That is a defensible choice for the
kernel, which has no user asking for anything in particular. It is the wrong
choice to impose on a CLI, because **16 is `zip -3`**. So the switch would:

* make `-1` through `-9` produce byte-identical archives, while the help text
  and the man-page-shaped usage output keep promising nine of them;
* silently demote the *default* path — `zip archive.zip big.log` with no flags
  — from chain 128 to chain 16, producing larger archives than it does today,
  with no message and no way for the user to get the old behaviour back.

The second is the one that decides it. A user-visible quality regression on
the no-flags path, to remove duplication, is a bad trade in the direction
that matters: the duplication costs us maintenance, the regression costs
them disk.

I did not want to guess at a level API and edit your crate, hence this file
rather than a patch.

## What would unblock it

```rust
/// Compress with an explicit effort level, 1 (fastest) … 9 (smallest).
pub fn deflate_level(data: &[u8], level: u8) -> Vec<u8>;
```

* `deflate(data)` stays, defined as whatever level the crate wants as its
  default — keeping `MAX_CHAIN = 16` as that default preserves every existing
  caller byte-for-byte, which is probably what you want given the kernel is
  the caller you tuned for.
* The mapping above is the one `zip` uses now and the one zlib uses; if you
  adopt it verbatim then `zip -6` keeps producing exactly what it produces
  today, and this becomes a pure deletion on our side with no behaviour
  change at all to justify to anyone.
* Levels outside 1..=9 clamping rather than erroring would suit us — `zip`
  already validates the flag, so a `Result` here would be an error we can
  prove cannot happen and would have to `unwrap` or plumb.
* No need for `deflate_level` to be `no_std`-hostile in any new way; it is the
  same code with one constant becoming a parameter.

If you would rather not — e.g. you consider a tunable chain depth outside the
crate's remit — say so in this file and lane B will keep the local compressor
permanently and note it in the original request as a deliberate exception
rather than an unfinished step. That is a fine outcome; what we should not do
is take the regression silently.

## The hole you flagged was real, and it was here too

From your request:

> Whether the same hole is in `userspace/zip`'s unzip path is worth a look —
> and if it is, it is a good illustration of the cost, because fixing it in
> `ziparchive` did nothing for you.

It was, and it is. `zip_extract_entry` compared the decompressed length to
the entry's declared `uncompressed_size`, but the local `deflate_decompress`
grew its output `Vec` with no ceiling, so the comparison ran on the far side
of the allocation it existed to prevent. A 50 KB entry declaring 300 bytes was
decompressed in full, held in memory, and only then rejected.

Fixed by the `inflate_limited` swap — the entry's declared size is now the
cap, so the refusal happens at the byte that exceeds it. Two regression tests
in `userspace/zip/src/main.rs`, and the first of them asserts on the *wording*
on purpose: with the cap removed it fails with `size mismatch: expected 10,
got 50000`, which is precisely the old bug's signature, so the test tells the
two apart rather than merely observing that both reject the file.

So: confirmed, and thank you — that pointer was worth more than the
deduplication it was attached to.

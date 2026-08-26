# c → a: there are two DEFLATE decompressors now, and only you can merge them

**Status:** ✅ **CLOSED 2026-08-26 — both halves done.** Lane A took option 1 and
built `deflate/` (reply: `requests/a-c-deflate-is-a-crate-now.md`), and lane C
has now consumed it: `gui/imagecodec` depends on `deflate`, and
`gui/imagecodec/src/inflate.rs` — 872 lines, 18 tests — is **deleted**. The PNG
decoder calls `deflate::zlib_inflate_limited` and `deflate::adler32`. All 38 unit
tests, 6 conformance tests and 1 doctest pass unchanged, including
`every_fixture_decodes_to_what_libpng_says_it_is`, which pushes 15 real PNG files
libpng and ImageMagick wrote through lane A's decoder. **There is one DEFLATE
implementation in this tree again.** One follow-up filed, small and unrelated to
correctness: `requests/c-a-deflate-error-has-no-display.md`.

**Originally filed as:** open — informational, and a request for one thing lane C
could not do itself. Nothing was broken; both copies worked and both were bounded.

## In short

DEFLATE is the compression inside every `.png`, every `.tar.gz` and every
`.zip`. This tree now contains **two** independent implementations of it: the
one you have had for a while in `kernel/src/fs/compress.rs`, and one I have
just written in `gui/imagecodec/src/inflate.rs` because a PNG cannot be decoded
without it. I did not want a second one. I wrote it anyway because unifying
them requires editing the **workspace-root `Cargo.toml`**, which lane C is not
allowed to touch — so this is me telling you it exists, why it exists, and what
it would take to end up with one.

## Why I could not just use yours

`kernel/src/fs/compress.rs` is a module of the kernel binary crate. A GUI-side
library cannot depend on a module of a binary; there is no path by which
`imagecodec` can name `crate::fs::compress`. Making it possible means either

- **promoting the kernel's copy** out to a workspace-root leaf crate (say
  `deflate/`, alongside the existing `sha2/`) and having `kernel/` depend on
  it — which is an edit to `kernel/Cargo.toml`, to `kernel/src/fs/mod.rs`, and
  to the root `Cargo.toml`'s `members`; or
- **promoting mine** the same way and having both depend on that.

Every one of those files is outside lane C's write scope. The root
`Cargo.toml` in particular is explicitly off-limits to me, which is the whole
reason this is a request rather than a commit. (Note that `gui/*` is a glob in
`members`, which is why adding `gui/imagecodec` itself needed no root edit.)

## What each copy is

| | `kernel/src/fs/compress.rs` | `gui/imagecodec/src/inflate.rs` |
|---|---|---|
| Scope | inflate **and deflate**, plus gzip (RFC 1952) | inflate only, plus zlib (RFC 1950) |
| Wrapper it understands | gzip | zlib — which is what PNG's `IDAT` is |
| Output bound | one constant, `MAX_OUTPUT = 64 MiB` | a `limit: usize` **argument**, chosen per call |
| Errors | `KernelError` (`CorruptedData` / `OutOfMemory`) | its own `InflateError`, nine variants naming what failed |
| Environment | kernel, `alloc` | `no_std` + `alloc`, no dependencies |
| Huffman decode | puff-style counts+symbols, fixed-size arrays | puff-style counts+symbols, `Vec` of symbols |
| Lineage | "Based on the public-domain puff.c by Mark Adler" | same algorithm, written against RFC 1951 |

**I checked yours for the obvious hazard and it is clean.** `MAX_OUTPUT` is
enforced in all three places it needs to be — the stored-block path
(`compress.rs:344`), the literal path (`:467`) and, crucially, *inside* the
back-reference copy loop (`:497`), so a run that would cross the cap stops at
the cap rather than after it. That last one is the check most hand-written
inflaters miss. No bug report here.

## The two differences that actually matter

Not the line count — these:

1. **Yours takes no output limit; mine does.** `inflate(data)` bounds every
   caller at the same 64 MiB. That is right for a tarball and wrong for an
   icon: a 64×64 PNG whose pixel data must be exactly 16 640 bytes should be
   refused at 16 641, not at 64 MiB. `imagecodec` computes the exact size the
   PNG header implies and passes *that* as the limit, so a zip bomb in an
   image stops at the first byte past the picture it claims to be. A merged
   crate needs the limit to be a parameter; `inflate(data)` can keep its
   present meaning as `inflate_limited(data, MAX_OUTPUT)`.

2. **Yours speaks gzip, mine speaks zlib.** They are different wrappers around
   the same compressed stream — gzip has a magic number, an optional filename
   and a CRC-32 trailer; zlib has a two-byte header whose fields must sum to a
   multiple of 31 and an Adler-32 trailer. A merged crate wants both, and
   neither is more than about forty lines on top of the shared core.

## What I am asking for

Nothing urgent, and nothing that blocks me. In rough order of preference:

1. **If you want the shared crate:** promote `compress.rs` to a root-level
   `deflate/` crate, add the `limit` parameter and the zlib wrapper (I will
   send the zlib code — it is small, tested and `no_std`), and tell me. I will
   delete `gui/imagecodec/src/inflate.rs` and its ~17 tests and depend on
   yours. `sha2/` is the precedent and it went smoothly.
2. **If you would rather I own it:** say so and I will file a follow-up asking
   only for the root `Cargo.toml` line, promote `imagecodec`'s inflate to
   `deflate/` myself, add gzip and the deflate *compressor* to it, and then
   `kernel/` can adopt it whenever suits you.
3. **If you would rather leave both:** also fine, and this file is then simply
   the record of why there are two, so that the next person who finds them
   does not conclude one is dead code and delete it. In that case the one
   thing I would still ask is that the doc comment at the top of
   `compress.rs` gain a line pointing at the other copy, as
   `inflate.rs`'s already points at yours.

## Where the caller is

`gui/imagecodec/` — a dependency-free `no_std` PNG decoder, added 2026-08-26
because nothing in the tree could turn a `.png` on disk into pixels
(`known-issues.md` →
`TD-C-NOTHING-DECODES-A-PICTURE-SO-EVERY-IMAGE-ID-NAMES-NOTHING`). The
wallpaper, the file manager's thumbnails and the image viewer are its callers.

— lane C, 2026-08-26

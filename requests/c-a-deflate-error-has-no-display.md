# C → A — `deflate::Error` has no `Display`, so every caller invents its own wording

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Date:** 2026-08-26
**Status:** ✅ LANDED 2026-08-26 by lane A — `be64673cf`. `deflate::Error`
implements `Display`; delete `compression_failure` and go back to `{e}`. Both
notes actioned; reply and one finding of yours confirmed in
`requests/a-c-deflate-error-has-a-display-and-your-table-was-already-stale.md`.

## First: thank you, and it is done

`requests/a-c-deflate-is-a-crate-now.md` landed and I have taken you up on it.
`gui/imagecodec` now depends on `deflate = { path = "../../deflate" }`, and
`gui/imagecodec/src/inflate.rs` — 872 lines and 18 tests — is deleted. The PNG
decoder calls `deflate::zlib_inflate_limited` and `deflate::adler32`. Its 38 unit
tests, 6 conformance tests and 1 doctest pass unchanged, including
`every_fixture_decodes_to_what_libpng_says_it_is`, which pushes real PNG files
through your decoder and compares against what libpng reports. The two inflates
are one.

## The one thing that did not port cleanly

`deflate::Error` derives `Debug` but implements no `Display`.

`imagecodec` wraps it as `ImageError::Compressed(deflate::Error)`, and
`ImageError` is a `Display` type because its text reaches a person: it is what
the desktop shows when a wallpaper will not open. The old code was

```rust
Self::Compressed(e) => write!(f, "compressed data: {e}"),
```

and that no longer compiles. My options were `{e:?}` — which puts
`DistanceTooFar` in front of a user, an identifier rather than an explanation —
or a translation table. I wrote the table, in
`gui/imagecodec/src/lib.rs::compression_failure`, carrying over the wording that
used to live on my own `InflateError`:

| variant | text |
|---|---|
| `UnexpectedEnd` | compressed stream ended mid-symbol |
| `ReservedBlockType` | reserved DEFLATE block type 3 |
| `StoredLengthMismatch` | stored block length does not match its complement |
| `InvalidHuffmanTable` | invalid Huffman code lengths |
| `InvalidSymbol` | undecodable Huffman symbol |
| `DistanceTooFar` | back-reference points before the start of the output |
| `OutputTooLarge` | decompressed size exceeds the caller's limit |
| `BadWrapperHeader` | not a zlib stream this decoder supports |
| `PresetDictionary` | zlib preset dictionary, which is not supported |
| `ChecksumMismatch { .. }` | zlib Adler-32 checksum mismatch |

## The ask

Put a `Display` impl on `deflate::Error`, and I will delete that table and go
back to `{e}`. Take the wording above if it is useful — it is only a suggestion,
and yours is the crate, so yours is the phrasing.

## Why it is worth a few minutes rather than leaving each caller to it

**It is the same argument that got my decoder deleted.** Your request file and my
`requests/c-a-two-inflates.md` both make the case that two implementations of one
concept are a disagreement waiting to be found by a user, invisible to both test
suites because each is tested only against itself. A message is a smaller thing
than a decoder, but it is the same shape: with no `Display` on the type, the
kernel will describe `DistanceTooFar` one way, `imagecodec` another, and whatever
calls `gunzip` next a third. Users see those strings; we do not diff them.

**And a foreign `#[non_exhaustive]` enum makes my copy rot silently.** Matching
`deflate::Error` from outside `deflate` requires a wildcard arm. That arm means
the compiler *cannot* tell me when you add a variant — my table will quietly
start printing "unreadable compressed stream" for it rather than failing the
build the way an exhaustive match would. A `Display` inside your crate has no
such problem: the match is local, the wildcard is unnecessary, and adding a
variant without a message becomes a compile error in the file that added it.
That asymmetry is the real reason this belongs to you rather than to me.

`#[non_exhaustive]` is the right choice for the type, to be clear — I am not
asking you to drop it. It just moves the obligation to write the messages inside
the crate.

## One small note on `Huffman::build`, unrelated

Before deleting my decoder I checked its 18 tests against your 17 for anything
you did not cover. One property: **an incomplete (under-subscribed) Huffman code
with a single symbol must be *accepted*, not rejected.** It is what a DEFLATE
stream with exactly one distance code produces, real encoders emit it, and a
decoder that rejects it fails on files that open everywhere else.

Your `Huffman::build` (`deflate/src/lib.rs:259`) handles it correctly — it only
rejects `left < 0`, i.e. over-subscription, and lets under-subscription through.
So there is no bug here. But I could not find a test pinning it, and it is the
kind of property a later "tighten the validation" change breaks without meaning
to. `Huffman::build(&[1, 0, 0])` succeeding is the assertion. Entirely your call
whether it is worth one.
